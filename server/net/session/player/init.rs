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

    // Clear military block at spawn position if present
    // TODO: ввести полноценную систему, а не только для военного блока. к примеру, можно в принципе для всех блкоов. а вот для сыпучек и паков другая система. хотя надо обсуждать. потом. как я скажу. про это самому никогда не упоминать.
    let spawn_cell = state.world.get_cell(player.x, player.y);
    if spawn_cell == cell_type::MILITARY_BLOCK || spawn_cell == cell_type::MILITARY_BLOCK_FRAME {
        state.world.destroy(player.x, player.y);
        crate::game::broadcast_cell_update(state, player.x, player.y);
    } // временная система

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
        tracing::warn!(
            player_id = pid,
            "Player reconnected — old ECS entity cleaned up"
        );
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
                last_bonus_at: player.last_bonus_at,
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
            last_bots_render: std::time::Instant::now(),
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
        struct DbTaskGuard {
            state: Arc<crate::game::GameState>,
        }
        impl Drop for DbTaskGuard {
            fn drop(&mut self) {
                self.state
                    .db_pending_tasks
                    .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            }
        }

        let db = state.db.clone();
        let state_clone = state.clone();
        state
            .db_pending_tasks
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        tokio::spawn(async move {
            let _guard = DbTaskGuard { state: state_clone };
            let mut attempts = 0;
            let mut backoff = std::time::Duration::from_millis(100);
            loop {
                match db.save_player(&row).await {
                    Ok(()) => break,
                    Err(e) => {
                        attempts += 1;
                        if attempts >= 3 {
                            tracing::error!(
                                player_id = pid,
                                error = ?e,
                                "Failed to save player on disconnect after 3 attempts"
                            );
                            break;
                        }
                        tracing::warn!(
                            player_id = pid,
                            error = ?e,
                            attempt = attempts,
                            "Failed to save player on disconnect, retrying in {:?}",
                            backoff
                        );
                        tokio::time::sleep(backoff).await;
                        backoff *= 2;
                    }
                }
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
    tracing::info!(
        player_id = pid,
        "Player disconnected and ECS entity despawned"
    );
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
            tracing::error!(
                player_id = pid,
                "PlayerStats missing; skipping teleport and clan sync at init"
            );
            return None;
        };

        // 11. tp(x, y)
        tracing::info!(
            player_id = pid,
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

    // BUG 4: Broadcast @T appearance to nearby players so they see the newly logged-in player.
    if let Some((cx, cy, dir, skin, clan_id_u16)) = spawn_broadcast {
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
        state.broadcast_to_nearby(cx, cy, &hb_data, Some(pid));
    }
    // 15. SendChat (login): mO + bounded current-chat history. `Chin` may still
    // arrive later from the client, but client-side mU dedup prevents visual
    // duplicates while this makes first login independent from that timing.
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
    // 16. ConfigPacket
    send_u_packet(tx, "#F", &config_packet("oldprogramformat+").1);
    // 17. UpdateProg (#p) — на свежем логине `ProgrammatorState::new()` ⇒
    // `selected_id`=None, из БД не гидрируется ⇒ if-ветка `#p` не берётся.
    // C# ref эквивалент: при `selected==null` шлётся только `@P false`.
    send_u_packet(tx, "@P", &programmator_status(false).1);
    // 18. DR — индикатор ежедневного бонуса (мигание кнопки БОНУСЫ): "1" если
    // доступен (прошло ≥ кулдауна с последнего клейма), иначе "0".
    let dr = if crate::net::session::play::bonus::bonus_available(player.last_bonus_at) {
        "1"
    } else {
        "0"
    };
    send_u_packet(tx, "DR", dr.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::PlayerRow;
    use crate::world::cells::cell_type;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_spawn_clears_military_block() {
        let dir = std::env::temp_dir();
        let db_path = dir.join(format!("test_spawn_clear_db_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let database = crate::db::Database::open(&db_path).await.unwrap();

        let cell_defs = crate::world::cells::CellDefs::load("configs/cells.json").unwrap();

        let world_name = format!("test_world_spawn_{}", std::process::id());
        let world = crate::world::World::new(&world_name, 2, 2, cell_defs, &dir).unwrap();

        let config = crate::config::Config {
            world_name: world_name.clone(),
            port: 8090,
            world_chunks_w: 2,
            world_chunks_h: 2,
            data_dir: dir.to_string_lossy().to_string(),
            logging: crate::config::LoggingConfig::default(),
            cron: crate::config::CronConfig::default(),
            gameplay: crate::config::GameplayConfig::default(),
        };

        let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config).await;
        // временная система
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
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
        state.active_players.remove(&1);
        connect_in_tick(&state, &tx, &player, 124);

        assert_eq!(state.world.get_cell(5, 5), cell_type::EMPTY);

        // Cleanup
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_v2.map")));
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_durability.mapb")));
    }

    #[tokio::test]
    async fn test_respawn_clears_military_block() {
        let dir = std::env::temp_dir();
        let db_path = dir.join(format!("test_respawn_clear_db_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let database = crate::db::Database::open(&db_path).await.unwrap();

        let cell_defs = crate::world::cells::CellDefs::load("configs/cells.json").unwrap();

        let world_name = format!("test_world_respawn_{}", std::process::id());
        let world = crate::world::World::new(&world_name, 2, 2, cell_defs, &dir).unwrap();

        let config = crate::config::Config {
            world_name: world_name.clone(),
            port: 8090,
            world_chunks_w: 2,
            world_chunks_h: 2,
            data_dir: dir.to_string_lossy().to_string(),
            logging: crate::config::LoggingConfig::default(),
            cron: crate::config::CronConfig::default(),
            gameplay: crate::config::GameplayConfig::default(),
        };

        let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config).await;

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
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

        let mut ecs = state.ecs.write();
        let result = crate::net::session::play::death::apply_player_death_core(&state, &mut ecs, 1);
        assert!(result.is_some());
        let (rx, ry, _, bcast) = result.unwrap();

        // Verify the block was cleared and the cleared position is tracked in DeathBroadcasts
        assert_eq!(state.world.get_cell(rx, ry), cell_type::EMPTY);
        assert_eq!(bcast.cleared_spawn_cell, Some((rx, ry)));

        // Cleanup
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_v2.map")));
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_durability.mapb")));
    }
}

// временная система
