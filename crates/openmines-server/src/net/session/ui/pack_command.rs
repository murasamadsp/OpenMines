use crate::game::buildings::{BuildingFlags, BuildingStorage};
use crate::game::player::{PlayerFlags, PlayerStats};
use crate::net::session::prelude::*;

pub(super) fn withdraw_state_ready(state: &Arc<GameState>, pid: PlayerId, x: i32, y: i32) -> bool {
    let player_ready = state
        .query_player(pid, |ecs, entity| {
            ecs.get::<PlayerStats>(entity).is_some() && ecs.get::<PlayerFlags>(entity).is_some()
        })
        .unwrap_or(false);
    let building_ready = state
        .query_building_opt(x, y, |ecs, entity| {
            Some(
                ecs.get::<BuildingStorage>(entity).is_some()
                    && ecs.get::<BuildingFlags>(entity).is_some(),
            )
        })
        .unwrap_or(false);
    player_ready && building_ready
}

pub(super) fn send_action_error(tx: &Outbox) {
    send_u_packet(tx, "OK", &ok_message("ЗДАНИЕ", "Некорректное действие.").1);
}

pub(super) fn send_state_error(tx: &Outbox) {
    send_u_packet(
        tx,
        "OK",
        &ok_message("ЗДАНИЕ", "Состояние здания недоступно.").1,
    );
}
