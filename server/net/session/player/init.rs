use crate::db::players::PlayerRow;
use crate::game::LifeCmd;
use crate::game::player::{
    ActivePlayer, PlayerConnection, PlayerCooldowns, PlayerFlags, PlayerGeoStack, PlayerId,
    PlayerInventory, PlayerMetadata, PlayerPosition, PlayerSettings, PlayerSkills, PlayerStats,
    PlayerUI, PlayerView,
};
use crate::game::programmator::ProgrammatorState;
use crate::net::session::outbound::inventory_sync::send_inventory;
use crate::net::session::outbound::player_sync::{
    send_player_basket, send_player_health, send_player_level, send_player_skills,
    send_player_speed,
};
use crate::net::session::play::chunks::check_chunk_changed;
use crate::net::session::prelude::*;

/// Conn-таск: ставит вход игрока в lifecycle-очередь. Сам ecs не трогает —
/// spawn entity + Init-пакеты выполняет game-tick (`connect_in_tick`), чтобы
/// `ecs`-`RwLock` не контендился между conn-тасками и тиком. cf/Gu (и AH при
/// регистрации) уже отправлены вызывающим до этой точки — порядок в tx сохранён.
pub fn init_player(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    player: &PlayerRow,
    token: u64,
) -> PlayerId {
    state.enqueue_life(LifeCmd::Connect {
        row: Box::new(player.clone()),
        tx: tx.clone(),
        token,
    });
    player.id
}

/// Conn-таск: ставит выход игрока в lifecycle-очередь (см. `init_player`).
pub fn on_disconnect(state: &Arc<GameState>, pid: PlayerId, token: u64) {
    state.enqueue_life(LifeCmd::Disconnect { pid, token });
}

/// game-tick: спавн entity + Init-пакеты (1:1 порядок с `Player.Init()`).
/// Выполняется в tick-таске (единственный писатель `ecs`).
#[allow(clippy::similar_names)]
pub fn connect_in_tick(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    player: &PlayerRow,
    token: u64,
) {
    let pid = player.id;

    // BUG 1: Reconnect entity leak — clean up any existing session for this pid before spawning a new one.
    if let Some((_, old_player)) = state.active_players.remove(&pid) {
        let old_entity = old_player.ecs_entity;
        let (old_cx, old_cy) = {
            let ecs = state.ecs.read();
            ecs.get::<PlayerPosition>(old_entity)
                .map(|pos| (pos.chunk_x(), pos.chunk_y()))
                .unwrap_or((0, 0))
        };
        // Remove from chunk_players — iterate all entries to handle stale registrations.
        state
            .chunk_players
            .iter_mut()
            .for_each(|mut e| e.value_mut().retain(|&id| id != pid));
        // Broadcast removal to nearby players.
        let sub = crate::protocol::packets::hb_bot_del(net_u16_nonneg(pid));
        let hb_data = encode_hb_bundle(&hb_bundle(&[sub]).1);
        state.broadcast_to_nearby(old_cx, old_cy, &hb_data, None);
        // Despawn old ECS entity.
        state.ecs.write().despawn(old_entity);
        tracing::warn!("Player {pid} reconnected — old ECS entity cleaned up");
    }

    let now = std::time::Instant::now();
    // 1:1-ish ref behavior: immediately allow first actions after login.
    // If we initialize cooldown timestamps to `now`, the first few client `Xmov` packets can be ignored,
    // causing the next accepted move to be "too far" and trigger a server correction (@T).
    // `checked_sub` может вернуть None в первую секунду аптайма машины (Instant у базы);
    // тогда `now` — безопасный фолбэк (худшее — одна @T-коррекция на первом ходе).
    let ready = now
        .checked_sub(std::time::Duration::from_secs(1))
        .unwrap_or(now);

    let entity = state
        .ecs
        .write()
        .spawn((
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
            PlayerConnection { tx: tx.clone() },
            PlayerStats {
                health: player.health,
                max_health: player.max_health,
                money: player.money,
                creds: player.creds,
                crystals: player.crystals,
                role: player.role,
                skin: player.skin,
                clan_id: player.clan_id,
                clan_rank: player.clan_rank,
                crystal_carry: 0.0,
            },
            PlayerInventory {
                items: player.inventory.clone(),
                selected: -1,
                minv: true,
                miniq: Vec::new(),
            },
            PlayerSkills {
                states: player.skills.clone(),
            },
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
                last_shot: None,
                c190_stacks: 1,
                last_c190_hit: None,
            },
            PlayerGeoStack::default(),
            ProgrammatorState::new(),
            PlayerSettings {
                auto_dig: player.auto_dig,
                ..PlayerSettings::default()
            },
            PlayerFlags { dirty: false },
        ))
        .id();

    state.active_players.insert(
        pid,
        ActivePlayer {
            ecs_entity: entity,
            session_token: token,
        },
    );

    state.player_sessions.insert(pid, tx.clone());

    // BUG 3: Recalculate max_health from Health skill at login (C# ref: MaxHealth = 100 + skill.Effect).
    state.modify_player(pid, |ecs, entity| {
        let max_health = {
            let skills = ecs.get::<PlayerSkills>(entity)?;
            let effect = get_player_skill_effect(&skills.states, SkillType::Health);
            #[allow(clippy::cast_possible_truncation)]
            {
                effect as i32
            }
        };
        let mut stats = ecs.get_mut::<PlayerStats>(entity)?;
        stats.max_health = max_health;
        if stats.health <= 0 {
            stats.health = stats.max_health;
        }
        Some(())
    });

    send_initial_sync(state, tx, player);
}

/// game-tick: despawn entity + сохранение в БД. Token-guard от reconnect-гонки:
/// сносим только если `active_players[pid]` всё ещё этот сеанс.
pub fn disconnect_in_tick(state: &Arc<GameState>, pid: PlayerId, token: u64) {
    // Guard: если токен в active_players не совпадает — игрок уже переподключился
    // (новый сеанс владеет entity), этот Disconnect устарел → ничего не делаем.
    let Some(p) = state
        .active_players
        .get(&pid)
        .filter(|p| p.session_token == token)
        .map(|p| p.ecs_entity)
    else {
        return;
    };
    state.active_players.remove(&pid);
    state.player_sessions.remove(&pid);
    let entity = p;

    // Берём чанк и row из ECS (sync), затем save_player отдаём в отдельный
    // таск — БД НЕ должна блокировать 10ms tick-цикл.
    let (cx, cy, row) = {
        let ecs = state.ecs.read();
        let chunk = ecs
            .get::<PlayerPosition>(entity)
            .map(|pos| (pos.chunk_x(), pos.chunk_y()))
            .unwrap_or((0, 0));
        let row = crate::game::player::extract_player_row(&ecs, entity);
        (chunk.0, chunk.1, row)
    };
    if let Some(row) = row {
        let db = state.db.clone();
        tokio::spawn(async move {
            if let Err(e) = db.save_player(&row).await {
                tracing::error!("Failed to save player {pid} on disconnect: {e}");
            }
        });
    }

    if let Some(mut e) = state.chunk_players.get_mut(&(cx, cy)) {
        e.retain(|&id| id != pid);
    }

    let sub = crate::protocol::packets::hb_bot_del(net_u16_nonneg(pid));
    let hb_data = encode_hb_bundle(&hb_bundle(&[sub]).1);
    state.broadcast_to_nearby(cx, cy, &hb_data, None);

    state.ecs.write().despawn(entity);
    tracing::info!("Player {pid} disconnected and ECS entity despawned");
}

/// Порядок 1:1 с референсом `Player.Init()` (`Player.cs:597-652`).
/// Полностью синхронна: на логине нет async-DB (`current_chat`=="FED" резолвится
/// из in-memory `chat_channels`; блок программы мёртв — `selected_id`=None).
#[allow(clippy::similar_names)]
fn send_initial_sync(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    player: &PlayerRow,
) {
    let pid = player.id;
    // BUG 2: C# ref calls MoveToChunk(ChunkX, ChunkY) BEFORE sync packets (BD, GE, @L, BI, etc.).
    check_chunk_changed(state, tx, pid);
    state.modify_player(pid, |ecs, entity| {
        let stats = ecs.get::<PlayerStats>(entity)?;
        let skills = ecs.get::<PlayerSkills>(entity)?;

        // 1. SendAutoDigg
        send_u_packet(tx, "BD", &auto_digg(player.auto_dig).1);
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
        let bi = bot_info(&player.name, player.x, player.y, pid);
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
    let spawn_broadcast = state.query_player_opt(pid, |ecs, entity| {
        let Some(stats) = ecs.get::<PlayerStats>(entity) else {
            tracing::error!("[Init] PlayerStats missing for pid={pid}; skip @T/clan tail");
            return None;
        };

        // 11. tp(x, y)
        tracing::info!(
            "[Init] @T pid={pid} to=({},{}) (db/player row position)",
            player.x,
            player.y
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

    // BUG 4: Broadcast @T appearance to nearby players so they see the newly logged-in player.
    if let Some((cx, cy, dir, skin, clan_id_u16)) = spawn_broadcast {
        let sub = hb_bot(
            net_u16_nonneg(pid),
            player.x.max(0) as u16,
            player.y.max(0) as u16,
            dir,
            skin,
            clan_id_u16,
            0,
        );
        let hb_data = encode_hb_bundle(&hb_bundle(&[sub]).1);
        state.broadcast_to_nearby(cx, cy, &hb_data, Some(pid));
    }
    // 15. SendChat (login) — как `Player.SendChat()`: только `mO`. На логине
    // `current_chat`=="FED", `chat_access` резолвит FED из in-memory
    // `chat_channels` (без БД) → инлайн-sync, wire-идентично.
    let chat_mo = state
        .query_player_opt(pid, |ecs, e| {
            ecs.get::<PlayerUI>(e).map(|u| u.current_chat.clone())
        })
        .and_then(|tag| {
            let channels = state.chat_channels.read();
            channels
                .iter()
                .find(|c| c.tag == tag)
                .map(|c| (tag, c.name.clone()))
        });
    if let Some((tag, name)) = chat_mo {
        send_u_packet(tx, "mO", &chat_current(&tag, &name).1);
    }
    // 16. ConfigPacket
    send_u_packet(tx, "#F", &config_packet("oldprogramformat+").1);
    // 17. UpdateProg (#p) — на свежем логине `ProgrammatorState::new()` ⇒
    // `selected_id`=None, из БД не гидрируется ⇒ if-ветка `#p` не берётся.
    // C# ref эквивалент: при `selected==null` шлётся только `@P false`.
    send_u_packet(tx, "@P", &programmator_status(false).1);
}
