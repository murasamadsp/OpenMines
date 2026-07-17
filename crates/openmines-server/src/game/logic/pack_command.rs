#![allow(
    clippy::too_many_lines,
    clippy::needless_pass_by_value,
    clippy::option_if_let_else,
    clippy::assigning_clones,
    clippy::items_after_statements,
    clippy::used_underscore_binding,
    clippy::semicolon_if_nothing_returned,
    clippy::missing_panics_doc,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::significant_drop_tightening,
    clippy::map_unwrap_or,
    clippy::manual_let_else,
    clippy::format_push_string,
    clippy::single_match_else,
    clippy::nonminimal_bool,
    clippy::collapsible_if,
    clippy::cast_possible_wrap,
    clippy::redundant_closure_for_method_calls
)]
use crate::game::buildings::{BuildingFlags, BuildingStorage};
use crate::game::player::{PlayerFlags, PlayerStats};
use crate::net::session::prelude::*;

pub fn withdraw_state_ready(state: &Arc<GameState>, pid: PlayerId, x: i32, y: i32) -> bool {
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

pub fn send_action_error(tx: &Outbox) {
    send_u_packet(tx, "OK", &ok_message("ЗДАНИЕ", "Некорректное действие.").1);
}

pub fn send_state_error(tx: &Outbox) {
    send_u_packet(
        tx,
        "OK",
        &ok_message("ЗДАНИЕ", "Состояние здания недоступно.").1,
    );
}
