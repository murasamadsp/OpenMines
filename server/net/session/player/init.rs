//! Вход игрока в мир после авторизации.
use crate::net::session::outbound::chat_sync::send_chat_init;
use crate::net::session::outbound::inventory_sync::send_inventory;
use crate::net::session::play::chunks::check_chunk_changed;
use crate::net::session::prelude::*;

// ─── Player init ────────────────────────────────────────────────────────────

pub fn init_player(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    player: &crate::db::PlayerRow,
) -> PlayerId {
    let pid = player.id;

    // Remove old session if exists
    state.active_players.remove(&pid);

    // Close any GUI
    send_u_packet(tx, "Gu", &[]);

    let entity = state
        .ecs
        .write()
        .spawn(crate::game::PlayerComponent { pid })
        .id();

    let active = ActivePlayer {
        auto_dig: player.auto_dig,
        inv_selected: -1,
        current_window: None,
        current_chat: "FED".to_string(),
        data: player.clone(),
        tx: tx.clone(),
        last_chunk: None,
        visible_chunks: vec![],
        dirty: false,
        ecs_entity: entity,
        last_move_ts: std::time::Instant::now(),
        last_dig_ts: std::time::Instant::now(),
        protection_until: None,
        last_shot_ts: None,
    };
    state.active_players.insert(pid, active);

    // Register in spatial chunk index
    state
        .chunk_players
        .entry(player_chunk(player.x, player.y))
        .or_default()
        .push(pid);

    // Send player info
    send_u_packet(tx, "BD", &auto_digg(player.auto_dig).1);
    send_u_packet(tx, "GE", &geo(player.x, player.y).1);

    use crate::net::session::outbound::player_sync::{
        send_player_basket, send_player_health, send_player_level, send_player_skills,
        send_player_speed,
    };
    send_player_health(tx, player);
    send_u_packet(tx, "BI", &bot_info(&player.name, player.x, player.y, pid).1);
    send_player_speed(tx, player);
    send_player_basket(tx, player);
    send_u_packet(tx, "P$", &money(player.money, player.creds).1);
    send_player_level(tx, &player.skills);
    send_u_packet(
        tx,
        "ON",
        &online(i32::try_from(state.online_count()).unwrap_or(i32::MAX), 200).1,
    );
    send_u_packet(tx, "#F", &config_packet("oldprogramformat+").1);
    send_u_packet(tx, "@P", &programmator_status(false).1);

    // Send inventory
    send_inventory(tx, &player.inventory, -1);

    // Send skills progress
    send_player_skills(tx, &player.skills);

    // Send clan info
    if let Some(cid) = player.clan_id {
        send_u_packet(tx, "CS", &clan_show(cid).1);
    } else {
        send_u_packet(tx, "CH", &clan_hide().1);
    }

    // Send initial chunks around player
    check_chunk_changed(state, tx, pid);

    // Teleport to position
    send_u_packet(tx, "@T", &tp(player.x, player.y).1);

    // Send chat channel data
    send_chat_init(state, tx, pid, "FED");

    pid
}

pub fn on_disconnect(state: &Arc<GameState>, pid: PlayerId) {
    if let Some((_, player)) = state.active_players.remove(&pid) {
        // Despawn ECS entity
        state.ecs.write().despawn(player.ecs_entity);

        // Remove from spatial chunk index
        let chunk_pos = player_chunk(player.data.x, player.data.y);
        let should_remove_chunk =
            state
                .chunk_players
                .get_mut(&chunk_pos)
                .is_some_and(|mut entry| {
                    entry.retain(|&id| id != pid);
                    entry.is_empty()
                });
        if should_remove_chunk {
            state.chunk_players.remove(&chunk_pos);
        }
        // Persist to DB
        if let Err(e) = state.db.save_player(&player.data) {
            tracing::error!("Failed to save player {pid}: {e}");
        }
        tracing::info!("Player {} (id={pid}) disconnected", player.data.name);
    }
}

fn player_chunk(x: i32, y: i32) -> (u32, u32) {
    World::chunk_pos(x, y)
}
