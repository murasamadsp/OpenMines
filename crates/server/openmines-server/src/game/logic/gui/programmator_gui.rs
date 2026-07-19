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

use crate::game::player::PlayerUI;
use crate::net::session::prelude::*;

// ─── Программатор ────────────────────────────────────────────────────────────

pub fn open_create_prog_dialog(state: &Arc<GameState>, tx: &dyn PacketSink, pid: PlayerId) {
    use crate::game::logic::horb::{Button, Horb};

    Horb::new("НОВАЯ ПРОГРАММА")
        .text("Введите название программы")
        .input("Название программы...", true)
        .button(Button::new("Создать", "createprog:%I%"))
        .close_button()
        .send(state, tx, pid, "createprog");
}

pub fn send_programmator_error(tx: &dyn PacketSink, message: &str) {
    send_u_packet(tx, "OK", &ok_message("ПРОГРАММАТОР", message).1);
}

pub fn send_programmator_action_error(tx: &dyn PacketSink, message: &str) {
    send_programmator_error(tx, message);
}

pub fn clear_programmator_window(state: &Arc<GameState>, pid: PlayerId) {
    state.modify_player(pid, |ecs, entity| {
        if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) {
            ui.current_window = None;
        }
        Some(())
    });
}

pub async fn handle_open_prog(
    state: &Arc<GameState>,
    tx: &dyn PacketSink,
    pid: PlayerId,
    prog_id: i32,
) {
    let p = match state.db.get_program(prog_id).await {
        Ok(Some(program)) => program,
        Ok(None) => {
            send_programmator_error(tx, "Программа не найдена.");
            return;
        }
        Err(e) => {
            tracing::error!(player_id = %pid, program_id = prog_id, error = ?e, "DB get failed for openprog");
            send_programmator_error(tx, "Не удалось прочитать программу.");
            return;
        }
    };
    if p.player_id != pid.as_i32() {
        tracing::warn!(
            player_id = %pid,
            program_id = prog_id,
            owner_id = p.player_id,
            "Rejected foreign program open"
        );
        send_programmator_error(tx, "Программа недоступна.");
        return;
    }
    if let Err(e) = state.db.set_selected_program(pid.into(), Some(p.id)).await {
        tracing::error!(player_id = %pid, program_id = p.id, error = ?e, "DB selected program update failed for openprog");
        send_programmator_error(tx, "Не удалось выбрать программу.");
        return;
    }
    state.modify_player(pid, |ecs, entity| {
        if let Some(mut ps) = ecs.get_mut::<crate::game::programmator::ProgrammatorState>(entity) {
            ps.selected_id = Some(p.id);
            ps.selected_data = Some(p.code.clone());
        }
        if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) {
            ui.current_window = None;
        }
        Some(())
    });
    // C# `StaticGUI.OpenProg`: `win = null` (→ `Gu` закрыть список) → `OpenProg` (#P).
    // Без `Gu` окно-список программ не закрывалось поверх редактора.
    send_u_packet(tx, "Gu", &crate::protocol::packets::gu_close().1);
    send_u_packet(
        tx,
        "#P",
        &crate::protocol::packets::open_programmator(p.id, &p.name, &p.code).1,
    );
}

pub async fn handle_create_prog(
    state: &Arc<GameState>,
    tx: &dyn PacketSink,
    pid: PlayerId,
    name: &str,
) {
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    match state.db.insert_program(pid.into(), name, "").await {
        Ok(prog_id) => {
            if let Err(e) = state
                .db
                .set_selected_program(pid.into(), Some(prog_id))
                .await
            {
                tracing::error!(player_id = %pid, program_id = prog_id, error = ?e, "DB selected program update failed for createprog");
                send_programmator_error(tx, "Не удалось выбрать программу.");
                return;
            }
            state.modify_player(pid, |ecs, entity| {
                if let Some(mut ps) =
                    ecs.get_mut::<crate::game::programmator::ProgrammatorState>(entity)
                {
                    ps.selected_id = Some(prog_id);
                    ps.selected_data = Some(String::new());
                }
                if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) {
                    ui.current_window = None;
                }
                Some(())
            });
            // C# `NewProg`: `win = null` (→ `Gu`) перед открытием редактора (#P).
            send_u_packet(tx, "Gu", &crate::protocol::packets::gu_close().1);
            send_u_packet(
                tx,
                "#P",
                &crate::protocol::packets::open_programmator(prog_id, name, "").1,
            );
        }
        Err(e) => {
            tracing::error!(player_id = %pid, error = ?e, "DB insert failed for createprog");
            send_programmator_error(tx, "Не удалось создать программу.");
        }
    }
}

pub async fn handle_rename_prog(
    state: &Arc<GameState>,
    tx: &dyn PacketSink,
    pid: PlayerId,
    prog_id: i32,
    name: &str,
) {
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    let p = match state.db.get_program(prog_id).await {
        Ok(Some(program)) => program,
        Ok(None) => {
            send_programmator_error(tx, "Программа не найдена.");
            return;
        }
        Err(e) => {
            tracing::error!(player_id = %pid, program_id = prog_id, error = ?e, "DB get failed for rename program");
            send_programmator_error(tx, "Не удалось прочитать программу.");
            return;
        }
    };
    if p.player_id != pid.as_i32() {
        tracing::warn!(
            player_id = %pid,
            program_id = prog_id,
            owner_id = p.player_id,
            "Rejected foreign program rename"
        );
        send_programmator_error(tx, "Программа недоступна.");
        return;
    }
    if let Err(e) = state.db.rename_program(prog_id, name).await {
        tracing::error!(player_id = %pid, program_id = prog_id, error = ?e, "DB rename failed for program");
        send_programmator_error(tx, "Не удалось переименовать программу.");
        return;
    }
    clear_programmator_window(state, pid);
    send_u_packet(
        tx,
        "#p",
        &crate::protocol::packets::open_programmator(prog_id, name, &p.code).1,
    );
}
