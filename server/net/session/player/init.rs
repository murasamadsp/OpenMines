use crate::db::players::PlayerRow;
use crate::game::player::{
    ActivePlayer, PlayerConnection, PlayerCooldowns, PlayerFlags, PlayerId, PlayerInventory,
    PlayerMetadata, PlayerPosition, PlayerSettings, PlayerSkills, PlayerStats, PlayerUI, PlayerView,
};
use crate::net::session::outbound::inventory_sync::send_inventory;
use crate::net::session::outbound::player_sync::{
    send_player_basket, send_player_health, send_player_level, send_player_skills, send_player_speed,
};
use crate::net::session::play::chunks::check_chunk_changed;
use crate::net::session::prelude::*;

pub fn init_player(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, player: &PlayerRow) -> PlayerId {
    let pid = player.id;
    let now = std::time::Instant::now();

    let entity = state.ecs.write().spawn((
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
        },
        PlayerInventory {
            items: player.inventory.clone(),
            selected: -1,
        },
        PlayerSkills { states: player.skills.clone() },
        PlayerView {
            last_chunk: None,
            visible_chunks: Vec::new(),
        },
        PlayerUI {
            current_window: None,
            current_chat: "FED".to_string(),
        },
        PlayerCooldowns {
            last_move: now,
            last_dig: now,
            protection_until: None,
            last_shot: None,
        },
        PlayerSettings {
            auto_dig: player.auto_dig,
        },
        PlayerFlags { dirty: false },
    )).id();

    state.active_players.insert(pid, ActivePlayer { ecs_entity: entity });
    send_initial_sync(state, tx, player);
    pid
}

pub fn on_disconnect(state: &Arc<GameState>, pid: PlayerId) {
    if let Some((_, p)) = state.active_players.remove(&pid) {
        let (cx, cy) = state.modify_player(pid, |ecs, entity| {
            let pos = ecs.get::<PlayerPosition>(entity)?;
            let row = crate::game::player::extract_player_row(ecs, entity)?;
            if let Err(e) = state.db.save_player(&row) {
                tracing::error!("Failed to save player {pid} on disconnect: {e}");
            }
            Some((pos.chunk_x(), pos.chunk_y()))
        }).flatten().unwrap_or((0, 0));

        state.chunk_players.get_mut(&(cx, cy)).map(|mut e| e.retain(|&id| id != pid));
        
        let sub = crate::protocol::packets::hb_bot_del(net_u16_nonneg(pid));
        let hb_data = encode_hb_bundle(&hb_bundle(&[sub]).1);
        state.broadcast_to_nearby(cx, cy, &hb_data, None);

        state.ecs.write().despawn(p.ecs_entity);
        tracing::info!("Player {pid} disconnected and ECS entity despawned");
    }
}

fn send_initial_sync(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, player: &PlayerRow) {
    let pid = player.id;
    state.query_player(pid, |ecs, entity| {
        let stats = ecs.get::<PlayerStats>(entity).unwrap();
        let skills = ecs.get::<PlayerSkills>(entity).unwrap();
        let inv = ecs.get::<PlayerInventory>(entity).unwrap();

        send_u_packet(tx, "AU", &au_session(&format!("{pid}")).1);
        send_u_packet(tx, "ST", &status(&format!("Добро пожаловать, {}!", player.name)).1);
        send_u_packet(tx, "AH", &auth_hash(pid, &player.hash).1);
        send_u_packet(tx, "@T", &tp(player.x, player.y).1);

        send_player_health(tx, stats);
        send_player_speed(tx, skills);
        send_player_basket(tx, stats, skills);
        send_player_level(tx, skills);

        let online_val = i32::try_from(state.online_count()).unwrap_or(i32::MAX);
        send_u_packet(tx, "ON", &online(online_val, 200).1);
        send_u_packet(tx, "P$", &money(player.money, player.creds).1);
        send_u_packet(tx, "BD", &auto_digg(player.auto_dig).1);

        send_inventory(tx, inv);
        send_player_skills(tx, skills);
    });
    check_chunk_changed(state, tx, pid);
}
