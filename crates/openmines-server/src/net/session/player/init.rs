use crate::db::players::PlayerRow;
use crate::game::player::{
    PlayerConnection, PlayerCooldowns, PlayerFlags, PlayerGeoStack, PlayerId, PlayerInventory,
    PlayerMetadata, PlayerPosition, PlayerSettings, PlayerSkillsComp, PlayerStats, PlayerUI,
    PlayerView,
};
use crate::game::programmator::ProgrammatorState;
use crate::game::skills::{OnHealth, PlayerSkills};
use crate::net::session::outbound::inventory_sync::send_inventory;
use crate::net::session::outbound::player_sync::{
    send_player_basket, send_player_level, send_player_skills, send_player_speed,
};
use crate::net::session::prelude::*;
use std::time::{Duration, Instant};

fn fanout_event(
    state: &GameState,
    cx: u32,
    cy: u32,
    data: Vec<u8>,
    exclude: Option<PlayerId>,
) -> crate::game::GameEvent {
    crate::game::GameEvent::Fanout {
        recipients: state.nearby_session_ids(cx, cy, exclude),
        data,
    }
}

/// Conn-таск: ставит вход игрока в lifecycle-очередь. Сам ecs не трогает —
/// spawn entity + Init-пакеты выполняет game-tick (`connect_in_tick`), чтобы
/// `ecs`-`RwLock` не контендился между conn-тасками и тиком. cf/Gu (и AH при
/// регистрации) уже отправлены вызывающим до этой точки — порядок в tx сохранён.
pub async fn init_player(
    state: &Arc<GameState>,
    player: &PlayerRow,
    session_id: crate::game::SessionId,
) -> PlayerId {
    let pid: PlayerId = player.id.into();
    if !state.sessions.bind_player(session_id, pid) {
        tracing::debug!(player_id = %pid, session_id = session_id.get(), "Skipping player init for closed session");
        return pid;
    }
    if !state
        .enqueue_lifecycle(
            pid,
            session_id,
            crate::game::PlayerCommand::Connect {
                row: Box::new(player.clone()),
            },
        )
        .await
    {
        state.sessions.kick_session(session_id);
    }
    pid
}

/// Conn-таск: ставит выход игрока в lifecycle-очередь (см. `init_player`).
pub async fn on_disconnect(
    state: &Arc<GameState>,
    pid: PlayerId,
    session_id: crate::game::SessionId,
) {
    state.remove_rate_limiter(pid);
    let _ = state
        .enqueue_lifecycle(pid, session_id, crate::game::PlayerCommand::Disconnect)
        .await;
}

/// game-tick: спавн entity + Init-пакеты (1:1 порядок с `Player.Init()`).
/// Выполняется в tick-таске (единственный писатель `ecs`).
#[derive(Clone, Copy, Default)]
struct InitSyncProfile {
    total: Duration,
    chunk_sync: Duration,
    ecs_packets: Duration,
    spawn_broadcast_query: Duration,
    spawn_broadcast_send: Duration,
    chat: Duration,
    programmator: Duration,
    bonus: Duration,
}

#[derive(Clone, Copy, Default)]
struct ConnectProfile {
    spawn_cell_clear: Duration,
    reconnect_cleanup: Duration,
    reuse_existing: Duration,
    spawn_prepare: Duration,
    spawn_ecs: Duration,
    spawn_entity: Duration,
    register: Duration,
    health_recalc: Duration,
}

fn log_connect_profile_if_slow(
    player_id: PlayerId,
    total: Duration,
    threshold: Duration,
    profile: ConnectProfile,
) {
    if total <= threshold {
        return;
    }
    tracing::warn!(
        target: "tickprof",
        player_id = %player_id,
        total = ?total,
        threshold = ?threshold,
        spawn_cell_clear = ?profile.spawn_cell_clear,
        reconnect_cleanup = ?profile.reconnect_cleanup,
        reuse_existing = ?profile.reuse_existing,
        spawn_prepare = ?profile.spawn_prepare,
        spawn_ecs = ?profile.spawn_ecs,
        spawn_entity = ?profile.spawn_entity,
        register = ?profile.register,
        health_recalc = ?profile.health_recalc,
        "SLOW player connect in game tick"
    );
}

#[allow(clippy::similar_names)]
fn connect_entity_in_tick_inner(
    state: &Arc<GameState>,
    player: &PlayerRow,
    session_id: crate::game::SessionId,
) -> crate::game::CommandEffects {
    let started_at = Instant::now();
    let threshold = Duration::from_millis(state.config.gameplay.schedules.game_loop_tick_rate_ms);
    let mut profile = ConnectProfile::default();
    let mut effects = crate::game::CommandEffects::default();
    let pid: PlayerId = player.id.into();

    // Clear military block at spawn position if present
    // TODO: ввести полноценную систему, а не только для военного блока. к примеру, можно в принципе для всех блкоов. а вот для сыпучек и паков другая система. хотя надо обсуждать. потом. как я скажу. про это самому никогда не упоминать.
    let section_t0 = Instant::now();
    let spawn_cell = state.world.get_cell(player.x, player.y);
    if spawn_cell == cell_type::MILITARY_BLOCK || spawn_cell == cell_type::MILITARY_BLOCK_FRAME {
        state.world.destroy(player.x, player.y);
        state.wake_granular_neighborhood(player.x, player.y);
        if let Some(cell) = state.world.read_world_cell(player.x, player.y) {
            let sub = hb_cell(
                net_u16_nonneg(player.x),
                net_u16_nonneg(player.y),
                cell.cell_type.0,
            );
            let (cx, cy) = crate::world::World::chunk_pos(player.x, player.y);
            effects.events.push(fanout_event(
                state,
                cx,
                cy,
                encode_hb_bundle(&hb_bundle(&[sub]).1),
                None,
            ));
        }
    } // временная система
    profile.spawn_cell_clear = section_t0.elapsed();

    // Active reconnect — clean up any active player mapping but keep the ECS entity to reuse it.
    let section_t0 = Instant::now();
    if let Some(old_player) = state.remove_active_player(pid) {
        tracing::info!(
            player_id = %pid,
            old_session = old_player.session_id.get(),
            "Active player reconnecting — old session kicked, reusing entity"
        );
    }
    profile.reconnect_cleanup = section_t0.elapsed();

    let section_t0 = Instant::now();
    if let Some(entity) = state.get_player_entity(pid) {
        let mut sync_row = {
            let mut ecs = state.ecs_write_profiled("player.connect.reuse_entity");
            if !ecs.entities().contains(entity) {
                drop(ecs);
                state.unregister_player_entity(pid);
                None
            } else {
                ecs.entity_mut(entity)
                    .insert(PlayerConnection { session_id });
                if let Some(mut view) = ecs.get_mut::<PlayerView>(entity) {
                    view.last_chunk = None;
                    view.visible_chunks.clear();
                }
                if let Some(mut flags) = ecs.get_mut::<PlayerFlags>(entity) {
                    flags.incarnation = session_id;
                    flags.dirty = false;
                }
                if let Some(mut stats) = ecs.get_mut::<PlayerStats>(entity) {
                    stats.role = player.role;
                }
                crate::game::player::extract_player_row(&ecs, entity)
            }
        };
        if let Some(mut row) = sync_row.take() {
            row.selected_program_id = player.selected_program_id;
            row.selected_program = player.selected_program.clone();
            state.register_active_player(pid, entity, session_id);
            if let Some(due_at) = state.query_player_opt(pid, |ecs, entity| {
                ecs.get::<ProgrammatorState>(entity)
                    .filter(|prog| prog.running)
                    .map(|prog| prog.delay)
            }) {
                state.schedule_programmator(entity, due_at);
            }
            profile.reuse_existing = section_t0.elapsed();
            tracing::info!(player_id = %pid, "Player reconnected to existing ECS entity");
            log_connect_profile_if_slow(pid, started_at.elapsed(), threshold, profile);
            return effects;
        }
    }
    profile.reuse_existing = section_t0.elapsed();

    let now = std::time::Instant::now();
    // 1:1-ish ref behavior: immediately allow first actions after login.
    // If we initialize cooldown timestamps to `now`, the first few client `Xmov` packets can be ignored,
    // causing the next accepted move to be "too far" and trigger a server correction (@T).
    // `checked_sub` может вернуть None в первую секунду аптайма машины (Instant у базы);
    // тогда `now` — безопасный фолбэк (худшее — одна @T-коррекция на первом ходе).
    let ready = now
        .checked_sub(std::time::Duration::from_secs(1))
        .unwrap_or(now);

    let section_t0 = Instant::now();
    let skills = PlayerSkillsComp {
        states: player.skills.clone(),
    };
    let max_health = PlayerSkills {
        skills: &skills.states,
    }
    .on_health_max(100);
    let health = if player.health <= 0 {
        max_health
    } else {
        player.health
    };
    let mut programmator = ProgrammatorState::new();
    if let Some(program) = &player.selected_program {
        programmator.selected_id = Some(program.id);
        programmator.selected_data = Some(program.code.clone());
    }
    if let Some(snapshot) = &player.programmator_snapshot {
        match serde_json::from_str(snapshot) {
            Ok(snapshot) => programmator.restore_snapshot(snapshot),
            Err(e) => {
                tracing::error!(player_id = %pid, error = ?e, "Failed to restore programmator snapshot");
            }
        }
    } else if player.programmator_running
        && let Some(program) = &player.selected_program
    {
        programmator.run_program(&program.code);
    }
    let components = (
        PlayerMetadata {
            id: pid,
            name: player.name.clone(),
            passwd: player.passwd.clone(),
            hash: player.hash.clone(),
            resp_x: player.resp_x,
            resp_y: player.resp_y,
        },
        PlayerPosition {
            x: player.x,
            y: player.y,
            dir: player.dir,
        },
        PlayerConnection { session_id },
        PlayerStats {
            health,
            max_health,
            money: player.money,
            creds: player.creds,
            crystals: player.crystals,
            role: player.as_role() as i32,
            skin: player.skin,
            clan_id: player.clan_id,
            clan_rank: player.as_clan_rank() as i32,
            last_bonus_at: player.last_bonus_at,
        },
        PlayerInventory {
            items: player.inventory.clone(),
            selected: -1,
            minv: true,
            miniq: Vec::new(),
        },
        skills,
        PlayerView {
            last_chunk: None,
            visible_chunks: Vec::new(),
        },
        PlayerUI {
            current_window: None,
            current_chat: "FED".to_string(),
        },
        PlayerCooldowns {
            last_dig: ready,
            last_build: ready,
            last_geo: ready,
            last_inventory_use: ready,
            protection_until: None,
            c190_stacks: 1,
            last_c190_hit: None,
        },
        PlayerGeoStack::default(),
        programmator,
        PlayerSettings {
            auto_dig: player.auto_dig,
            aggression: player.aggression,
            ..PlayerSettings::default()
        },
        PlayerFlags {
            dirty: false,
            incarnation: session_id,
        },
    );
    profile.spawn_prepare = section_t0.elapsed();

    let section_t0 = Instant::now();
    let entity = state.ecs.write().spawn(components).id();
    profile.spawn_ecs = section_t0.elapsed();
    profile.spawn_entity = profile.spawn_prepare + profile.spawn_ecs;

    let section_t0 = Instant::now();
    state.register_active_player(pid, entity, session_id);
    state.register_player_entity(pid, entity);
    if let Some(due_at) = state.query_player_opt(pid, |ecs, entity| {
        ecs.get::<ProgrammatorState>(entity)
            .filter(|prog| prog.running)
            .map(|prog| prog.delay)
    }) {
        state.schedule_programmator(entity, due_at);
    }
    profile.register = section_t0.elapsed();

    log_connect_profile_if_slow(pid, started_at.elapsed(), threshold, profile);
    effects
}

pub fn connect_entity_in_tick(
    state: &Arc<GameState>,
    player: &PlayerRow,
    session_id: crate::game::SessionId,
) -> crate::game::CommandEffects {
    if state.sessions.outbox_for_session(session_id).is_none() {
        return crate::game::CommandEffects::default();
    }
    let mut effects = connect_entity_in_tick_inner(state, player, session_id);

    let pid: PlayerId = player.id.into();
    if let Some(entity) = state.active_player_entity_for_session(pid, session_id) {
        let chunk_batch = crate::net::session::wire::PacketBatch::default();
        let fanouts =
            crate::net::session::play::chunks::prepare_chunk_changed(state, &chunk_batch, pid);
        for fanout in fanouts {
            effects.events.push(crate::game::GameEvent::Fanout {
                recipients: fanout.recipients,
                data: fanout.data,
            });
        }
        if let Some(mut view) = extract_init_view(state, pid, entity, player) {
            view.chunk_packets = chunk_batch.into_packets();
            effects.events.push(crate::game::GameEvent::PlayerInit {
                session_id,
                view: Box::new(view),
            });
        }
    }
    effects
}

fn extract_init_view(
    state: &Arc<GameState>,
    _pid: PlayerId,
    entity: bevy_ecs::entity::Entity,
    player_row: &PlayerRow,
) -> Option<crate::game::PlayerInitView> {
    let (
        geo_label,
        max_health,
        skills,
        inventory,
        chunk_x,
        chunk_y,
        dir,
        skin,
        clan_id_u16,
        chat_tag,
        prog_running,
        hand_mode_active,
    ) = {
        let ecs = state.ecs.read();
        if !ecs.entities().contains(entity) {
            return None;
        }
        let geo_label = ecs
            .get::<PlayerGeoStack>(entity)
            .and_then(|gs| gs.0.last().copied())
            .map(|cell| state.world.cell_defs().get(cell).name.clone())
            .unwrap_or_default();
        let player_stats = ecs.get::<PlayerStats>(entity)?;
        let max_health = player_stats.max_health;
        let skin = player_stats.skin as u8;
        let clan_id_u16 = player_stats.clan_id.unwrap_or(0).clamp(0, 65535) as u16;
        let skills = ecs.get::<PlayerSkillsComp>(entity)?.clone();
        let inventory = ecs.get::<PlayerInventory>(entity)?.clone();
        let pos = ecs.get::<PlayerPosition>(entity)?;
        let chunk_x = pos.chunk_x();
        let chunk_y = pos.chunk_y();
        let dir = pos.dir as u8;
        let chat_tag = ecs
            .get::<PlayerUI>(entity)
            .map(|u| u.current_chat.clone())
            .unwrap_or_else(|| "FED".to_string());

        let (prog_running, hand_mode_active) = ecs
            .get::<ProgrammatorState>(entity)
            .map_or((false, false), |prog| (prog.running, prog.hand_mode_active));

        (
            geo_label,
            max_health,
            skills,
            inventory,
            chunk_x,
            chunk_y,
            dir,
            skin,
            clan_id_u16,
            chat_tag,
            prog_running,
            hand_mode_active,
        )
    };

    let (chat_name, chat_history) = {
        let channels = state.chat_channels.read();
        channels
            .iter()
            .find(|c| c.tag == chat_tag)
            .map(|c| {
                (
                    c.name.clone(),
                    c.messages.iter().cloned().collect::<Vec<_>>(),
                )
            })
            .unwrap_or_default()
    };

    Some(crate::game::PlayerInitView {
        player: Box::new(player_row.clone()),
        geo_label,
        max_health,
        skills,
        inventory,
        chunk_x,
        chunk_y,
        dir,
        skin,
        clan_id_u16,
        chat_tag,
        chat_name,
        chat_history,
        prog_running,
        hand_mode_active,
        chunk_packets: Vec::new(),
    })
}

#[cfg(test)]
pub fn connect_in_tick(state: &Arc<GameState>, tx: &Outbox, player: &PlayerRow, session_id: u64) {
    let session_id = crate::game::SessionId::new(session_id);
    state.sessions.register_test_outbox(session_id, tx.clone());
    state.sessions.bind_player(session_id, player.id.into());
    let effects = connect_entity_in_tick(state, player, session_id);
    assert!(effects.saves.is_empty());
    for event in effects.events {
        match event {
            crate::game::GameEvent::PlayerInit { session_id, view } => {
                deliver_player_init(state, session_id, &view);
            }
            crate::game::GameEvent::SessionBatch {
                session_id,
                player_id,
                packets,
            } => deliver_initial_presentation(state, session_id, player_id, packets),
            crate::game::GameEvent::Fanout { recipients, data } => {
                state.sessions.fanout(&recipients, &data);
            }
            crate::game::GameEvent::GuiView { .. } | crate::game::GameEvent::ChatFanout { .. } => {
                unreachable!("connect flow cannot produce GUI view or chat events")
            }
        }
    }
}

pub fn prepare_initial_presentation(
    state: &Arc<GameState>,
    view: &crate::game::PlayerInitView,
    session_id: crate::game::SessionId,
) -> crate::game::CommandEffects {
    let pid: PlayerId = view.player.id.into();
    if state
        .active_player_entity_for_session(pid, session_id)
        .is_none()
    {
        tracing::debug!(
            player_id = %pid,
            session_id = session_id.get(),
            "Skipping stale initial presentation after reconnect/disconnect"
        );
        return crate::game::CommandEffects::default();
    }
    let packets = crate::net::session::wire::PacketBatch::default();
    let threshold = Duration::from_millis(state.config.gameplay.schedules.game_loop_tick_rate_ms);
    let (profile, nearby) = build_initial_presentation(state, &packets, view);
    if profile.total > threshold {
        tracing::warn!(
            target: "tickprof",
            player_id = %pid,
            total = ?profile.total,
            threshold = ?threshold,
            chunk_sync = ?profile.chunk_sync,
            ecs_packets = ?profile.ecs_packets,
            spawn_broadcast_query = ?profile.spawn_broadcast_query,
            spawn_broadcast_send = ?profile.spawn_broadcast_send,
            chat = ?profile.chat,
            programmator = ?profile.programmator,
            bonus = ?profile.bonus,
            "SLOW player initial presentation build in game tick"
        );
    }
    let mut effects = crate::game::CommandEffects::default();
    effects.events.push(crate::game::GameEvent::SessionBatch {
        session_id,
        player_id: pid,
        packets: packets.into_packets(),
    });
    if let Some(nearby) = nearby {
        effects.events.push(nearby);
    }
    effects
}

/// Presentation owner: builds and emits Player.Init after authoritative connect
/// apply has made this session current.
pub fn deliver_player_init(
    state: &Arc<GameState>,
    session_id: crate::game::SessionId,
    view: &crate::game::PlayerInitView,
) {
    let effects = prepare_initial_presentation(state, view, session_id);
    for event in effects.events {
        match event {
            crate::game::GameEvent::SessionBatch {
                session_id,
                player_id,
                packets,
            } => deliver_initial_presentation(state, session_id, player_id, packets),
            crate::game::GameEvent::Fanout { recipients, data } => {
                state.sessions.fanout(&recipients, &data);
            }
            crate::game::GameEvent::PlayerInit { .. }
            | crate::game::GameEvent::GuiView { .. }
            | crate::game::GameEvent::ChatFanout { .. } => {
                unreachable!("Player.Init builder only produces packet/fanout effects")
            }
        }
    }
}

pub fn deliver_initial_presentation(
    state: &Arc<GameState>,
    session_id: crate::game::SessionId,
    player_id: PlayerId,
    packets: Vec<Vec<u8>>,
) {
    if state
        .active_player_entity_for_session(player_id, session_id)
        .is_none()
    {
        return;
    }
    let Some(tx) = state.sessions.outbox_for_session(session_id) else {
        return;
    };
    for packet in packets {
        if tx.send(packet).is_err() {
            break;
        }
    }
}

/// game-tick: despawn entity + сохранение в БД. Token-guard от reconnect-гонки:
/// сносим только если `active_players[pid]` всё ещё этот сеанс.
pub fn disconnect_in_tick(
    state: &Arc<GameState>,
    pid: PlayerId,
    session_id: impl Into<crate::game::SessionId>,
) -> crate::game::CommandEffects {
    let session_id = session_id.into();
    // Guard: если токен в active_players не совпадает — игрок уже переподключился
    // (новый сеанс владеет entity), этот Disconnect устарел → ничего не делаем.
    let Some(p) = state.active_player_entity_for_session(pid, session_id) else {
        return crate::game::CommandEffects::default();
    };
    state.remove_active_player(pid);
    let entity = p;

    // Берём чанк и row из ECS (sync), затем save_player отдаём в отдельный
    // таск — БД НЕ должна блокировать 10ms tick-цикл.
    let (cx, cy, row, keep_offline) = {
        let ecs = state.ecs.read();
        let chunk = ecs
            .get::<PlayerPosition>(entity)
            .map(|pos| (pos.chunk_x(), pos.chunk_y()))
            .unwrap_or((0, 0));
        let row = crate::game::player::extract_player_row(&ecs, entity);
        let keep_offline = ecs
            .get::<ProgrammatorState>(entity)
            .is_some_and(|prog| prog.running);
        (chunk.0, chunk.1, row, keep_offline)
    };
    let mut effects = crate::game::CommandEffects::default();
    if let Some(row) = row {
        effects
            .saves
            .push(crate::game::SaveCommand::Player { row: Box::new(row) });
    }

    let sub = crate::protocol::packets::hb_bot_del(net_u16_nonneg(pid));
    let hb_data = encode_hb_bundle(&hb_bundle(&[sub]).1);
    effects
        .events
        .push(fanout_event(state, cx, cy, hb_data, None));
    state.unregister_player_from_chunk(pid, cx, cy);

    if keep_offline {
        state
            .ecs_write_profiled("player.disconnect.keep_programmator")
            .entity_mut(entity)
            .remove::<PlayerConnection>();
        tracing::info!(
            player_id = %pid,
            "Player disconnected; running programmator entity kept offline"
        );
    } else {
        state
            .ecs_write_profiled("player.disconnect.despawn")
            .despawn(entity);
        state.unregister_player_entity(pid);
        tracing::info!(
            player_id = %pid,
            "Player disconnected and ECS entity despawned"
        );
    }
    effects
}

/// Порядок 1:1 с референсом `Player.Init()` (`Player.cs:597-652`).
/// Полностью без синхронного чтения ECS и глобального лок стейта.
#[allow(clippy::similar_names)]
fn build_initial_presentation(
    state: &Arc<GameState>,
    tx: &dyn PacketSink,
    view: &crate::game::PlayerInitView,
) -> (InitSyncProfile, Option<crate::game::GameEvent>) {
    let started_at = Instant::now();
    let mut profile = InitSyncProfile::default();
    let pid: PlayerId = view.player.id.into();

    let section_t0 = Instant::now();
    // BUG 2: C# ref calls MoveToChunk(ChunkX, ChunkY) BEFORE sync packets (BD, GE, @L, BI, etc.).
    for packet in &view.chunk_packets {
        // Send chunk packets calculated synchronously inside authoritative connect tick
        let _ = tx.send_packet(packet.clone());
    }
    profile.chunk_sync = section_t0.elapsed();

    let section_t0 = Instant::now();
    // 1. SendAutoDigg
    send_u_packet(tx, "BD", &auto_digg(view.player.auto_dig).1);
    send_u_packet(tx, "BA", &aggression(view.player.aggression).1);
    // 2. SendGeo
    send_u_packet(tx, "GE", &geo(&view.geo_label).1);
    // 3. SendHealth
    send_u_packet(
        tx,
        "@L",
        &crate::protocol::packets::health(view.player.health, view.max_health).1,
    );
    // 4. SendBotInfo
    let bi = bot_info(&view.player.name, view.player.x, view.player.y, pid.into());
    send_u_packet(tx, bi.0, &bi.1);
    // 5. SendSpeed
    send_player_speed(tx, &view.skills);
    // 6. SendCrys
    let dummy_stats = PlayerStats {
        health: 0,
        max_health: 0,
        money: 0,
        creds: 0,
        crystals: view.player.crystals,
        role: 0,
        skin: 0,
        clan_id: None,
        clan_rank: 0,
        last_bonus_at: 0,
    };
    send_player_basket(tx, &dummy_stats, &view.skills);
    // 7. SendMoney
    send_u_packet(tx, "P$", &money(view.player.money, view.player.creds).1);
    // 8. SendLvl
    send_player_level(tx, &view.skills);
    // 8a. SendSkills (@S)
    send_player_skills(tx, &view.skills);
    // 9. SendInventory
    let mut inv = view.inventory.clone();
    send_inventory(tx, &mut inv);
    profile.ecs_packets = section_t0.elapsed();

    let section_t0 = Instant::now();
    // 11. tp(x, y)
    tracing::info!(
        player_id = %pid,
        x = view.player.x,
        y = view.player.y,
        "Teleporting player to saved position at init"
    );
    send_u_packet(tx, "@T", &tp(view.player.x, view.player.y).1);
    // 12 консоль — пропускаем
    // 13. SendSettings (#S)
    let stg = settings_default_wire();
    send_u_packet(tx, stg.0, &stg.1);
    // 14. SendClan
    if let Some(cid) = view.player.clan_id {
        if cid != 0 {
            send_u_packet(tx, "cS", &clan_show(cid).1);
        } else {
            send_u_packet(tx, "cH", &clan_hide().1);
        }
    } else {
        send_u_packet(tx, "cH", &clan_hide().1);
    }
    profile.spawn_broadcast_query = section_t0.elapsed();

    // BUG 4: Broadcast @T appearance to nearby players so they see the newly logged-in player.
    let section_t0 = Instant::now();
    let sub = hb_bot(
        net_u16_nonneg(pid),
        net_u16_nonneg(view.player.x),
        net_u16_nonneg(view.player.y),
        view.dir,
        view.skin,
        view.clan_id_u16,
        0,
    );
    let hb_data = encode_hb_bundle(&hb_bundle(&[sub]).1);
    let nearby = Some(fanout_event(
        state,
        view.chunk_x,
        view.chunk_y,
        hb_data,
        Some(pid),
    ));
    profile.spawn_broadcast_send = section_t0.elapsed();

    // 15. SendChat (login)
    let section_t0 = Instant::now();
    send_u_packet(tx, "mO", &chat_current(&view.chat_tag, &view.chat_name).1);
    send_u_packet(
        tx,
        "mU",
        &chat_messages(&view.chat_tag, &view.chat_history).1,
    );
    profile.chat = section_t0.elapsed();

    // 16. ConfigPacket
    let section_t0 = Instant::now();
    send_u_packet(tx, "#F", &config_packet("oldprogramformat+").1);
    send_u_packet(tx, "@P", &programmator_status(view.prog_running).1);
    send_u_packet(tx, "BH", &hand_mode(view.hand_mode_active).1);
    if let Some(program) = &view.player.selected_program {
        send_u_packet(
            tx,
            "#p",
            &crate::protocol::packets::open_programmator(program.id, &program.name, &program.code)
                .1,
        );
    }
    profile.programmator = section_t0.elapsed();

    // 17. DR
    let section_t0 = Instant::now();
    let dr = if crate::game::logic::bonus::bonus_available(
        view.player.last_bonus_at,
        state.config.gameplay.bonus.cooldown_secs,
    ) {
        "1"
    } else {
        "0"
    };
    send_u_packet(tx, "DR", dr.as_bytes());
    profile.bonus = section_t0.elapsed();
    profile.total = started_at.elapsed();
    (profile, nearby)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::PlayerRow;
    use crate::db::ProgramRow;
    use crate::test_support::{ServerTestHarness, drain_events};
    use crate::world::cells::cell_type;

    fn base_player() -> PlayerRow {
        PlayerRow {
            id: 1,
            name: "TestPlayer".to_string(),
            passwd: "123".to_string(),
            hash: "hash".to_string(),
            x: 5,
            y: 5,
            dir: 0,
            health: 100,
            max_health: 100,
            money: 0,
            creds: 0,
            skin: 0,
            auto_dig: false,
            aggression: false,
            crystals: [0; 6],
            clan_id: None,
            resp_x: None,
            resp_y: None,
            inventory: std::collections::HashMap::new(),
            skills: crate::db::SkillSlots {
                skills: std::collections::HashMap::new(),
                total_slots: 20,
            },
            role: 0,
            selected_program_id: None,
            selected_program: None,
            programmator_running: false,
            programmator_snapshot: None,
            clan_rank: 0,
            last_bonus_at: 0,
        }
    }

    async fn make_init_test_state(name: &str) -> ServerTestHarness {
        ServerTestHarness::new(name, "init-test-player").await
    }

    fn event_index(events: &[(String, Vec<u8>)], name: &str) -> usize {
        events
            .iter()
            .position(|(event, _)| event == name)
            .unwrap_or_else(|| panic!("missing event {name}; events: {events:?}"))
    }

    #[tokio::test]
    async fn connect_command_returns_initial_sync_without_sending_during_dispatch() {
        let test = make_init_test_state("connect_command_effect").await;
        let state = &test.state;
        let (tx, mut rx) = crate::net::session::outbox::channel();
        let session_id = crate::game::SessionId::new(123);
        let player = base_player();
        state.sessions.register_test_outbox(session_id, tx);
        assert!(state.sessions.bind_player(session_id, player.id.into()));

        let effects = crate::game::logic::commands::apply_player_command(
            state,
            player.id.into(),
            session_id,
            crate::game::PlayerCommand::Connect {
                row: Box::new(player.clone()),
            },
        );

        assert!(effects.saves.is_empty());
        assert_eq!(effects.events.len(), 1);
        assert_eq!(
            effects
                .events
                .iter()
                .filter(|event| matches!(event, crate::game::GameEvent::PlayerInit { .. }))
                .count(),
            1
        );
        assert!(effects.events.iter().any(|event| matches!(
            event,
            crate::game::GameEvent::PlayerInit {
                session_id: actual_session_id,
                view,
            } if *actual_session_id == session_id
                && view.player.id == player.id
        )));
        assert!(
            rx.try_recv().is_err(),
            "command dispatch must not deliver initial presentation"
        );

        let (presentation_session, row) = effects
            .events
            .into_iter()
            .find_map(|event| match event {
                crate::game::GameEvent::PlayerInit { session_id, view } => Some((session_id, view)),
                crate::game::GameEvent::SessionBatch { .. }
                | crate::game::GameEvent::Fanout { .. }
                | crate::game::GameEvent::GuiView { .. }
                | crate::game::GameEvent::ChatFanout { .. } => None,
            })
            .expect("initial presentation event");
        let _ = disconnect_in_tick(state, PlayerId(player.id), session_id);
        deliver_player_init(state, presentation_session, &row);
        assert!(
            rx.try_recv().is_err(),
            "stale initial presentation must not be delivered after disconnect"
        );
    }

    #[tokio::test]
    async fn stale_disconnect_cannot_remove_or_save_reconnected_incarnation() {
        let test = make_init_test_state("stale_disconnect_after_reconnect").await;
        let state = &test.state;
        let player = base_player();
        let pid = PlayerId(player.id);
        let (old_tx, _old_rx) = crate::net::session::outbox::channel();
        let (new_tx, _new_rx) = crate::net::session::outbox::channel();

        connect_in_tick(state, &old_tx, &player, 123);
        let old_entity = state.get_player_entity(pid).expect("old active entity");
        connect_in_tick(state, &new_tx, &player, 456);
        let new_entity = state.get_player_entity(pid).expect("new active entity");

        assert_eq!(old_entity, new_entity);
        assert_eq!(
            state.active_session_for_player(pid),
            Some(crate::game::SessionId::new(456))
        );

        let stale_effects = disconnect_in_tick(state, pid, 123);

        assert!(stale_effects.saves.is_empty());
        assert!(stale_effects.events.is_empty());
        assert_eq!(state.get_player_entity(pid), Some(new_entity));
        assert_eq!(
            state.active_session_for_player(pid),
            Some(crate::game::SessionId::new(456))
        );
        let session_id = state
            .ecs
            .read()
            .get::<PlayerConnection>(new_entity)
            .map(|connection| connection.session_id);
        assert_eq!(session_id, Some(crate::game::SessionId::new(456)));
    }

    #[tokio::test]
    async fn init_running_selected_program_hydrates_after_status() {
        let test = make_init_test_state("init_running_selected_program").await;
        let state = &test.state;
        let (tx, mut rx) = crate::net::session::outbox::channel();
        let mut player = base_player();
        let program = ProgramRow {
            id: 7,
            player_id: player.id,
            name: "main".to_string(),
            code: "$z".to_string(),
        };
        player.selected_program_id = Some(program.id);
        player.selected_program = Some(program);
        player.programmator_running = true;

        connect_in_tick(state, &tx, &player, 123);

        let events = drain_events(&mut rx);
        assert!(
            events.iter().all(|(event, _)| event != "#P"),
            "login hydration must not open the editor with #P; events: {events:?}"
        );
        let config_idx = event_index(&events, "#F");
        let status_idx = event_index(&events, "@P");
        let hand_idx = event_index(&events, "BH");
        let update_idx = event_index(&events, "#p");
        assert!(
            config_idx < status_idx && status_idx < hand_idx && hand_idx < update_idx,
            "init programmator wire must be #F -> @P -> BH -> #p; events: {events:?}"
        );
        assert_eq!(events[status_idx].1, b"1");
        assert_eq!(events[hand_idx].1, b"0");
        let update_json: serde_json::Value = serde_json::from_slice(&events[update_idx].1).unwrap();
        assert_eq!(update_json["id"], 7);
        assert_eq!(update_json["title"], "main");
        assert_eq!(update_json["source"], "$z");
    }

    #[tokio::test]
    async fn test_spawn_clears_military_block() {
        let test = make_init_test_state("test_spawn_clear").await;
        let state = &test.state;
        // временная система
        let (tx, _rx) = crate::net::session::outbox::channel();
        let player = PlayerRow {
            id: 1,
            name: "TestPlayer".to_string(),
            passwd: "123".to_string(),
            hash: "hash".to_string(),
            x: 5,
            y: 5,
            dir: 0,
            health: 100,
            max_health: 100,
            money: 0,
            creds: 0,
            skin: 0,
            auto_dig: false,
            aggression: false,
            crystals: [0; 6],
            clan_id: None,
            resp_x: None,
            resp_y: None,
            inventory: std::collections::HashMap::new(),
            skills: crate::db::SkillSlots {
                skills: std::collections::HashMap::new(),
                total_slots: 20,
            },
            role: 0,
            selected_program_id: None,
            selected_program: None,
            programmator_running: false,
            programmator_snapshot: None,
            clan_rank: 0,
            last_bonus_at: 0,
        };

        // Scenario 1: MILITARY_BLOCK
        state.world.set_cell(5, 5, cell_type::MILITARY_BLOCK);
        assert_eq!(state.world.get_cell(5, 5), cell_type::MILITARY_BLOCK);

        connect_in_tick(state, &tx, &player, 123);

        // Verify it was cleared
        assert_eq!(state.world.get_cell(5, 5), cell_type::EMPTY);

        // Scenario 2: MILITARY_BLOCK_FRAME
        state.world.set_cell(5, 5, cell_type::MILITARY_BLOCK_FRAME);
        assert_eq!(state.world.get_cell(5, 5), cell_type::MILITARY_BLOCK_FRAME);

        // We clean up from active_players first to allow reconnecting
        if let Some(active) = state.remove_active_player(1.into()) {
            state.ecs.write().despawn(active.ecs_entity);
        }
        state.unregister_player_entity(1.into());
        connect_in_tick(state, &tx, &player, 124);

        assert_eq!(state.world.get_cell(5, 5), cell_type::EMPTY);

        // Cleanup
    }

    #[tokio::test]
    async fn disconnect_keeps_running_programmator_entity_offline() {
        let test = make_init_test_state("test_prog_disconnect").await;
        let state = &test.state;
        let (tx, _rx) = crate::net::session::outbox::channel();
        let player = PlayerRow {
            id: 1,
            name: "TestPlayer".to_string(),
            passwd: "123".to_string(),
            hash: "hash".to_string(),
            x: 5,
            y: 5,
            dir: 0,
            health: 100,
            max_health: 100,
            money: 0,
            creds: 0,
            skin: 0,
            auto_dig: false,
            aggression: false,
            crystals: [0; 6],
            clan_id: None,
            resp_x: None,
            resp_y: None,
            inventory: std::collections::HashMap::new(),
            skills: crate::db::SkillSlots {
                skills: std::collections::HashMap::new(),
                total_slots: 20,
            },
            role: 0,
            selected_program_id: None,
            selected_program: None,
            programmator_running: false,
            programmator_snapshot: None,
            clan_rank: 0,
            last_bonus_at: 0,
        };

        connect_in_tick(state, &tx, &player, 123);
        let pid = PlayerId(1);
        let entity = state.get_player_entity(pid).unwrap();
        state.modify_player(pid, |ecs, entity| {
            let mut prog = ecs.get_mut::<ProgrammatorState>(entity)?;
            prog.running = true;
            Some(())
        });

        let effects = disconnect_in_tick(state, pid, 123);

        assert_eq!(effects.saves.len(), 1);
        assert!(matches!(
            &effects.saves[0],
            crate::game::SaveCommand::Player { row } if row.id == player.id
        ));
        assert_eq!(effects.events.len(), 1);
        assert!(matches!(
            &effects.events[0],
            crate::game::GameEvent::Fanout { recipients, data }
                if recipients.is_empty() && !data.is_empty()
        ));

        let stale_effects = disconnect_in_tick(state, pid, 123);
        assert!(stale_effects.saves.is_empty());
        assert!(stale_effects.events.is_empty());

        assert!(!state.is_player_active(pid));
        assert_eq!(state.get_player_entity(pid), Some(entity));
        let still_running = {
            let ecs = state.ecs.read();
            ecs.get::<ProgrammatorState>(entity)
                .is_some_and(|prog| prog.running)
        };
        assert!(still_running);
        let has_connection_after_disconnect = {
            let ecs = state.ecs.read();
            ecs.get::<PlayerConnection>(entity).is_some()
        };
        assert!(
            !has_connection_after_disconnect,
            "offline programmator entity must not keep a closed connection channel"
        );

        let (tx2, _rx2) = crate::net::session::outbox::channel();
        connect_in_tick(state, &tx2, &player, 456);
        assert!(state.is_player_active(pid));
        assert_eq!(state.get_player_entity(pid), Some(entity));
        let has_connection_after_reconnect = {
            let ecs = state.ecs.read();
            ecs.get::<PlayerConnection>(entity).is_some()
        };
        assert!(
            has_connection_after_reconnect,
            "reconnect must restore PlayerConnection on the offline entity"
        );
    }

    #[tokio::test]
    async fn disconnect_in_tick_can_run_from_plain_game_thread_without_tokio_reactor() {
        let test = make_init_test_state("disconnect_plain_thread").await;
        let state = test.state.clone();
        let (tx, _rx) = crate::net::session::outbox::channel();
        let player = PlayerRow {
            id: 1,
            name: "PlainThreadPlayer".to_string(),
            passwd: "123".to_string(),
            hash: "hash".to_string(),
            x: 5,
            y: 5,
            dir: 0,
            health: 100,
            max_health: 100,
            money: 0,
            creds: 0,
            skin: 0,
            auto_dig: false,
            aggression: false,
            crystals: [0; 6],
            clan_id: None,
            resp_x: None,
            resp_y: None,
            inventory: std::collections::HashMap::new(),
            skills: crate::db::SkillSlots {
                skills: std::collections::HashMap::new(),
                total_slots: 20,
            },
            role: 0,
            selected_program_id: None,
            selected_program: None,
            programmator_running: false,
            programmator_snapshot: None,
            clan_rank: 0,
            last_bonus_at: 0,
        };
        connect_in_tick(&state, &tx, &player, 123);

        let state_for_thread = state.clone();
        let effects =
            std::thread::spawn(move || disconnect_in_tick(&state_for_thread, PlayerId(1), 123))
                .join()
                .expect("disconnect_in_tick must not panic outside Tokio reactor");

        assert_eq!(effects.saves.len(), 1);
        assert_eq!(effects.events.len(), 1);
        assert_eq!(
            state
                .db_pending_tasks
                .load(std::sync::atomic::Ordering::SeqCst),
            0,
            "disconnect command application must not start DB work"
        );
    }

    #[tokio::test]
    async fn test_respawn_clears_military_block() {
        let test = make_init_test_state("test_respawn_clear").await;
        let state = &test.state;

        let (tx, _rx) = crate::net::session::outbox::channel();
        let player = PlayerRow {
            id: 1,
            name: "TestPlayer".to_string(),
            passwd: "123".to_string(),
            hash: "hash".to_string(),
            x: 5,
            y: 5,
            dir: 0,
            health: 100,
            max_health: 100,
            money: 0,
            creds: 0,
            skin: 0,
            auto_dig: false,
            aggression: false,
            crystals: [0; 6],
            clan_id: None,
            resp_x: Some(10), // Respawn position
            resp_y: Some(10),
            inventory: std::collections::HashMap::new(),
            skills: crate::db::SkillSlots {
                skills: std::collections::HashMap::new(),
                total_slots: 20,
            },
            role: 0,
            selected_program_id: None,
            selected_program: None,
            programmator_running: false,
            programmator_snapshot: None,
            clan_rank: 0,
            last_bonus_at: 0,
        }; // временная система

        connect_in_tick(state, &tx, &player, 123);

        // Place military block at (10, 10) and all possible random offsets from (10, 10)
        state.world.set_cell(10, 10, cell_type::MILITARY_BLOCK);
        for ox in 2..5 {
            for oy in -1..3 {
                state
                    .world
                    .set_cell(10 + ox, 10 + oy, cell_type::MILITARY_BLOCK);
            }
        }

        let building_entities = state.building_entities_snapshot();
        let mut ecs = state.ecs.write();
        let result = crate::net::session::play::death::apply_player_death_core(
            state,
            &mut ecs,
            &building_entities,
            crate::game::player::PlayerId(1),
        );
        assert!(result.is_ok());
        let output = result.unwrap();

        // Verify the block was cleared and the cleared position is tracked in DeathBroadcasts
        assert_eq!(
            state.world.get_cell(output.resp_x, output.resp_y),
            cell_type::EMPTY
        );
        assert_eq!(
            output.broadcasts.cleared_spawn_cell,
            Some((output.resp_x, output.resp_y))
        );

        // Cleanup
    }
}

// временная система
