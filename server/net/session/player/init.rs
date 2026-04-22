use crate::db::players::PlayerRow;
use crate::game::player::{
    ActivePlayer, PlayerConnection, PlayerCooldowns, PlayerFlags, PlayerGeoStack, PlayerId,
    PlayerInventory, PlayerMetadata, PlayerPosition, PlayerSettings, PlayerSkills, PlayerStats,
    PlayerUI, PlayerView,
};
use crate::game::programmator::ProgrammatorState;
use crate::net::session::outbound::chat_sync::send_chat_login_per_reference;
use crate::net::session::outbound::inventory_sync::send_inventory;
use crate::net::session::outbound::player_sync::{
    send_player_basket, send_player_health, send_player_level, send_player_skills,
    send_player_speed,
};
use crate::net::session::play::chunks::check_chunk_changed;
use crate::net::session::prelude::*;

#[allow(clippy::similar_names)]
pub fn init_player(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    player: &PlayerRow,
) -> PlayerId {
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
    let ready = now - std::time::Duration::from_secs(1);

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
                total_slots: player.skills.get("__slots").map_or(20, |s| s.level),
                states: {
                    let mut s = player.skills.clone();
                    s.remove("__slots");
                    s
                },
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
                last_move: ready,
                last_dig: ready,
                last_build: ready,
                last_geo: ready,
                protection_until: None,
                last_shot: None,
                c190_stacks: 1,
                last_c190_hit: None,
            },
            PlayerGeoStack::default(),
            ProgrammatorState::new(),
            PlayerSettings {
                auto_dig: player.auto_dig,
            },
            PlayerFlags { dirty: false },
        ))
        .id();

    state
        .active_players
        .insert(pid, ActivePlayer { ecs_entity: entity });

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
    pid
}

pub fn on_disconnect(state: &Arc<GameState>, pid: PlayerId) {
    let Some((_, p)) = state.active_players.remove(&pid) else {
        return;
    };
    let entity = p.ecs_entity;

    // После `remove` `modify_player(pid, …)` уже не найдёт сущность в `active_players` — сохраняем и чанк из ECS напрямую.
    let (cx, cy) = {
        let ecs = state.ecs.read();
        let chunk = ecs
            .get::<PlayerPosition>(entity)
            .map(|pos| (pos.chunk_x(), pos.chunk_y()))
            .unwrap_or((0, 0));
        if let Some(row) = crate::game::player::extract_player_row(&ecs, entity) {
            if let Err(e) = state.db.save_player(&row) {
                tracing::error!("Failed to save player {pid} on disconnect: {e}");
            }
        }
        chunk
    };

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
    let spawn_broadcast = state
        .query_player(pid, |ecs, entity| {
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
        })
        .flatten();

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
    // 15. SendChat — как `Player.SendChat()` в server_reference: только `mO` и при наличии — `mU`.
    send_chat_login_per_reference(state, tx, pid);
    // 16. ConfigPacket
    send_u_packet(tx, "#F", &config_packet("oldprogramformat+").1);
    // 17. UpdateProg (#p) — C# ref: if programsData.selected != null → UpdateProg()
    let prog_data = state
        .query_player(pid, |ecs, entity| {
            let ps = ecs.get::<crate::game::programmator::ProgrammatorState>(entity)?;
            ps.selected_id.map(|id| (id, ps.running))
        })
        .flatten();
    if let Some((prog_id, prog_running)) = prog_data {
        if let Ok(Some(prog)) = state.db.get_program(prog_id) {
            send_u_packet(
                tx,
                "#p",
                &crate::protocol::packets::open_programmator(prog_id, &prog.name, &prog.code).1,
            );
        }
        send_u_packet(tx, "@P", &programmator_status(prog_running).1);
    } else {
        send_u_packet(tx, "@P", &programmator_status(false).1);
    }
}
