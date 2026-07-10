//! Application of ordered player commands inside the simulation boundary.
//!
//! This module is intentionally still calling legacy session handlers while the
//! kernel migration is in progress. The important boundary is that lifecycle
//! drains commands and this module owns command application.

use crate::game::{CommandEffects, GameState, PlayerCommand};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

struct PendingTaskGuard {
    state: Arc<GameState>,
}

impl Drop for PendingTaskGuard {
    fn drop(&mut self) {
        self.state.db_pending_tasks.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn apply_player_command(state: &Arc<GameState>, command: PlayerCommand) -> CommandEffects {
    match command {
        command @ (PlayerCommand::Connect { .. }
        | PlayerCommand::Disconnect { .. }
        | PlayerCommand::Move { .. }) => apply_session_command(state, command),
        command @ (PlayerCommand::Dig { .. }
        | PlayerCommand::Build { .. }
        | PlayerCommand::Geology { .. }
        | PlayerCommand::Heal { .. }
        | PlayerCommand::Respawn { .. }
        | PlayerCommand::OpenBox { .. }
        | PlayerCommand::ClaimBonus { .. }) => {
            apply_gameplay_command(state, command);
            CommandEffects::default()
        }
        command @ (PlayerCommand::InventoryToggle { .. }
        | PlayerCommand::InventoryChoose { .. }
        | PlayerCommand::InventoryUse { .. }
        | PlayerCommand::ToggleAutoDig { .. }
        | PlayerCommand::ToggleAggression { .. }
        | PlayerCommand::SettingsSave { .. }) => {
            apply_inventory_command(state, command);
            CommandEffects::default()
        }
        command @ (PlayerCommand::GuiButton { .. }
        | PlayerCommand::AdminAction { .. }
        | PlayerCommand::OpenProgrammer { .. }
        | PlayerCommand::RequestMyBuildings { .. }
        | PlayerCommand::OpenClan { .. }) => {
            apply_presentation_command(state, command);
            CommandEffects::default()
        }
        command @ (PlayerCommand::LocalChat { .. }
        | PlayerCommand::ChannelChat { .. }
        | PlayerCommand::ChatResync { .. }
        | PlayerCommand::ChatMenu { .. }
        | PlayerCommand::ChatChoose { .. }
        | PlayerCommand::ChatSettings { .. }
        | PlayerCommand::ChatPrivate { .. }
        | PlayerCommand::Whois { .. }) => {
            apply_chat_command(state, command);
            CommandEffects::default()
        }
        command @ (PlayerCommand::ProgramAction { .. }
        | PlayerCommand::ApplySavedProgram { .. }
        | PlayerCommand::ApplyDeletedProgram { .. }
        | PlayerCommand::ApplyProgramEditorOpen { .. }
        | PlayerCommand::ApplyProgramEditorRename { .. }) => {
            apply_program_command(state, command);
            CommandEffects::default()
        }
        command @ (PlayerCommand::ApplyInventoryBuildingPlaced { .. }
        | PlayerCommand::ApplyPaidBuildingPlaced { .. }
        | PlayerCommand::RefundPaidBuildingPlacement { .. }
        | PlayerCommand::ApplyRemovedBuilding { .. }) => {
            apply_building_completion(state, command);
            CommandEffects::default()
        }
        PlayerCommand::KnownNoopTy {
            player_id,
            event,
            payload,
        } => {
            if let Some(tx) = state.player_sender(player_id) {
                handle_known_noop_ty(&tx, player_id, &event, &payload);
            }
            CommandEffects::default()
        }
    }
}

fn apply_session_command(state: &Arc<GameState>, command: PlayerCommand) -> CommandEffects {
    let mut effects = CommandEffects::default();
    match command {
        crate::game::PlayerCommand::Connect { row, session_id } => {
            effects.append(crate::net::session::player::init::connect_entity_in_tick(
                state, &row, session_id,
            ));
            effects.append(
                crate::net::session::player::init::prepare_initial_presentation(
                    state, &row, session_id,
                ),
            );
        }
        crate::game::PlayerCommand::Disconnect {
            player_id,
            session_id,
        } => {
            effects.append(crate::net::session::player::init::disconnect_in_tick(
                state, player_id, session_id,
            ));
        }
        crate::game::PlayerCommand::Move {
            player_id,
            session_id,
            time: _,
            x,
            y,
            direction,
            programmatic,
        } => {
            effects.append(crate::net::session::play::movement::apply_move_command(
                state,
                player_id,
                session_id,
                crate::net::session::play::movement::MoveRequest {
                    target_x: x,
                    target_y: y,
                    direction,
                    programmatic,
                },
            ));
        }
        _ => unreachable!("non-session command routed to session command handler"),
    }
    effects
}

fn apply_gameplay_command(state: &Arc<GameState>, command: PlayerCommand) {
    match command {
        crate::game::PlayerCommand::Dig {
            player_id,
            direction,
            programmatic,
        } => {
            if let Some(tx) = state.player_sender(player_id) {
                crate::net::session::play::dig_build::handle_dig(
                    state,
                    &tx,
                    player_id,
                    direction,
                    programmatic,
                );
            }
        }
        crate::game::PlayerCommand::Build {
            player_id,
            direction,
            block_type,
            programmatic,
        } => {
            if let Some(tx) = state.player_sender(player_id) {
                let bld = crate::protocol::packets::XbldClient {
                    direction,
                    block_type: &block_type,
                };
                crate::net::session::play::dig_build::handle_build(
                    state,
                    &tx,
                    player_id,
                    &bld,
                    programmatic,
                );
            }
        }
        crate::game::PlayerCommand::Geology {
            player_id,
            programmatic,
        } => {
            if let Some(tx) = state.player_sender(player_id) {
                apply_geology_command(state, &tx, player_id, programmatic);
            }
        }
        crate::game::PlayerCommand::Heal {
            player_id,
            programmatic,
        } => {
            if let Some(tx) = state.player_sender(player_id) {
                apply_heal_command(state, &tx, player_id, programmatic);
            }
        }
        crate::game::PlayerCommand::Respawn { player_id } => {
            if let Some(tx) = state.player_sender(player_id) {
                crate::net::session::play::death::handle_death(state, &tx, player_id);
            }
        }
        crate::game::PlayerCommand::OpenBox { player_id } => {
            if let Some(tx) = state.player_sender(player_id) {
                crate::net::session::social::buildings::handle_dpbx_crystal_box(
                    state, &tx, player_id,
                );
            }
        }
        crate::game::PlayerCommand::ClaimBonus { player_id } => {
            if let Some(tx) = state.player_sender(player_id) {
                apply_bonus_claim(state, &tx, player_id);
            }
        }
        _ => unreachable!("non-gameplay command routed to gameplay command handler"),
    }
}

fn apply_inventory_command(state: &Arc<GameState>, command: PlayerCommand) {
    match command {
        PlayerCommand::InventoryToggle { player_id } => {
            if let Some(tx) = state.player_sender(player_id) {
                apply_inventory_result(
                    &tx,
                    player_id,
                    crate::game::logic::inventory::toggle_inventory(state, player_id),
                    "toggle",
                );
            }
        }
        PlayerCommand::InventoryChoose { player_id, payload } => {
            if let Some(tx) = state.player_sender(player_id) {
                apply_inventory_result(
                    &tx,
                    player_id,
                    crate::game::logic::inventory::choose_inventory(state, player_id, &payload),
                    "choose",
                );
            }
        }
        PlayerCommand::InventoryUse { player_id } => apply_inventory_use(state, player_id),
        PlayerCommand::ToggleAutoDig { player_id } => {
            if let Some(tx) = state.player_sender(player_id) {
                apply_auto_dig_result(
                    &tx,
                    player_id,
                    crate::game::logic::settings::toggle_auto_dig(state, player_id),
                    "toggle",
                );
            }
        }
        PlayerCommand::ToggleAggression { player_id } => {
            if let Some(tx) = state.player_sender(player_id) {
                apply_aggression_result(
                    &tx,
                    player_id,
                    crate::game::logic::settings::toggle_aggression(state, player_id),
                    "toggle",
                );
            }
        }
        PlayerCommand::SettingsSave { player_id, payload } => {
            if let Some(tx) = state.player_sender(player_id) {
                if !payload.is_empty() {
                    tracing::debug!(player_id = %player_id, bytes = payload.len(), "Sett TY payload ignored");
                }
                crate::net::session::ui::gui_buttons::open_settings_gui(state, &tx, player_id);
            }
        }
        _ => unreachable!("non-inventory command routed to inventory command handler"),
    }
}

fn apply_inventory_use(state: &Arc<GameState>, player_id: crate::game::PlayerId) {
    let Some(tx) = state.player_sender(player_id) else {
        return;
    };
    if crate::net::session::ui::heal_inventory::handle_inventory_use_sync_nonbuilding(
        state, &tx, player_id,
    ) {
        return;
    }
    if let Some(placement) = crate::net::session::ui::heal_inventory::prepare_inventory_building_use(
        state, &tx, player_id,
    ) {
        spawn_inventory_building_insert_task(state, tx, placement);
    }
}

fn apply_chat_command(state: &Arc<GameState>, command: PlayerCommand) {
    match command {
        crate::game::PlayerCommand::LocalChat { player_id, message } => {
            apply_local_chat_command(state, player_id, message);
        }
        crate::game::PlayerCommand::ChannelChat { player_id, payload } => {
            apply_channel_chat_command(state, player_id, payload);
        }
        crate::game::PlayerCommand::ChatResync { player_id, payload } => {
            if let Some(tx) = state.player_sender(player_id) {
                let task_state = state.clone();
                spawn_session_async_task(state, "chat_resync", async move {
                    crate::net::session::social::chat::handle_chat_resync(
                        &task_state,
                        &tx,
                        player_id,
                        &payload,
                    )
                    .await;
                });
            }
        }
        crate::game::PlayerCommand::ChatMenu { player_id, payload } => {
            if let Some(tx) = state.player_sender(player_id) {
                let task_state = state.clone();
                spawn_session_async_task(state, "chat_menu", async move {
                    crate::net::session::social::chat::handle_chat_menu(
                        &task_state,
                        &tx,
                        player_id,
                        &payload,
                    )
                    .await;
                });
            }
        }
        crate::game::PlayerCommand::ChatChoose { player_id, payload } => {
            if let Some(tx) = state.player_sender(player_id) {
                let task_state = state.clone();
                spawn_session_async_task(state, "chat_choose", async move {
                    crate::net::session::social::chat::handle_chat_choose(
                        &task_state,
                        &tx,
                        player_id,
                        &payload,
                    )
                    .await;
                });
            }
        }
        crate::game::PlayerCommand::ChatSettings { player_id, payload } => {
            if let Some(tx) = state.player_sender(player_id) {
                let task_state = state.clone();
                spawn_session_async_task(state, "chat_settings", async move {
                    crate::net::session::social::chat::handle_chat_settings(
                        &task_state,
                        &tx,
                        player_id,
                        &payload,
                    )
                    .await;
                });
            }
        }
        crate::game::PlayerCommand::ChatPrivate { player_id, payload } => {
            apply_private_chat_command(state, player_id, payload);
        }
        crate::game::PlayerCommand::Whois { player_id, ids } => {
            if let Some(tx) = state.player_sender(player_id) {
                let task_state = state.clone();
                spawn_session_async_task(state, "whois", async move {
                    crate::net::session::social::misc::handle_whoi(&task_state, &tx, &ids).await;
                });
            }
        }
        _ => unreachable!("non-chat command routed to chat command handler"),
    }
}

fn apply_local_chat_command(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    message: String,
) {
    let Some(tx) = state.player_sender(player_id) else {
        return;
    };
    if !state.check_chat_rate(player_id) {
        tracing::debug!(player_id = %player_id, "chat rate limited (Locl)");
        return;
    }
    if crate::net::session::social::chat::handle_local_chat_non_command(
        state, &tx, player_id, &message,
    ) {
        return;
    }
    let task_state = state.clone();
    spawn_session_async_task(state, "local_chat_command", async move {
        crate::net::session::social::chat::handle_local_chat(&task_state, &tx, player_id, &message)
            .await;
    });
}

fn apply_channel_chat_command(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    payload: bytes::Bytes,
) {
    let Some(tx) = state.player_sender(player_id) else {
        return;
    };
    if !state.check_chat_rate(player_id) {
        tracing::debug!(player_id = %player_id, "chat rate limited (Chat)");
        return;
    }
    let text = crate::net::session::social::chat::extract_channel_message_text(&payload);
    if text.trim().starts_with('/') {
        let task_state = state.clone();
        spawn_session_async_task(state, "channel_chat_command", async move {
            crate::net::session::social::chat::handle_channel_chat(
                &task_state,
                &tx,
                player_id,
                &payload,
            )
            .await;
        });
    } else if let Some(prepared) =
        crate::net::session::social::chat::prepare_channel_chat_non_command(
            state, &tx, player_id, &text,
        )
    {
        let task_state = state.clone();
        spawn_session_async_task(state, "channel_chat", async move {
            crate::net::session::social::chat::persist_prepared_channel_chat(
                &task_state,
                &tx,
                player_id,
                prepared,
            )
            .await;
        });
    }
}

fn apply_private_chat_command(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    payload: bytes::Bytes,
) {
    let Some(tx) = state.player_sender(player_id) else {
        return;
    };
    if !state.check_chat_rate(player_id) {
        tracing::debug!(player_id = %player_id, "chat rate limited (Cpri)");
        return;
    }
    let task_state = state.clone();
    spawn_session_async_task(state, "chat_private", async move {
        crate::net::session::social::chat::handle_chat_private(
            &task_state,
            &tx,
            player_id,
            &payload,
        )
        .await;
    });
}

fn apply_presentation_command(state: &Arc<GameState>, command: PlayerCommand) {
    match command {
        PlayerCommand::GuiButton { player_id, button } => {
            apply_gui_button_command(state, player_id, button);
        }
        crate::game::PlayerCommand::AdminAction { player_id } => {
            if let Some(tx) = state.player_sender(player_id) {
                crate::net::session::social::commands::handle_admin_action(state, &tx, player_id);
            }
        }
        crate::game::PlayerCommand::OpenProgrammer { player_id } => {
            if let Some(tx) = state.player_sender(player_id) {
                let task_state = state.clone();
                spawn_session_async_task(state, "open_programmer", async move {
                    crate::net::session::social::buildings::handle_programmator_pope_menu(
                        &task_state,
                        &tx,
                        player_id,
                    )
                    .await;
                });
            }
        }
        crate::game::PlayerCommand::RequestMyBuildings { player_id } => {
            if let Some(tx) = state.player_sender(player_id) {
                let task_state = state.clone();
                spawn_session_async_task(state, "request_my_buildings", async move {
                    crate::net::session::social::buildings::handle_my_buildings_list(
                        &task_state,
                        &tx,
                        player_id,
                    )
                    .await;
                });
            }
        }
        crate::game::PlayerCommand::OpenClan { player_id } => {
            if let Some(tx) = state.player_sender(player_id) {
                let task_state = state.clone();
                spawn_session_async_task(state, "open_clan", async move {
                    crate::net::session::social::clans::handle_clan_menu(
                        &task_state,
                        &tx,
                        player_id,
                    )
                    .await;
                });
            }
        }
        _ => unreachable!("non-presentation command routed to presentation command handler"),
    }
}

fn apply_gui_button_command(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    button: String,
) {
    let Some(tx) = state.player_sender(player_id) else {
        return;
    };
    if !state.check_gui_rate(player_id) {
        tracing::debug!(player_id = %player_id, "gui rate limited (GUI_)");
        return;
    }
    if let Some(type_code) = button.strip_prefix("bld_place:") {
        if let Some(placement) =
            crate::net::session::social::buildings::prepare_paid_building_placement(
                state, &tx, player_id, type_code,
            )
        {
            spawn_paid_building_insert_task(state, tx, placement);
        }
        return;
    }
    if let Some((x, y)) = parse_pack_remove_button(&button) {
        if let Some(removal) = crate::net::session::social::buildings::prepare_building_removal(
            state, &tx, player_id, x, y,
        ) {
            spawn_building_remove_task(state, tx, removal);
        }
        return;
    }
    if let Some(program_id) = button
        .strip_prefix("openprog:")
        .and_then(|rest| rest.parse::<i32>().ok())
    {
        spawn_program_editor_open_task(state, tx, player_id, program_id);
        return;
    }
    if let Some(name) = button.strip_prefix("createprog:") {
        spawn_program_editor_create_task(state, tx, player_id, name);
        return;
    }
    if let Some((program_id, name)) = parse_program_rename_button(&button) {
        spawn_program_editor_rename_task(state, tx, player_id, program_id, &name);
        return;
    }
    if crate::net::session::ui::gui_buttons::handle_gui_button_sync_fast_path(
        state, &tx, player_id, &button,
    ) {
        return;
    }
    spawn_gui_async_task(state, tx, player_id, button);
}

#[derive(Clone, Copy)]
enum GuiAsyncHandler {
    Auction,
    Clan,
    Programmer,
    Legacy,
}

fn spawn_gui_async_task(
    state: &Arc<GameState>,
    tx: crate::net::session::outbox::Outbox,
    player_id: crate::game::PlayerId,
    button: String,
) {
    let handler = if crate::net::session::ui::gui_buttons::is_auction_button(&button) {
        GuiAsyncHandler::Auction
    } else if crate::net::session::ui::gui_buttons::is_clan_button(&button) {
        GuiAsyncHandler::Clan
    } else if crate::net::session::ui::gui_buttons::is_programmer_button(&button) {
        GuiAsyncHandler::Programmer
    } else {
        GuiAsyncHandler::Legacy
    };
    let task_name = match handler {
        GuiAsyncHandler::Auction => "auction_gui",
        GuiAsyncHandler::Clan => "clan_gui",
        GuiAsyncHandler::Programmer => "programmer_gui",
        GuiAsyncHandler::Legacy => "legacy_gui_button",
    };
    let task_state = state.clone();
    spawn_session_async_task(state, task_name, async move {
        match handler {
            GuiAsyncHandler::Auction => {
                crate::net::session::ui::gui_buttons::handle_auction_button(
                    &task_state,
                    &tx,
                    player_id,
                    &button,
                )
                .await;
            }
            GuiAsyncHandler::Clan => {
                crate::net::session::ui::gui_buttons::handle_clan_button(
                    &task_state,
                    &tx,
                    player_id,
                    &button,
                )
                .await;
            }
            GuiAsyncHandler::Programmer => {
                crate::net::session::ui::gui_buttons::handle_programmer_button(
                    &task_state,
                    &tx,
                    player_id,
                    &button,
                )
                .await;
            }
            GuiAsyncHandler::Legacy => {
                crate::net::session::ui::gui_buttons::handle_gui_button(
                    &task_state,
                    &tx,
                    player_id,
                    &button,
                )
                .await;
            }
        }
    });
}

fn apply_program_command(state: &Arc<GameState>, command: PlayerCommand) {
    match command {
        crate::game::PlayerCommand::ProgramAction {
            player_id,
            event,
            payload,
        } => {
            if let Some(tx) = state.player_sender(player_id) {
                match event.as_str() {
                    "PROG" => spawn_program_save_task(state, tx, player_id, payload),
                    "PDEL" => spawn_program_delete_task(state, tx, player_id, payload),
                    "pRST" => crate::net::session::social::misc::handle_prog_reset_ty(
                        state, &tx, player_id,
                    ),
                    "PREN" => crate::net::session::social::misc::handle_prog_rename_prompt_ty(
                        state, &tx, player_id, &payload,
                    ),
                    "PCOP" => {
                        let task_state = state.clone();
                        spawn_session_async_task(state, "program_copy", async move {
                            crate::net::session::social::misc::handle_prog_ty(
                                &task_state,
                                &tx,
                                player_id,
                                "PCOP",
                                &payload,
                            )
                            .await;
                        });
                    }
                    _ => tracing::warn!(
                        player_id = %player_id,
                        event,
                        "unknown program action reached tick"
                    ),
                }
            }
        }
        crate::game::PlayerCommand::ApplySavedProgram {
            player_id,
            session_id,
            program_id,
            program_name,
            source,
        } => {
            let Some(tx) = state.sessions.outbox_for_session(session_id) else {
                return;
            };
            crate::net::session::social::misc::apply_saved_program_to_tick_state(
                state,
                &tx,
                player_id,
                program_id,
                &program_name,
                &source,
            );
        }
        crate::game::PlayerCommand::ApplyDeletedProgram {
            player_id,
            program_id,
        } => {
            let cleared = crate::net::session::social::misc::clear_deleted_program_runtime(
                state, player_id, program_id,
            );
            if cleared {
                let task_state = state.clone();
                spawn_session_async_task(state, "program_clear_selected", async move {
                    if let Err(e) = task_state
                        .db
                        .set_selected_program(player_id.into(), None)
                        .await
                    {
                        tracing::error!(player_id = %player_id, program_id, error = ?e, "DB selected program clear failed after delete");
                    }
                });
            }
        }
        PlayerCommand::ApplyProgramEditorOpen { .. }
        | PlayerCommand::ApplyProgramEditorRename { .. } => {
            apply_program_editor_completion(state, command);
        }
        _ => unreachable!("non-program command routed to program command handler"),
    }
}

fn apply_building_completion(state: &Arc<GameState>, command: PlayerCommand) {
    match command {
        crate::game::PlayerCommand::ApplyInventoryBuildingPlaced {
            session_id,
            placement,
            db_id,
        } => {
            let Some(tx) = state.sessions.outbox_for_session(session_id) else {
                return;
            };
            crate::net::session::ui::heal_inventory::apply_inventory_building_placed(
                state, &tx, &placement, db_id,
            );
        }
        crate::game::PlayerCommand::ApplyPaidBuildingPlaced {
            session_id,
            placement,
            db_id,
        } => {
            let Some(tx) = state.sessions.outbox_for_session(session_id) else {
                return;
            };
            crate::net::session::social::buildings::apply_paid_building_placed(
                state, &tx, &placement, db_id,
            );
        }
        crate::game::PlayerCommand::RefundPaidBuildingPlacement {
            session_id,
            player_id,
            cost,
        } => {
            let Some(tx) = state.sessions.outbox_for_session(session_id) else {
                return;
            };
            crate::net::session::social::buildings::refund_paid_building_placement(
                state, &tx, player_id, cost,
            );
        }
        crate::game::PlayerCommand::ApplyRemovedBuilding { removal } => {
            crate::net::session::social::buildings::apply_removed_building(state, &removal);
        }
        _ => unreachable!("non-building command routed to building completion handler"),
    }
}

fn apply_program_editor_completion(state: &Arc<GameState>, command: PlayerCommand) {
    match command {
        crate::game::PlayerCommand::ApplyProgramEditorOpen {
            session_id,
            player_id,
            program_id,
            program_name,
            source,
        } => {
            let Some(tx) = state.sessions.outbox_for_session(session_id) else {
                return;
            };
            crate::net::session::ui::gui_buttons::apply_program_editor_open(
                state,
                &tx,
                player_id,
                program_id,
                &program_name,
                &source,
            );
        }
        crate::game::PlayerCommand::ApplyProgramEditorRename {
            session_id,
            player_id,
            program_id,
            program_name,
            source,
        } => {
            let Some(tx) = state.sessions.outbox_for_session(session_id) else {
                return;
            };
            crate::net::session::ui::gui_buttons::apply_program_editor_rename(
                state,
                &tx,
                player_id,
                program_id,
                &program_name,
                &source,
            );
        }
        _ => unreachable!("non-editor command routed to editor completion handler"),
    }
}

pub fn apply_programmator_auto_dig_set(
    state: &Arc<GameState>,
    tx: &crate::net::session::outbox::Outbox,
    player_id: crate::game::PlayerId,
    enabled: bool,
) {
    apply_auto_dig_result(
        tx,
        player_id,
        crate::game::logic::settings::set_auto_dig(state, player_id, enabled),
        "set",
    );
}

pub fn apply_programmator_aggression_set(
    state: &Arc<GameState>,
    tx: &crate::net::session::outbox::Outbox,
    player_id: crate::game::PlayerId,
    enabled: bool,
) {
    apply_aggression_result(
        tx,
        player_id,
        crate::game::logic::settings::set_aggression(state, player_id, enabled),
        "set",
    );
}

pub fn apply_programmator_heal(
    state: &Arc<GameState>,
    tx: &crate::net::session::outbox::Outbox,
    player_id: crate::game::PlayerId,
) {
    apply_heal_command(state, tx, player_id, true);
}

pub fn apply_programmator_geology(
    state: &Arc<GameState>,
    tx: &crate::net::session::outbox::Outbox,
    player_id: crate::game::PlayerId,
) {
    apply_geology_command(state, tx, player_id, true);
}

fn apply_auto_dig_result(
    tx: &crate::net::session::outbox::Outbox,
    player_id: crate::game::PlayerId,
    result: crate::game::logic::settings::PlayerSettingMutation,
    action: &'static str,
) {
    match result {
        crate::game::logic::settings::PlayerSettingMutation::Changed(val) => {
            let packet = crate::protocol::packets::auto_digg(val);
            let _ = tx.send(crate::net::session::wire::make_u_packet_bytes(
                packet.0, &packet.1,
            ));
        }
        crate::game::logic::settings::PlayerSettingMutation::Unchanged => {}
        crate::game::logic::settings::PlayerSettingMutation::MissingState(component) => {
            tracing::error!(
                player_id = %player_id,
                component,
                action,
                "Player component missing for auto-dig"
            );
            send_settings_state_error(tx);
        }
        crate::game::logic::settings::PlayerSettingMutation::MissingEntity => {
            tracing::error!(player_id = %player_id, action, "Player entity missing for auto-dig");
            send_settings_state_error(tx);
        }
    }
}

fn apply_aggression_result(
    tx: &crate::net::session::outbox::Outbox,
    player_id: crate::game::PlayerId,
    result: crate::game::logic::settings::PlayerSettingMutation,
    action: &'static str,
) {
    match result {
        crate::game::logic::settings::PlayerSettingMutation::Changed(val) => {
            let packet = crate::protocol::packets::aggression(val);
            let _ = tx.send(crate::net::session::wire::make_u_packet_bytes(
                packet.0, &packet.1,
            ));
        }
        crate::game::logic::settings::PlayerSettingMutation::Unchanged => {}
        crate::game::logic::settings::PlayerSettingMutation::MissingState(component) => {
            tracing::error!(
                player_id = %player_id,
                component,
                action,
                "Player component missing for aggression"
            );
            send_settings_state_error(tx);
        }
        crate::game::logic::settings::PlayerSettingMutation::MissingEntity => {
            tracing::error!(player_id = %player_id, action, "Player entity missing for aggression");
            send_settings_state_error(tx);
        }
    }
}

fn send_settings_state_error(tx: &crate::net::session::outbox::Outbox) {
    let packet =
        crate::protocol::packets::ok_message("НАСТРОЙКИ", "Состояние настроек недоступно.");
    let _ = tx.send(crate::net::session::wire::make_u_packet_bytes(
        packet.0, &packet.1,
    ));
}

fn apply_inventory_result(
    tx: &crate::net::session::outbox::Outbox,
    player_id: crate::game::PlayerId,
    result: crate::game::logic::inventory::InventoryMutation,
    action: &'static str,
) {
    match result {
        crate::game::logic::inventory::InventoryMutation::Packets(packets) => {
            for (event, payload) in packets {
                let _ = tx.send(crate::net::session::wire::make_u_packet_bytes(
                    event, &payload,
                ));
            }
        }
        crate::game::logic::inventory::InventoryMutation::MissingState(component) => {
            tracing::error!(
                player_id = %player_id,
                component,
                action,
                "Player component missing for inventory"
            );
            send_inventory_state_error(tx);
        }
        crate::game::logic::inventory::InventoryMutation::MissingEntity => {
            tracing::error!(player_id = %player_id, action, "Player entity missing for inventory");
            send_inventory_state_error(tx);
        }
        crate::game::logic::inventory::InventoryMutation::RejectedPayload => {
            tracing::warn!(player_id = %player_id, action, "Rejected malformed inventory payload");
        }
    }
}

fn send_inventory_state_error(tx: &crate::net::session::outbox::Outbox) {
    let packet =
        crate::protocol::packets::ok_message("ИНВЕНТАРЬ", "Состояние инвентаря недоступно.");
    let _ = tx.send(crate::net::session::wire::make_u_packet_bytes(
        packet.0, &packet.1,
    ));
}

fn handle_known_noop_ty(
    tx: &crate::net::session::outbox::Outbox,
    player_id: crate::game::PlayerId,
    event: &str,
    payload: &[u8],
) {
    match event {
        "Xhur" => {
            if is_unit_payload(payload) {
                tracing::debug!(pid = %player_id, "known no-op TY event: self-hurt");
            } else {
                tracing::warn!(pid = %player_id, payload = ?payload, "invalid Xhur payload");
            }
        }
        "FINV" => {
            if let Some(index) = decode_finv_index(payload) {
                tracing::debug!(pid = %player_id, index, "known no-op TY event: inventory filter hotkey");
            } else {
                tracing::warn!(pid = %player_id, payload = ?payload, "invalid FINV payload");
            }
        }
        "Help" => {
            crate::net::session::social::commands::send_ok(
                tx,
                "Справка",
                "Справка пока не подключена на сервере.",
            );
        }
        "Miso" => {
            let (event, payload) = crate::protocol::packets::mission_panel("", 0, 0, 0, "");
            crate::net::session::wire::send_u_packet(tx, event, &payload);
        }
        "THID" => {
            let marker = String::from_utf8_lossy(payload);
            tracing::debug!(pid = %player_id, marker = %marker, "tutorial marker hidden");
        }
        "Miss" => {
            if let Some(enabled) = decode_miss_enabled(payload) {
                tracing::debug!(pid = %player_id, enabled, "known no-op TY event: mission init");
            } else {
                tracing::warn!(pid = %player_id, payload = ?payload, "invalid Miss payload");
            }
        }
        "Rndm" => {
            if let Some(hash) = decode_rndm_hash(payload) {
                tracing::debug!(pid = %player_id, hash_len = hash.len(), "known no-op TY event: device hash");
            } else {
                tracing::warn!(pid = %player_id, payload = ?payload, "invalid Rndm payload");
            }
        }
        "TAUR" => {
            if is_unit_payload(payload) {
                tracing::debug!(pid = %player_id, "known no-op TY event: auto-respawn toggle");
            } else {
                tracing::warn!(pid = %player_id, payload = ?payload, "invalid TAUR payload");
            }
        }
        _ => {
            tracing::warn!(pid = %player_id, event, "unknown no-op TY command");
        }
    }
}

fn spawn_session_async_task<F>(state: &Arc<GameState>, name: &'static str, task: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let threshold = std::time::Duration::from_millis(
        state.config.gameplay.schedules.schedule_warn_threshold_ms,
    );
    let state_for_guard = state.clone();
    state.db_pending_tasks.fetch_add(1, Ordering::SeqCst);
    state.tokio_handle.spawn(async move {
        let _guard = PendingTaskGuard {
            state: state_for_guard,
        };
        let t0 = Instant::now();
        task.await;
        let elapsed = t0.elapsed();
        if elapsed > threshold {
            tracing::warn!(
                target: "tickprof",
                command = name,
                elapsed = ?elapsed,
                threshold = ?threshold,
                "SLOW async session command"
            );
        }
    });
}

fn apply_bonus_claim(
    state: &Arc<GameState>,
    tx: &crate::net::session::outbox::Outbox,
    player_id: crate::game::PlayerId,
) {
    match crate::game::logic::bonus::claim_bonus(state, player_id) {
        crate::game::logic::bonus::BonusClaim::Claimed {
            money: new_money,
            creds,
            reward_money,
            cooldown_hours,
            row,
        } => {
            crate::net::session::wire::send_u_packet(
                tx,
                "P$",
                &crate::protocol::packets::money(new_money, creds).1,
            );
            crate::net::session::wire::send_u_packet(tx, "DR", b"0");
            crate::net::session::social::commands::send_ok(
                tx,
                "Бонус",
                &format!(
                    "Вы получили {reward_money}$!\nВозвращайтесь через {cooldown_hours} часов."
                ),
            );

            let db = state.db.clone();
            let task_state = state.clone();
            spawn_session_async_task(state, "bonus_save", async move {
                if let Err(e) = db.save_player(&row).await {
                    tracing::error!(player_id = %player_id, error = ?e, "Failed to write-through save player after daily bonus");
                } else {
                    crate::game::logic::bonus::mark_bonus_saved(&task_state, player_id);
                }
            });
        }
        crate::game::logic::bonus::BonusClaim::NotReady { hours, minutes } => {
            crate::net::session::social::commands::send_ok(
                tx,
                "Бонус",
                &format!("Бонус ещё не готов.\nПриходите через {hours}ч {minutes}м."),
            );
        }
        crate::game::logic::bonus::BonusClaim::MissingState => {
            crate::net::session::wire::send_u_packet(
                tx,
                "OK",
                &crate::protocol::packets::ok_message("Бонус", "Состояние бонуса недоступно.").1,
            );
        }
    }
}

fn apply_geology_command(
    state: &Arc<GameState>,
    tx: &crate::net::session::outbox::Outbox,
    player_id: crate::game::PlayerId,
    programmatic: bool,
) {
    match crate::game::logic::geology::apply_geology(state, player_id, programmatic) {
        crate::game::logic::geology::GeologyResult::Applied {
            geo_name,
            changed_cells,
        } => {
            for (x, y) in changed_cells {
                crate::game::broadcast_cell_update(state, x, y);
            }
            crate::net::session::wire::send_u_packet(
                tx,
                "GE",
                &crate::protocol::packets::geo(&geo_name).1,
            );
        }
        crate::game::logic::geology::GeologyResult::MissingState(_)
        | crate::game::logic::geology::GeologyResult::MissingEntity => {
            crate::net::session::wire::send_u_packet(
                tx,
                "OK",
                &crate::protocol::packets::ok_message("ГЕОЛОГИЯ", "Состояние игрока недоступно.").1,
            );
        }
        crate::game::logic::geology::GeologyResult::SilentNoop => {}
    }
}

fn apply_heal_command(
    state: &Arc<GameState>,
    tx: &crate::net::session::outbox::Outbox,
    player_id: crate::game::PlayerId,
    programmatic: bool,
) {
    match crate::game::logic::healing::apply_heal(state, player_id, programmatic) {
        crate::game::logic::healing::HealResult::Applied {
            health,
            max_health,
            crystals,
            x,
            y,
            skill_packet,
        } => {
            crate::net::session::wire::send_u_packet(
                tx,
                "@L",
                &crate::protocol::packets::health(health, max_health).1,
            );
            crate::net::session::wire::send_u_packet(
                tx,
                "@B",
                &crate::protocol::packets::basket(&crystals, 1).1,
            );
            if let Some(packet) = skill_packet {
                crate::net::session::wire::send_u_packet(tx, packet.0, &packet.1);
            }
            let fx = crate::protocol::packets::hb_heal_fx(
                crate::net::session::util::net_u16_nonneg(player_id),
            );
            state.broadcast_hb_at(x, y, &[fx], None);
        }
        crate::game::logic::healing::HealResult::MissingState(_)
        | crate::game::logic::healing::HealResult::MissingEntity => {
            crate::net::session::wire::send_u_packet(
                tx,
                "OK",
                &crate::protocol::packets::ok_message("ЛЕЧЕНИЕ", "Состояние игрока недоступно.").1,
            );
        }
        crate::game::logic::healing::HealResult::SilentNoop => {}
    }
}

fn spawn_program_save_task(
    state: &Arc<GameState>,
    tx: crate::net::session::outbox::Outbox,
    player_id: crate::game::PlayerId,
    payload: bytes::Bytes,
) {
    let Some(session_id) = state.sessions.session_for_player(player_id) else {
        return;
    };
    let task_state = state.clone();
    spawn_session_async_task(state, "program_save", async move {
        let Some((program_id, source)) = decode_program_save(&tx, player_id, &payload) else {
            return;
        };

        if program_id <= 0 {
            tracing::warn!(
                player_id = %player_id,
                program_id,
                "PROG received no selected client program; opening program list"
            );
            crate::net::session::social::buildings::handle_programmator_pope_menu(
                &task_state,
                &tx,
                player_id,
            )
            .await;
            return;
        }

        if let Err(e) = task_state
            .db
            .save_program(player_id.into(), program_id, &source)
            .await
        {
            tracing::error!(player_id = %player_id, program_id, error = ?e, "DB save failed");
            crate::net::session::wire::send_u_packet(
                &tx,
                "OK",
                &crate::protocol::packets::ok_message(
                    "ПРОГРАММАТОР",
                    "Не удалось сохранить программу.",
                )
                .1,
            );
            return;
        }
        if let Err(e) = task_state
            .db
            .set_selected_program(player_id.into(), Some(program_id))
            .await
        {
            tracing::error!(player_id = %player_id, program_id, error = ?e, "DB selected program update failed for PROG");
            crate::net::session::wire::send_u_packet(
                &tx,
                "OK",
                &crate::protocol::packets::ok_message(
                    "ПРОГРАММАТОР",
                    "Не удалось выбрать программу.",
                )
                .1,
            );
            return;
        }

        let program_name = match task_state.db.get_program(program_id).await {
            Ok(Some(program)) if program.player_id == player_id.as_i32() => program.name,
            Ok(Some(_) | None) => {
                tracing::error!(player_id = %player_id, program_id, "Saved program is missing after PROG save");
                crate::net::session::wire::send_u_packet(
                    &tx,
                    "OK",
                    &crate::protocol::packets::ok_message("ПРОГРАММАТОР", "Программа недоступна.")
                        .1,
                );
                return;
            }
            Err(e) => {
                tracing::error!(player_id = %player_id, program_id, error = ?e, "DB get failed after PROG save");
                crate::net::session::wire::send_u_packet(
                    &tx,
                    "OK",
                    &crate::protocol::packets::ok_message(
                        "ПРОГРАММАТОР",
                        "Не удалось прочитать программу.",
                    )
                    .1,
                );
                return;
            }
        };

        task_state.enqueue_command(crate::game::PlayerCommand::ApplySavedProgram {
            player_id,
            session_id,
            program_id,
            program_name,
            source,
        });
    });
}

fn decode_program_save(
    tx: &crate::net::session::outbox::Outbox,
    player_id: crate::game::PlayerId,
    payload: &[u8],
) -> Option<(i32, String)> {
    let decoded = crate::game::programmator::ProgrammatorState::decode_prog_packet(payload);
    if decoded.is_none() {
        tracing::warn!(
            player_id = %player_id,
            len = payload.len(),
            "PROGDIAG PROG decode FAILED"
        );
        crate::net::session::wire::send_u_packet(
            tx,
            "@P",
            &crate::protocol::packets::programmator_status(false).1,
        );
        crate::net::session::wire::send_u_packet(
            tx,
            "OK",
            &crate::protocol::packets::ok_message(
                "ПРОГРАММАТОР",
                "Не удалось прочитать программу.",
            )
            .1,
        );
    }
    decoded
}

fn spawn_inventory_building_insert_task(
    state: &Arc<GameState>,
    tx: crate::net::session::outbox::Outbox,
    placement: crate::game::logic::contracts::InventoryBuildingPlacement,
) {
    let Some(session_id) = state.sessions.session_for_player(placement.owner_id) else {
        return;
    };
    let task_state = state.clone();
    spawn_session_async_task(state, "inventory_building_insert", async move {
        let inserted = task_state
            .db
            .insert_building(
                &placement.type_code,
                placement.x,
                placement.y,
                placement.owner_id.into(),
                placement.clan_id,
                &placement.extra,
            )
            .await;
        match inserted {
            Ok(db_id) => {
                task_state.enqueue_command(
                    crate::game::PlayerCommand::ApplyInventoryBuildingPlaced {
                        session_id,
                        placement,
                        db_id,
                    },
                );
            }
            Err(e) => {
                tracing::error!(
                    player_id = %placement.owner_id,
                    x = placement.x,
                    y = placement.y,
                    pack_type = ?placement.pack_type,
                    error = ?e,
                    "DB insert failed for inventory building placement"
                );
                crate::net::session::wire::send_u_packet(
                    &tx,
                    "OK",
                    &crate::protocol::packets::ok_message("Ошибка", "Ошибка БД").1,
                );
            }
        }
    });
}

fn spawn_paid_building_insert_task(
    state: &Arc<GameState>,
    _tx: crate::net::session::outbox::Outbox,
    placement: crate::game::logic::contracts::PaidBuildingPlacement,
) {
    let Some(session_id) = state.sessions.session_for_player(placement.owner_id) else {
        return;
    };
    let task_state = state.clone();
    spawn_session_async_task(state, "paid_building_insert", async move {
        let inserted = task_state
            .db
            .insert_building(
                &placement.type_code,
                placement.x,
                placement.y,
                placement.owner_id.into(),
                placement.building_clan_id,
                &placement.extra,
            )
            .await;
        match inserted {
            Ok(db_id) => {
                task_state.enqueue_command(crate::game::PlayerCommand::ApplyPaidBuildingPlaced {
                    session_id,
                    placement,
                    db_id,
                });
            }
            Err(e) => {
                tracing::error!(
                    player_id = %placement.owner_id,
                    x = placement.x,
                    y = placement.y,
                    pack_type = ?placement.pack_type,
                    error = ?e,
                    "DB insert failed for paid building placement"
                );
                task_state.enqueue_command(
                    crate::game::PlayerCommand::RefundPaidBuildingPlacement {
                        session_id,
                        player_id: placement.owner_id,
                        cost: placement.cost,
                    },
                );
            }
        }
    });
}

fn parse_pack_remove_button(button: &str) -> Option<(i32, i32)> {
    let rest = button.strip_prefix("pack_op:remove:")?;
    let mut parts = rest.split(':');
    let x = parts.next()?.parse::<i32>().ok()?;
    let y = parts.next()?.parse::<i32>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((x, y))
}

fn spawn_building_remove_task(
    state: &Arc<GameState>,
    tx: crate::net::session::outbox::Outbox,
    removal: crate::game::logic::contracts::BuildingRemoval,
) {
    let task_state = state.clone();
    spawn_session_async_task(state, "building_remove", async move {
        if crate::net::session::social::buildings::delete_destroyed_building_db(
            &task_state,
            &removal.view,
        )
        .await
        {
            task_state
                .enqueue_command(crate::game::PlayerCommand::ApplyRemovedBuilding { removal });
        } else {
            crate::net::session::wire::send_u_packet(
                &tx,
                "OK",
                &crate::protocol::packets::ok_message("Ошибка", "Ошибка БД").1,
            );
        }
    });
}

fn spawn_program_editor_open_task(
    state: &Arc<GameState>,
    tx: crate::net::session::outbox::Outbox,
    player_id: crate::game::PlayerId,
    program_id: i32,
) {
    let Some(session_id) = state.sessions.session_for_player(player_id) else {
        return;
    };
    let task_state = state.clone();
    spawn_session_async_task(state, "program_editor_open", async move {
        let program = match task_state.db.get_program(program_id).await {
            Ok(Some(program)) => program,
            Ok(None) => {
                crate::net::session::ui::gui_buttons::send_programmator_action_error(
                    &tx,
                    "Программа не найдена.",
                );
                return;
            }
            Err(e) => {
                tracing::error!(player_id = %player_id, program_id, error = ?e, "DB get failed for openprog");
                crate::net::session::ui::gui_buttons::send_programmator_action_error(
                    &tx,
                    "Не удалось прочитать программу.",
                );
                return;
            }
        };
        if program.player_id != player_id.as_i32() {
            tracing::warn!(
                player_id = %player_id,
                program_id,
                owner_id = program.player_id,
                "Rejected foreign program open"
            );
            crate::net::session::ui::gui_buttons::send_programmator_action_error(
                &tx,
                "Программа недоступна.",
            );
            return;
        }
        if let Err(e) = task_state
            .db
            .set_selected_program(player_id.into(), Some(program.id))
            .await
        {
            tracing::error!(player_id = %player_id, program_id = program.id, error = ?e, "DB selected program update failed for openprog");
            crate::net::session::ui::gui_buttons::send_programmator_action_error(
                &tx,
                "Не удалось выбрать программу.",
            );
            return;
        }
        task_state.enqueue_command(crate::game::PlayerCommand::ApplyProgramEditorOpen {
            session_id,
            player_id,
            program_id: program.id,
            program_name: program.name,
            source: program.code,
        });
    });
}

fn spawn_program_editor_create_task(
    state: &Arc<GameState>,
    tx: crate::net::session::outbox::Outbox,
    player_id: crate::game::PlayerId,
    name: &str,
) {
    let name = name.trim().to_owned();
    if name.is_empty() {
        return;
    }
    let Some(session_id) = state.sessions.session_for_player(player_id) else {
        return;
    };
    let task_state = state.clone();
    spawn_session_async_task(state, "program_editor_create", async move {
        let program_id = match task_state
            .db
            .insert_program(player_id.into(), &name, "")
            .await
        {
            Ok(program_id) => program_id,
            Err(e) => {
                tracing::error!(player_id = %player_id, error = ?e, "DB insert failed for createprog");
                crate::net::session::ui::gui_buttons::send_programmator_action_error(
                    &tx,
                    "Не удалось создать программу.",
                );
                return;
            }
        };
        if let Err(e) = task_state
            .db
            .set_selected_program(player_id.into(), Some(program_id))
            .await
        {
            tracing::error!(player_id = %player_id, program_id, error = ?e, "DB selected program update failed for createprog");
            crate::net::session::ui::gui_buttons::send_programmator_action_error(
                &tx,
                "Не удалось выбрать программу.",
            );
            return;
        }
        task_state.enqueue_command(crate::game::PlayerCommand::ApplyProgramEditorOpen {
            session_id,
            player_id,
            program_id,
            program_name: name,
            source: String::new(),
        });
    });
}

fn spawn_program_editor_rename_task(
    state: &Arc<GameState>,
    tx: crate::net::session::outbox::Outbox,
    player_id: crate::game::PlayerId,
    program_id: i32,
    name: &str,
) {
    let name = name.trim().to_owned();
    if name.is_empty() {
        return;
    }
    let Some(session_id) = state.sessions.session_for_player(player_id) else {
        return;
    };
    let task_state = state.clone();
    spawn_session_async_task(state, "program_editor_rename", async move {
        let program = match task_state.db.get_program(program_id).await {
            Ok(Some(program)) => program,
            Ok(None) => {
                crate::net::session::ui::gui_buttons::send_programmator_action_error(
                    &tx,
                    "Программа не найдена.",
                );
                return;
            }
            Err(e) => {
                tracing::error!(player_id = %player_id, program_id, error = ?e, "DB get failed for rename program");
                crate::net::session::ui::gui_buttons::send_programmator_action_error(
                    &tx,
                    "Не удалось прочитать программу.",
                );
                return;
            }
        };
        if program.player_id != player_id.as_i32() {
            tracing::warn!(
                player_id = %player_id,
                program_id,
                owner_id = program.player_id,
                "Rejected foreign program rename"
            );
            crate::net::session::ui::gui_buttons::send_programmator_action_error(
                &tx,
                "Программа недоступна.",
            );
            return;
        }
        if let Err(e) = task_state.db.rename_program(program_id, &name).await {
            tracing::error!(player_id = %player_id, program_id, error = ?e, "DB rename failed for program");
            crate::net::session::ui::gui_buttons::send_programmator_action_error(
                &tx,
                "Не удалось переименовать программу.",
            );
            return;
        }
        task_state.enqueue_command(crate::game::PlayerCommand::ApplyProgramEditorRename {
            session_id,
            player_id,
            program_id,
            program_name: name,
            source: program.code,
        });
    });
}

fn spawn_program_delete_task(
    state: &Arc<GameState>,
    tx: crate::net::session::outbox::Outbox,
    player_id: crate::game::PlayerId,
    payload: bytes::Bytes,
) {
    let task_state = state.clone();
    spawn_session_async_task(state, "program_delete", async move {
        let program_id = std::str::from_utf8(&payload)
            .ok()
            .and_then(|s| s.trim().parse::<i32>().ok());
        let Some(program_id) = program_id else {
            return;
        };

        match task_state
            .db
            .delete_program_owned(player_id.into(), program_id)
            .await
        {
            Ok(true) => {
                task_state.enqueue_command(crate::game::PlayerCommand::ApplyDeletedProgram {
                    player_id,
                    program_id,
                });
            }
            Ok(false) => {
                tracing::warn!(
                    player_id = %player_id,
                    program_id,
                    "Program delete rejected: missing or foreign row"
                );
                crate::net::session::wire::send_u_packet(
                    &tx,
                    "OK",
                    &crate::protocol::packets::ok_message("ПРОГРАММАТОР", "Программа не найдена.")
                        .1,
                );
            }
            Err(e) => {
                tracing::error!(player_id = %player_id, program_id, error = ?e, "DB delete failed");
                crate::net::session::wire::send_u_packet(
                    &tx,
                    "OK",
                    &crate::protocol::packets::ok_message(
                        "ПРОГРАММАТОР",
                        "Не удалось удалить программу.",
                    )
                    .1,
                );
            }
        }
    });
}

fn decode_finv_index(payload: &[u8]) -> Option<u8> {
    match payload {
        [b'0'..=b'9'] => Some(payload[0] - b'0'),
        _ => None,
    }
}

fn is_unit_payload(payload: &[u8]) -> bool {
    payload == b"_"
}

fn decode_miss_enabled(payload: &[u8]) -> Option<bool> {
    match payload {
        b"0" => Some(false),
        b"1" => Some(true),
        _ => None,
    }
}

fn decode_rndm_hash(payload: &[u8]) -> Option<&str> {
    const PREFIX: &[u8] = b"hash=";
    let hash = payload.strip_prefix(PREFIX)?;
    std::str::from_utf8(hash).ok()
}

fn parse_program_rename_button(button: &str) -> Option<(i32, String)> {
    let rest = button.strip_prefix("rename:")?;
    let (id, name) = rest.split_once(':')?;
    Some((id.parse().ok()?, name.to_owned()))
}
