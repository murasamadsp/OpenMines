use crate::db::players::PlayerRow;
use crate::game::LifeCmd;
use crate::game::player::{
    PlayerConnection, PlayerCooldowns, PlayerFlags, PlayerGeoStack, PlayerId, PlayerInventory,
    PlayerMetadata, PlayerPosition, PlayerSettings, PlayerSkillsComp, PlayerStats, PlayerUI,
    PlayerView,
};
use crate::game::programmator::ProgrammatorState;
use crate::game::skills::{OnHealth, PlayerSkills};
use crate::net::session::outbound::inventory_sync::send_inventory;
use crate::net::session::outbound::player_sync::{
    send_player_basket, send_player_health, send_player_level, send_player_skills,
    send_player_speed,
};
use crate::net::session::play::chunks::check_chunk_changed;
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
pub fn init_player(
    state: &Arc<GameState>,
    player: &PlayerRow,
    session_id: crate::game::SessionId,
) -> PlayerId {
    let pid: PlayerId = player.id.into();
    if !state.sessions.bind_player(session_id, pid) {
        tracing::debug!(player_id = %pid, session_id = session_id.get(), "Skipping player init for closed session");
        return pid;
    }
    state.enqueue_life(LifeCmd::Connect {
        row: Box::new(player.clone()),
        session_id,
    });
    pid
}

/// Conn-таск: ставит выход игрока в lifecycle-очередь (см. `init_player`).
pub fn on_disconnect(state: &Arc<GameState>, pid: PlayerId, session_id: crate::game::SessionId) {
    state.remove_rate_limiter(pid);
    state.enqueue_life(LifeCmd::Disconnect { pid, session_id });
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

    // BUG 1: Reconnect entity leak — clean up any existing session for this pid before spawning a new one.
    let section_t0 = Instant::now();
    if let Some(old_player) = state.remove_active_player(pid) {
        let old_entity = old_player.ecs_entity;
        let (old_cx, old_cy) = {
            let ecs = state.ecs.read();
            ecs.get::<PlayerPosition>(old_entity)
                .map(|pos| (pos.chunk_x(), pos.chunk_y()))
                .unwrap_or((0, 0))
        };
        // Broadcast removal to nearby players.
        let sub = crate::protocol::packets::hb_bot_del(net_u16_nonneg(pid));
        let hb_data = encode_hb_bundle(&hb_bundle(&[sub]).1);
        effects
            .events
            .push(fanout_event(state, old_cx, old_cy, hb_data, None));
        // Remove from chunk player index — iterate all entries to handle stale registrations.
        state.unregister_player_from_all_chunks(pid);
        // Despawn old ECS entity.
        state
            .ecs_write_profiled("player.connect.cleanup_old_entity")
            .despawn(old_entity);
        state.unregister_player_entity(pid);
        tracing::warn!(
            player_id = %pid,
            "Player reconnected — old ECS entity cleaned up"
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
                crate::game::player::extract_player_row(&ecs, entity)
            }
        };
        if let Some(mut row) = sync_row.take() {
            row.selected_program_id = player.selected_program_id;
            row.selected_program = player.selected_program.clone();
            state.register_active_player(pid, entity, session_id);
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
        PlayerFlags { dirty: false },
    );
    profile.spawn_prepare = section_t0.elapsed();

    let section_t0 = Instant::now();
    let entity = state.ecs.write().spawn(components).id();
    profile.spawn_ecs = section_t0.elapsed();
    profile.spawn_entity = profile.spawn_prepare + profile.spawn_ecs;

    let section_t0 = Instant::now();
    state.register_active_player(pid, entity, session_id);
    state.register_player_entity(pid, entity);
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
    connect_entity_in_tick_inner(state, player, session_id)
}

#[cfg(test)]
pub fn connect_in_tick(state: &Arc<GameState>, tx: &Outbox, player: &PlayerRow, session_id: u64) {
    let session_id = crate::game::SessionId::new(session_id);
    state.sessions.register_test_outbox(session_id, tx.clone());
    state.sessions.bind_player(session_id, player.id.into());
    let mut effects = connect_entity_in_tick_inner(state, player, session_id);
    effects.append(prepare_initial_presentation(state, player, session_id));
    assert!(effects.saves.is_empty());
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
        }
    }
}

pub fn prepare_initial_presentation(
    state: &Arc<GameState>,
    player: &PlayerRow,
    session_id: crate::game::SessionId,
) -> crate::game::CommandEffects {
    let pid: PlayerId = player.id.into();
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
    let (profile, nearby) = build_initial_presentation(state, &packets, player);
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
/// Полностью синхронна: на логине нет async-DB (`current_chat`=="FED" резолвится
/// из in-memory `chat_channels`; блок программы мёртв — `selected_id`=None).
#[allow(clippy::similar_names)]
fn build_initial_presentation(
    state: &Arc<GameState>,
    tx: &dyn PacketSink,
    player: &PlayerRow,
) -> (InitSyncProfile, Option<crate::game::GameEvent>) {
    let started_at = Instant::now();
    let mut profile = InitSyncProfile::default();
    let pid: PlayerId = player.id.into();
    // BUG 2: C# ref calls MoveToChunk(ChunkX, ChunkY) BEFORE sync packets (BD, GE, @L, BI, etc.).
    let section_t0 = Instant::now();
    check_chunk_changed(state, tx, pid);
    profile.chunk_sync = section_t0.elapsed();
    let section_t0 = Instant::now();
    state.modify_player(pid, |ecs, entity| {
        let stats = ecs.get::<PlayerStats>(entity)?;
        let skills = ecs.get::<PlayerSkillsComp>(entity)?;

        // 1. SendAutoDigg
        send_u_packet(tx, "BD", &auto_digg(player.auto_dig).1);
        send_u_packet(tx, "BA", &aggression(player.aggression).1);
        // 2. SendGeo (`pSenders.cs` — `World.GetProp(geo.Peek()).name` или "")
        let geo_label = ecs
            .get::<PlayerGeoStack>(entity)
            .and_then(|gs| gs.0.last().copied())
            .map(|cell| state.world.cell_defs().get(cell).name.clone())
            .unwrap_or_default();
        send_u_packet(tx, "GE", &geo(&geo_label).1);
        // 3. SendHealth
        send_player_health(tx, stats);
        // 4. SendBotInfo
        let bi = bot_info(&player.name, player.x, player.y, pid.into());
        send_u_packet(tx, bi.0, &bi.1);
        // 5. SendSpeed
        send_player_speed(tx, skills);
        // 6. SendCrys
        send_player_basket(tx, stats, skills);
        // 7. SendMoney
        send_u_packet(tx, "P$", &money(player.money, player.creds).1);
        // 8. SendLvl
        send_player_level(tx, skills);
        // 8a. SendSkills (@S) — C# ref: `Player.Init()` sends @S immediately after LV.
        send_player_skills(tx, skills);
        // 9. SendInventory (`Inventory.InvToSend` — нужен `&mut` для `miniq` префилла)
        let mut inv = ecs.get_mut::<PlayerInventory>(entity)?;
        send_inventory(tx, &mut inv);
        Some(())
    });
    profile.ecs_packets = section_t0.elapsed();
    let section_t0 = Instant::now();
    let spawn_broadcast = state.query_player_opt(pid, |ecs, entity| {
        let Some(stats) = ecs.get::<PlayerStats>(entity) else {
            tracing::error!(
                player_id = %pid,
                "PlayerStats missing; skipping teleport and clan sync at init"
            );
            return None;
        };

        // 11. tp(x, y)
        tracing::info!(
            player_id = %pid,
            x = player.x,
            y = player.y,
            "Teleporting player to saved position at init"
        );
        send_u_packet(tx, "@T", &tp(player.x, player.y).1);
        // 12 консоль — пропускаем
        // 13. SendSettings (#S)
        let stg = settings_default_wire();
        send_u_packet(tx, stg.0, &stg.1);
        // 14. SendClan
        if let Some(cid) = stats.clan_id {
            if cid != 0 {
                send_u_packet(tx, "cS", &clan_show(cid).1);
            } else {
                send_u_packet(tx, "cH", &clan_hide().1);
            }
        } else {
            send_u_packet(tx, "cH", &clan_hide().1);
        }

        // BUG 4: Collect data needed to broadcast hb_bot to nearby players.
        let pos = ecs.get::<PlayerPosition>(entity)?;
        let clan_id_raw = stats.clan_id.unwrap_or(0).clamp(0, 65535) as u16;
        Some((
            pos.chunk_x(),
            pos.chunk_y(),
            pos.dir as u8,
            stats.skin as u8,
            clan_id_raw,
        ))
    });
    profile.spawn_broadcast_query = section_t0.elapsed();

    // BUG 4: Broadcast @T appearance to nearby players so they see the newly logged-in player.
    let section_t0 = Instant::now();
    let nearby = spawn_broadcast.map(|(cx, cy, dir, skin, clan_id_u16)| {
        let sub = hb_bot(
            net_u16_nonneg(pid),
            net_u16_nonneg(player.x),
            net_u16_nonneg(player.y),
            dir,
            skin,
            clan_id_u16,
            0,
        );
        let hb_data = encode_hb_bundle(&hb_bundle(&[sub]).1);
        fanout_event(state, cx, cy, hb_data, Some(pid))
    });
    profile.spawn_broadcast_send = section_t0.elapsed();
    // 15. SendChat (login): mO + bounded current-chat history. `Chin` may still
    // arrive later from the client, but client-side mU dedup prevents visual
    // duplicates while this makes first login independent from that timing.
    let section_t0 = Instant::now();
    let chat_mo = state
        .query_player_opt(pid, |ecs, e| {
            ecs.get::<PlayerUI>(e).map(|u| u.current_chat.clone())
        })
        .and_then(|tag| {
            let channels = state.chat_channels.read();
            channels.iter().find(|c| c.tag == tag).map(|c| {
                (
                    tag,
                    c.name.clone(),
                    c.messages.iter().cloned().collect::<Vec<_>>(),
                )
            })
        });
    if let Some((tag, name, history)) = chat_mo {
        send_u_packet(tx, "mO", &chat_current(&tag, &name).1);
        send_u_packet(tx, "mU", &chat_messages(&tag, &history).1);
    }
    profile.chat = section_t0.elapsed();
    // 16. ConfigPacket
    let section_t0 = Instant::now();
    send_u_packet(tx, "#F", &config_packet("oldprogramformat+").1);
    let (prog_running, hand_mode_active) = state
        .query_player(pid, |ecs, entity| {
            ecs.get::<ProgrammatorState>(entity)
                .map_or((false, false), |prog| (prog.running, prog.hand_mode_active))
        })
        .unwrap_or((false, false));
    send_u_packet(tx, "@P", &programmator_status(prog_running).1);
    send_u_packet(tx, "BH", &hand_mode(hand_mode_active).1);
    // Unity @P=1 activates the whole ProgrammatorWindow. If a selected program
    // exists, #p must follow @P/BH so UpdateProgramm hydrates programId/source
    // and hides the window again.
    if let Some(program) = &player.selected_program {
        send_u_packet(
            tx,
            "#p",
            &crate::protocol::packets::open_programmator(program.id, &program.name, &program.code)
                .1,
        );
    }
    profile.programmator = section_t0.elapsed();
    // 17. DR — индикатор ежедневного бонуса (мигание кнопки БОНУСЫ): "1" если
    // доступен (прошло ≥ кулдауна с последнего клейма), иначе "0".
    let section_t0 = Instant::now();
    let dr = if crate::game::logic::bonus::bonus_available(
        player.last_bonus_at,
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
    use crate::world::cells::cell_type;
    use bytes::BytesMut;
    use std::sync::Arc;
    use tokio::sync::mpsc::Receiver;

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

    async fn make_init_test_state(
        name: &str,
    ) -> (
        Arc<GameState>,
        std::path::PathBuf,
        std::path::PathBuf,
        String,
    ) {
        let dir = std::env::temp_dir();
        let unique = format!("{}_{}", name, std::process::id());
        let db_path = dir.join(format!("{unique}.db"));
        let _ = std::fs::remove_file(&db_path);
        let database = crate::db::Database::open(&db_path).await.unwrap();

        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();
        let world_name = format!("{unique}_world");
        let world = crate::world::World::new(&world_name, 2, 2, cell_defs, &dir).unwrap();
        let config = crate::config::Config {
            world_name: world_name.clone(),
            port: 8090,
            world_chunks_w: 2,
            world_chunks_h: 2,
            data_dir: dir.to_string_lossy().to_string(),
            logging: crate::config::LoggingConfig::runtime_baseline(),
            cron: crate::config::CronConfig::runtime_baseline(),
            gameplay: crate::config::GameplayConfig::runtime_baseline(),
        };

        let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config)
            .await
            .unwrap();
        (state, db_path, dir, world_name)
    }

    fn cleanup_init_test(db_path: &std::path::Path, dir: &std::path::Path, world_name: &str) {
        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_v2.map")));
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_road_v2.map")));
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_durability.map")));
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_world.journal")));
    }

    fn drain_events(rx: &mut Receiver<Vec<u8>>) -> Vec<(String, Vec<u8>)> {
        let mut events = Vec::new();
        while let Ok(frame) = rx.try_recv() {
            let mut buf = BytesMut::from(&frame[..]);
            let packet = crate::protocol::Packet::try_decode(&mut buf)
                .expect("valid packet")
                .expect("decoded packet");
            events.push((packet.event_str().to_owned(), packet.payload.to_vec()));
        }
        events
    }

    fn event_index(events: &[(String, Vec<u8>)], name: &str) -> usize {
        events
            .iter()
            .position(|(event, _)| event == name)
            .unwrap_or_else(|| panic!("missing event {name}; events: {events:?}"))
    }

    #[tokio::test]
    async fn connect_command_returns_initial_sync_without_sending_during_dispatch() {
        let (state, db_path, dir, world_name) =
            make_init_test_state("connect_command_effect").await;
        let (tx, mut rx) = crate::net::session::outbox::channel();
        let session_id = crate::game::SessionId::new(123);
        let player = base_player();
        state.sessions.register_test_outbox(session_id, tx);
        assert!(state.sessions.bind_player(session_id, player.id.into()));

        let effects = crate::game::logic::commands::apply_player_command(
            &state,
            crate::game::PlayerCommand::Connect {
                row: Box::new(player.clone()),
                session_id,
            },
        );

        assert!(effects.saves.is_empty());
        assert_eq!(effects.events.len(), 2);
        assert_eq!(
            effects
                .events
                .iter()
                .filter(|event| matches!(event, crate::game::GameEvent::SessionBatch { .. }))
                .count(),
            1
        );
        assert!(effects.events.iter().any(|event| matches!(
            event,
            crate::game::GameEvent::SessionBatch {
                session_id: actual_session_id,
                player_id,
                packets,
            } if *actual_session_id == session_id
                && *player_id == PlayerId(player.id)
                && !packets.is_empty()
        )));
        assert_eq!(
            effects
                .events
                .iter()
                .filter(|event| matches!(event, crate::game::GameEvent::Fanout { .. }))
                .count(),
            1
        );
        assert!(
            rx.try_recv().is_err(),
            "command dispatch must not deliver initial presentation"
        );

        let (presentation_session, presentation_player, packets) = effects
            .events
            .into_iter()
            .find_map(|event| match event {
                crate::game::GameEvent::SessionBatch {
                    session_id,
                    player_id,
                    packets,
                } => Some((session_id, player_id, packets)),
                crate::game::GameEvent::Fanout { .. } => None,
            })
            .expect("initial presentation event");
        let _ = disconnect_in_tick(&state, PlayerId(player.id), session_id);
        deliver_initial_presentation(&state, presentation_session, presentation_player, packets);
        assert!(
            rx.try_recv().is_err(),
            "stale initial presentation must not be delivered after disconnect"
        );

        cleanup_init_test(&db_path, &dir, &world_name);
    }

    #[tokio::test]
    async fn init_running_selected_program_hydrates_after_status() {
        let (state, db_path, dir, world_name) =
            make_init_test_state("init_running_selected_program").await;
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

        connect_in_tick(&state, &tx, &player, 123);

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

        cleanup_init_test(&db_path, &dir, &world_name);
    }

    #[tokio::test]
    async fn test_spawn_clears_military_block() {
        let dir = std::env::temp_dir();
        let db_path = dir.join(format!("test_spawn_clear_db_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let database = crate::db::Database::open(&db_path).await.unwrap();

        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();

        let world_name = format!("test_world_spawn_{}", std::process::id());
        let world = crate::world::World::new(&world_name, 2, 2, cell_defs, &dir).unwrap();

        let config = crate::config::Config {
            world_name: world_name.clone(),
            port: 8090,
            world_chunks_w: 2,
            world_chunks_h: 2,
            data_dir: dir.to_string_lossy().to_string(),
            logging: crate::config::LoggingConfig::runtime_baseline(),
            cron: crate::config::CronConfig::runtime_baseline(),
            gameplay: crate::config::GameplayConfig::runtime_baseline(),
        };

        let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config)
            .await
            .unwrap();
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

        connect_in_tick(&state, &tx, &player, 123);

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
        connect_in_tick(&state, &tx, &player, 124);

        assert_eq!(state.world.get_cell(5, 5), cell_type::EMPTY);

        // Cleanup
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_v2.map")));
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_durability.map")));
    }

    #[tokio::test]
    async fn disconnect_keeps_running_programmator_entity_offline() {
        let dir = std::env::temp_dir();
        let db_path = dir.join(format!("test_prog_disconnect_db_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let database = crate::db::Database::open(&db_path).await.unwrap();

        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();
        let world_name = format!("test_prog_disconnect_world_{}", std::process::id());
        let world = crate::world::World::new(&world_name, 2, 2, cell_defs, &dir).unwrap();
        let config = crate::config::Config {
            world_name: world_name.clone(),
            port: 8090,
            world_chunks_w: 2,
            world_chunks_h: 2,
            data_dir: dir.to_string_lossy().to_string(),
            logging: crate::config::LoggingConfig::runtime_baseline(),
            cron: crate::config::CronConfig::runtime_baseline(),
            gameplay: crate::config::GameplayConfig::runtime_baseline(),
        };
        let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config)
            .await
            .unwrap();
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

        connect_in_tick(&state, &tx, &player, 123);
        let pid = PlayerId(1);
        let entity = state.get_player_entity(pid).unwrap();
        state.modify_player(pid, |ecs, entity| {
            let mut prog = ecs.get_mut::<ProgrammatorState>(entity)?;
            prog.running = true;
            Some(())
        });

        let effects = disconnect_in_tick(&state, pid, 123);

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

        let stale_effects = disconnect_in_tick(&state, pid, 123);
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
        connect_in_tick(&state, &tx2, &player, 456);
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

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_v2.map")));
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_road_v2.map")));
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_durability.map")));
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_world.journal")));
    }

    #[tokio::test]
    async fn disconnect_in_tick_can_run_from_plain_game_thread_without_tokio_reactor() {
        let dir = std::env::temp_dir();
        let db_path = dir.join(format!("disconnect_plain_thread_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let database = crate::db::Database::open(&db_path).await.unwrap();

        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();
        let world_name = format!("disconnect_plain_thread_{}", std::process::id());
        let world = crate::world::World::new(&world_name, 2, 2, cell_defs, &dir).unwrap();
        let config = crate::config::Config {
            world_name: world_name.clone(),
            port: 8090,
            world_chunks_w: 2,
            world_chunks_h: 2,
            data_dir: dir.to_string_lossy().to_string(),
            logging: crate::config::LoggingConfig::runtime_baseline(),
            cron: crate::config::CronConfig::runtime_baseline(),
            gameplay: crate::config::GameplayConfig::runtime_baseline(),
        };
        let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config)
            .await
            .unwrap();
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

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_v2.map")));
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_road_v2.map")));
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_durability.map")));
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_world.journal")));
    }

    #[tokio::test]
    async fn test_respawn_clears_military_block() {
        let dir = std::env::temp_dir();
        let db_path = dir.join(format!("test_respawn_clear_db_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let database = crate::db::Database::open(&db_path).await.unwrap();

        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();

        let world_name = format!("test_world_respawn_{}", std::process::id());
        let world = crate::world::World::new(&world_name, 2, 2, cell_defs, &dir).unwrap();

        let config = crate::config::Config {
            world_name: world_name.clone(),
            port: 8090,
            world_chunks_w: 2,
            world_chunks_h: 2,
            data_dir: dir.to_string_lossy().to_string(),
            logging: crate::config::LoggingConfig::runtime_baseline(),
            cron: crate::config::CronConfig::runtime_baseline(),
            gameplay: crate::config::GameplayConfig::runtime_baseline(),
        };

        let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config)
            .await
            .unwrap();

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

        connect_in_tick(&state, &tx, &player, 123);

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
            &state,
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
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_v2.map")));
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_durability.map")));
    }
}

// временная система
