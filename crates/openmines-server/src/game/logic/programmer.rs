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
    clippy::nonminimal_bool
)]
use crate::game::player::PlayerUI;
use crate::net::session::prelude::*;
use std::sync::Arc;

pub fn apply_editor_open(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    program_id: i32,
    name: &str,
    source: &str,
) {
    state.modify_player(pid, |ecs, entity| {
        if let Some(mut programmer) =
            ecs.get_mut::<crate::game::programmator::ProgrammatorState>(entity)
        {
            programmer.selected_id = Some(program_id);
            programmer.selected_data = Some(source.to_owned());
        }
        if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) {
            ui.current_window = None;
        }
        Some(())
    });
    send_u_packet(tx, "Gu", &crate::protocol::packets::gu_close().1);
    send_u_packet(
        tx,
        "#P",
        &crate::protocol::packets::open_programmator(program_id, name, source).1,
    );
    send_u_packet(tx, "Gu", &crate::protocol::packets::gu_close().1);
}

pub fn apply_editor_rename(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    program_id: i32,
    name: &str,
    source: &str,
) {
    clear_window(state, pid);
    send_u_packet(
        tx,
        "#p",
        &crate::protocol::packets::open_programmator(program_id, name, source).1,
    );
    send_u_packet(tx, "Gu", &crate::protocol::packets::gu_close().1);
}

pub(super) fn clear_window(state: &Arc<GameState>, pid: PlayerId) {
    state.modify_player(pid, |ecs, entity| {
        ecs.get_mut::<PlayerUI>(entity)?.current_window = None;
        Some(())
    });
}
