#![allow(
    clippy::too_many_lines,
    clippy::needless_pass_by_value,
    clippy::option_if_let_else,
    clippy::assigning_clones,
    clippy::items_after_statements,
    clippy::used_underscore_binding,
    clippy::semicolon_if_nothing_returned,
    clippy::missing_panics_doc
)]
//! Application of ordered player commands inside the simulation boundary.
//!
//! This module is intentionally still calling legacy session handlers while the
//! kernel migration is in progress. The important boundary is that lifecycle
//! drains commands and this module owns command application.

mod bonus_commands;
mod building_tasks;
mod chat_commands;
pub(super) mod completion;
pub(super) mod completion_clan;
mod gameplay_commands;
pub(super) mod gui;
mod gui_tasks;
mod inventory_commands;
mod known_noop_commands;
mod market_commands;
mod parsing;
mod session_commands;
pub(super) mod slash;

use bonus_commands::apply_bonus_claim;
pub(super) use building_tasks::{
    spawn_inventory_building_insert_task, spawn_paid_building_insert_task,
};
use chat_commands::apply_chat_command;
use gameplay_commands::apply_gameplay_command;
pub(super) use gui_tasks::spawn_gui_async_task;
use inventory_commands::apply_inventory_command;
use known_noop_commands::apply_known_noop_ty;
pub(super) use market_commands::{
    apply_market_buy, apply_market_get_profit, apply_market_sell, apply_market_sell_all,
};
use session_commands::apply_session_command;

use parsing::{decode_program_save, parse_pack_remove_button, parse_program_rename_button};

pub use completion::apply_persistence_completion;

use crate::game::logic::kernel_context::KernelContext;
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

#[cfg(test)]
pub fn apply_player_command(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    command: PlayerCommand,
) -> CommandEffects {
    let mut due_actions = crate::game::logic::due::DueActionQueue::new(
        state.config.gameplay.simulation.due_action_capacity,
    );
    apply_player_command_with_due(state, player_id, session_id, command, &mut due_actions)
}

#[cfg(test)]
pub fn apply_player_command_with_due(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    command: PlayerCommand,
    due_actions: &mut crate::game::logic::due::DueActionQueue,
) -> CommandEffects {
    apply_queued_player_command_with_due(
        state,
        player_id,
        session_id,
        command,
        crate::game::CommandSeq::new(0),
        due_actions,
    )
}

pub fn apply_queued_player_command_with_due(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    command: PlayerCommand,
    sequence: crate::game::CommandSeq,
    due_actions: &mut crate::game::logic::due::DueActionQueue,
) -> CommandEffects {
    match command {
        command @ (PlayerCommand::Connect { .. }
        | PlayerCommand::Disconnect
        | PlayerCommand::Move { .. }) => {
            apply_session_command(state, player_id, session_id, command)
        }
        command @ (PlayerCommand::Dig { .. }
        | PlayerCommand::Build { .. }
        | PlayerCommand::Geology { .. }
        | PlayerCommand::Heal { .. }
        | PlayerCommand::Respawn) => apply_gameplay_command(state, player_id, session_id, command),
        PlayerCommand::OpenBox => apply_open_box_command(state, player_id, session_id),
        PlayerCommand::ClaimBonus => apply_bonus_claim(state, player_id, session_id),
        command @ (PlayerCommand::InventoryToggle
        | PlayerCommand::InventoryChoose { .. }
        | PlayerCommand::InventoryUse
        | PlayerCommand::ToggleAutoDig
        | PlayerCommand::ToggleAggression
        | PlayerCommand::SettingsSave { .. }) => {
            apply_inventory_command(state, player_id, session_id, command, due_actions)
        }
        PlayerCommand::Gui { command } => {
            gui::apply_gui_command(state, session_id, player_id, command)
        }
        PlayerCommand::AdminAction => gui::apply_presentation_command(
            state,
            session_id,
            player_id,
            &PlayerCommand::AdminAction,
        ),
        PlayerCommand::OpenProgrammer => CommandEffects {
            events: Vec::new(),
            saves: vec![crate::game::SaveCommand::ProgramMenu {
                request: crate::game::ProgramMenuRequest {
                    player_id,
                    session_id,
                },
            }],
            broadcasts: Vec::new(),
        },
        PlayerCommand::RequestMyBuildings => CommandEffects {
            events: Vec::new(),
            saves: vec![crate::game::SaveCommand::BuildingMenu {
                request: crate::game::BuildingMenuRequest {
                    player_id,
                    session_id,
                },
            }],
            broadcasts: Vec::new(),
        },
        PlayerCommand::OpenClan => CommandEffects {
            events: Vec::new(),
            saves: vec![crate::game::SaveCommand::ClanMenu {
                request: crate::game::ClanMenuRequest {
                    player_id,
                    session_id,
                    player_clan_id: state.query_player_opt(player_id, |ecs, entity| {
                        ecs.get::<crate::game::player::PlayerStats>(entity)
                            .and_then(|stats| stats.clan_id)
                    }),
                    action: crate::game::ClanMenuAction::Main,
                    invite_candidates: Vec::new(),
                },
            }],
            broadcasts: Vec::new(),
        },
        command @ (PlayerCommand::LocalChat { .. }
        | PlayerCommand::ChannelChat { .. }
        | PlayerCommand::ChatResync { .. }
        | PlayerCommand::ChatMenu { .. }
        | PlayerCommand::ChatChoose { .. }
        | PlayerCommand::ChatSettings { .. }
        | PlayerCommand::ChatPrivate { .. }
        | PlayerCommand::Whois { .. }) => apply_chat_command(state, player_id, session_id, command),
        command @ (PlayerCommand::ProgramAction { .. }
        | PlayerCommand::ApplyDeletedProgram { .. }
        | PlayerCommand::ApplyProgramEditorOpen { .. }
        | PlayerCommand::ApplyProgramEditorRename { .. }) => {
            apply_program_command(state, player_id, session_id, command)
        }
        command @ (PlayerCommand::ApplyInventoryBuildingPlaced { .. }
        | PlayerCommand::InventoryBuildingPlacementFailed
        | PlayerCommand::ApplyPaidBuildingPlaced { .. }
        | PlayerCommand::RefundPaidBuildingPlacement { .. }) => {
            completion::apply_building_completion(state, player_id, session_id, command)
        }
        PlayerCommand::RemovePack { remove } => {
            completion::apply_remove_pack(state, remove, sequence)
        }
        PlayerCommand::MarketSell {
            sliders,
            building_x,
            building_y,
        } => apply_market_sell(
            state, player_id, session_id, &sliders, building_x, building_y,
        ),
        PlayerCommand::MarketSellAll {
            building_x,
            building_y,
        } => apply_market_sell_all(state, player_id, session_id, building_x, building_y),
        PlayerCommand::MarketBuy {
            sliders,
            building_x,
            building_y,
        } => apply_market_buy(
            state, player_id, session_id, &sliders, building_x, building_y,
        ),
        PlayerCommand::MarketGetProfit {
            building_x,
            building_y,
        } => apply_market_get_profit(state, player_id, session_id, building_x, building_y),
        PlayerCommand::KnownNoopTy { event, payload } => {
            apply_known_noop_ty(state, session_id, player_id, &event, &payload)
        }
        PlayerCommand::Slash { command } => {
            let context = crate::game::logic::kernel_context::KernelContext::new(state);
            slash::apply_slash_command(&context, player_id, session_id, command)
        }
    }
}

fn apply_open_box_command(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
) -> CommandEffects {
    let batch = crate::net::session::wire::PacketBatch::default();
    let Some(payload) = crate::game::logic::buildings::prepare_dpbx_crystal_box(state, player_id)
    else {
        return CommandEffects::default();
    };
    crate::net::session::wire::send_u_packet(&batch, "GU", &payload);
    state.modify_player(player_id, |ecs, entity| {
        if let Some(mut ui) = ecs.get_mut::<crate::game::player::PlayerUI>(entity) {
            ui.current_window = Some("open_box".to_string());
        }
        Some(())
    });
    CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: batch.into_packets(),
        }],
        ..CommandEffects::default()
    }
}

fn apply_local_chat_command(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    message: String,
) -> CommandEffects {
    let batch = crate::net::session::wire::PacketBatch::default();
    if !state.check_chat_rate(player_id) {
        tracing::debug!(player_id = %player_id, "chat rate limited (Locl)");
        return CommandEffects::default();
    }
    if crate::game::logic::chat::handle_local_chat_non_command(state, &batch, player_id, &message) {
        let packets = batch.into_packets();
        return if packets.is_empty() {
            CommandEffects::default()
        } else {
            CommandEffects {
                events: vec![crate::game::GameEvent::SessionBatch {
                    session_id,
                    player_id,
                    packets,
                }],
                ..CommandEffects::default()
            }
        };
    }
    let command = crate::game::logic::commands_social::parse_slash_command(message.trim());
    slash::apply_slash_command(
        &crate::game::logic::kernel_context::KernelContext::new(state),
        player_id,
        session_id,
        command,
    )
}

#[allow(clippy::needless_pass_by_value)]
fn apply_channel_chat_command(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    payload: bytes::Bytes,
) -> CommandEffects {
    let mut effects = CommandEffects::default();
    let batch = crate::net::session::wire::PacketBatch::default();
    if !state.check_chat_rate(player_id) {
        tracing::debug!(player_id = %player_id, "chat rate limited (Chat)");
        return effects;
    }
    let text = crate::game::logic::chat::extract_channel_message_text(&payload);
    if text.trim().starts_with('/') {
        let command = crate::game::logic::commands_social::parse_slash_command(text.trim());
        return slash::apply_slash_command(
            &crate::game::logic::kernel_context::KernelContext::new(state),
            player_id,
            session_id,
            command,
        );
    }

    if let Some(prepared) =
        crate::game::logic::chat::prepare_channel_chat_non_command(state, &batch, player_id, &text)
    {
        let msg_id = state.next_chat_id();
        let msg = openmines_protocol::chat::ChatMessage {
            id: msg_id,
            time: prepared.time,
            clan_id: prepared.clan_id,
            user_id: prepared.user_id,
            nickname: prepared.nickname.clone(),
            text: prepared.text.clone(),
            color: prepared.color,
        };

        effects.saves.push(crate::game::SaveCommand::ChatAppend {
            request: crate::game::ChatAppendRequest {
                id: msg_id,
                tag: prepared.db_tag,
                nickname: prepared.nickname,
                text: prepared.text,
                player_id: prepared.user_id,
                color: prepared.color,
            },
        });
        effects.events.push(crate::game::GameEvent::ChatFanout {
            route: prepared.route,
            message: msg,
        });
    }
    let packets = batch.into_packets();
    if !packets.is_empty() {
        effects.events.push(crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets,
        });
    }
    effects
}

fn apply_program_command(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    command: PlayerCommand,
) -> CommandEffects {
    let mut effects = CommandEffects::default();
    match command {
        crate::game::PlayerCommand::ProgramAction { event, payload } => {
            if state.sessions.session_for_player(player_id) == Some(session_id) {
                match event.as_str() {
                    "PROG" => match decode_program_save(&payload) {
                        None => {
                            tracing::warn!(
                                player_id = %player_id,
                                len = payload.len(),
                                "PROGDIAG PROG decode FAILED"
                            );
                            let batch = crate::net::session::wire::PacketBatch::default();
                            crate::net::session::wire::send_u_packet(
                                &batch,
                                "@P",
                                &crate::protocol::packets::programmator_status(false).1,
                            );
                            crate::net::session::wire::send_u_packet(
                                &batch,
                                "OK",
                                &crate::protocol::packets::ok_message(
                                    "ПРОГРАММАТОР",
                                    "Не удалось прочитать программу.",
                                )
                                .1,
                            );
                            effects.events.push(crate::game::GameEvent::SessionBatch {
                                session_id,
                                player_id,
                                packets: batch.into_packets(),
                            });
                        }
                        Some((program_id, _)) if program_id <= 0 => {
                            tracing::warn!(
                                player_id = %player_id,
                                program_id,
                                "PROG received no selected client program; opening program list"
                            );
                            effects.saves.push(crate::game::SaveCommand::ProgramMenu {
                                request: crate::game::ProgramMenuRequest {
                                    player_id,
                                    session_id,
                                },
                            });
                        }
                        Some((program_id, source)) => {
                            effects.saves.push(crate::game::SaveCommand::Program {
                                request: crate::game::ProgramSaveRequest {
                                    player_id,
                                    session_id,
                                    program_id,
                                    source,
                                },
                            });
                        }
                    },
                    "PDEL" => {
                        let Some(program_id) = std::str::from_utf8(&payload)
                            .ok()
                            .and_then(|raw| raw.trim().parse::<i32>().ok())
                            .filter(|program_id| *program_id > 0)
                        else {
                            return effects;
                        };
                        let clear_selected = state
                            .query_player_opt(player_id, |ecs, entity| {
                                Some(
                                    ecs.get::<crate::game::programmator::ProgrammatorState>(entity)
                                        .is_some_and(|program| {
                                            program.selected_id == Some(program_id)
                                        }),
                                )
                            })
                            .unwrap_or(false);
                        effects.saves.push(crate::game::SaveCommand::ProgramDelete {
                            request: crate::game::ProgramDeleteRequest {
                                player_id,
                                session_id,
                                program_id,
                                clear_selected,
                            },
                        });
                    }
                    "pRST" => effects.append(crate::game::logic::misc::apply_prog_reset_ty(
                        state, session_id, player_id,
                    )),
                    "PREN" => {
                        effects.append(crate::game::logic::misc::apply_prog_rename_prompt_ty(
                            state, session_id, player_id, &payload,
                        ))
                    }
                    "PCOP" => {
                        let Some(program) = std::str::from_utf8(&payload)
                            .ok()
                            .and_then(|raw| raw.trim().parse::<i32>().ok())
                            .filter(|program_id| *program_id > 0)
                        else {
                            return effects;
                        };
                        effects.saves.push(crate::game::SaveCommand::ProgramCopy {
                            request: crate::game::ProgramCopyRequest {
                                player: player_id,
                                session: session_id,
                                program,
                            },
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
        crate::game::PlayerCommand::ApplyDeletedProgram { program_id } => {
            crate::game::logic::misc::clear_deleted_program_runtime(state, player_id, program_id);
        }
        PlayerCommand::ApplyProgramEditorOpen { .. }
        | PlayerCommand::ApplyProgramEditorRename { .. } => {
            effects.append(completion::apply_program_editor_completion(
                state, session_id, player_id, command,
            ));
        }
        _ => unreachable!("non-program command routed to program command handler"),
    }
    effects
}

#[allow(clippy::too_many_lines)]
pub fn apply_programmator_auto_dig_set(
    state: &Arc<GameState>,
    tx: &dyn crate::net::session::wire::PacketSink,
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
    tx: &dyn crate::net::session::wire::PacketSink,
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
    tx: &dyn crate::net::session::wire::PacketSink,
    player_id: crate::game::PlayerId,
) {
    apply_heal_command(state, tx, player_id, true);
}

pub fn apply_programmator_geology(
    state: &Arc<GameState>,
    tx: &dyn crate::net::session::wire::PacketSink,
    player_id: crate::game::PlayerId,
) {
    apply_geology_command(state, tx, player_id, true);
}

fn apply_auto_dig_result(
    tx: &dyn crate::net::session::wire::PacketSink,
    player_id: crate::game::PlayerId,
    result: crate::game::logic::settings::PlayerSettingMutation,
    action: &'static str,
) {
    match result {
        crate::game::logic::settings::PlayerSettingMutation::Changed(val) => {
            let packet = crate::protocol::packets::auto_digg(val);
            tx.send_packet(crate::net::session::wire::make_u_packet_bytes(
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
    tx: &dyn crate::net::session::wire::PacketSink,
    player_id: crate::game::PlayerId,
    result: crate::game::logic::settings::PlayerSettingMutation,
    action: &'static str,
) {
    match result {
        crate::game::logic::settings::PlayerSettingMutation::Changed(val) => {
            let packet = crate::protocol::packets::aggression(val);
            tx.send_packet(crate::net::session::wire::make_u_packet_bytes(
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

fn send_settings_state_error(tx: &dyn crate::net::session::wire::PacketSink) {
    let packet =
        crate::protocol::packets::ok_message("НАСТРОЙКИ", "Состояние настроек недоступно.");
    tx.send_packet(crate::net::session::wire::make_u_packet_bytes(
        packet.0, &packet.1,
    ));
}

fn settings_open_effects(
    state: &Arc<GameState>,
    session_id: crate::game::SessionId,
    player_id: crate::game::PlayerId,
) -> CommandEffects {
    let batch = crate::net::session::wire::PacketBatch::default();
    crate::net::session::ui::settings::open(state, &batch, player_id);
    CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: batch.into_packets(),
        }],
        saves: Vec::new(),
        broadcasts: Vec::new(),
    }
}

fn setting_toggle_effects(
    session_id: crate::game::SessionId,
    player_id: crate::game::PlayerId,
    result: crate::game::logic::settings::PlayerSettingMutation,
    action: &'static str,
) -> CommandEffects {
    let batch = crate::net::session::wire::PacketBatch::default();
    match result {
        crate::game::logic::settings::PlayerSettingMutation::Changed(value) => {
            let packet = if action == "auto-dig" {
                crate::protocol::packets::auto_digg(value)
            } else {
                crate::protocol::packets::aggression(value)
            };
            crate::net::session::wire::send_u_packet(&batch, packet.0, &packet.1);
        }
        crate::game::logic::settings::PlayerSettingMutation::Unchanged => {}
        crate::game::logic::settings::PlayerSettingMutation::MissingState(component) => {
            tracing::error!(player_id = %player_id, component, action, "Player component missing for setting mutation");
            send_settings_state_error(&batch);
        }
        crate::game::logic::settings::PlayerSettingMutation::MissingEntity => {
            tracing::error!(player_id = %player_id, action, "Player entity missing for setting mutation");
            send_settings_state_error(&batch);
        }
    }
    CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: batch.into_packets(),
        }],
        saves: Vec::new(),
        broadcasts: Vec::new(),
    }
}

fn apply_inventory_result(
    session_id: crate::game::SessionId,
    player_id: crate::game::PlayerId,
    result: crate::game::logic::inventory::InventoryMutation,
    action: &'static str,
) -> CommandEffects {
    let mut packets = Vec::new();
    match result {
        crate::game::logic::inventory::InventoryMutation::Packets(mutation_packets) => {
            packets = mutation_packets
                .into_iter()
                .map(|(event, payload)| {
                    crate::net::session::wire::make_u_packet_bytes(event, &payload)
                })
                .collect();
        }
        crate::game::logic::inventory::InventoryMutation::MissingState(component) => {
            tracing::error!(
                player_id = %player_id,
                component,
                action,
                "Player component missing for inventory"
            );
            let packet = crate::protocol::packets::ok_message(
                "ИНВЕНТАРЬ",
                "Состояние инвентаря недоступно.",
            );
            packets.push(crate::net::session::wire::make_u_packet_bytes(
                packet.0, &packet.1,
            ));
        }
        crate::game::logic::inventory::InventoryMutation::MissingEntity => {
            tracing::error!(player_id = %player_id, action, "Player entity missing for inventory");
            let packet = crate::protocol::packets::ok_message(
                "ИНВЕНТАРЬ",
                "Состояние инвентаря недоступно.",
            );
            packets.push(crate::net::session::wire::make_u_packet_bytes(
                packet.0, &packet.1,
            ));
        }
        crate::game::logic::inventory::InventoryMutation::RejectedPayload => {
            tracing::warn!(player_id = %player_id, action, "Rejected malformed inventory payload");
        }
    }
    if packets.is_empty() {
        return CommandEffects::default();
    }
    CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets,
        }],
        ..CommandEffects::default()
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

fn apply_geology_command(
    state: &Arc<GameState>,
    tx: &dyn crate::net::session::wire::PacketSink,
    player_id: crate::game::PlayerId,
    programmatic: bool,
) {
    match crate::game::logic::geology::apply_geology(
        &crate::game::logic::kernel_context::KernelContext::new(state),
        player_id,
        programmatic,
    ) {
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
    tx: &dyn crate::net::session::wire::PacketSink,
    player_id: crate::game::PlayerId,
    programmatic: bool,
) {
    match crate::game::logic::healing::apply_heal(
        &crate::game::logic::kernel_context::KernelContext::new(state),
        player_id,
        programmatic,
    ) {
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

pub(super) fn apply_resp_bind(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    pack_x: i32,
    pack_y: i32,
) -> CommandEffects {
    let batch = crate::net::session::wire::PacketBatch::default();
    let Some(view) = state.get_pack_at(pack_x, pack_y) else {
        return CommandEffects::default();
    };
    if view.pack_type != crate::game::structures::buildings::PackType::Resp {
        return CommandEffects::default();
    }
    if !state
        .query_player_opt(player_id, |ecs, entity| {
            Some(
                crate::game::player::extract_player_row(ecs, entity).is_some()
                    && ecs.get::<crate::game::PlayerFlags>(entity).is_some(),
            )
        })
        .unwrap_or(false)
    {
        crate::game::logic::packs::send_resp_state_error(&batch);
        return CommandEffects {
            events: vec![crate::game::GameEvent::SessionBatch {
                session_id,
                player_id,
                packets: batch.into_packets(),
            }],
            ..CommandEffects::default()
        };
    }

    let updated = state
        .modify_player(player_id, |ecs, entity| {
            if ecs
                .get::<crate::game::player::PlayerMetadata>(entity)
                .is_none()
                || ecs
                    .get::<crate::game::player::PlayerFlags>(entity)
                    .is_none()
            {
                return Some(false);
            }
            {
                let mut meta = ecs
                    .get_mut::<crate::game::player::PlayerMetadata>(entity)
                    .expect("PlayerMetadata checked before resp bind");
                meta.resp_x = Some(pack_x);
                meta.resp_y = Some(pack_y);
            }
            ecs.get_mut::<crate::game::player::PlayerFlags>(entity)
                .expect("PlayerFlags checked before resp bind")
                .dirty = true;
            Some(true)
        })
        .flatten()
        .unwrap_or(false);
    if !updated {
        tracing::error!(player_id = %player_id, pack_x, pack_y, "Resp bind player state missing");
        crate::game::logic::packs::send_resp_state_error(&batch);
        return CommandEffects {
            events: vec![crate::game::GameEvent::SessionBatch {
                session_id,
                player_id,
                packets: batch.into_packets(),
            }],
            ..CommandEffects::default()
        };
    }

    // Re-open GUI to show "bound" state
    crate::game::logic::packs::open_resp_gui(state, &batch, player_id, &view);

    let mut effects = CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: batch.into_packets(),
        }],
        ..CommandEffects::default()
    };
    if let Some(entity) = state.get_player_entity(player_id)
        && let Some(row) = crate::game::player::extract_player_row(
            &state.ecs_read_profiled("commands.resp_bind_snapshot"),
            entity,
        )
    {
        effects
            .saves
            .push(crate::game::SaveCommand::Player { row: Box::new(row) });
    }
    effects
}

pub(super) fn apply_resp_fill(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    amount_str: &str,
    pack_x: i32,
    pack_y: i32,
) -> CommandEffects {
    apply_charge_fill(
        state,
        player_id,
        session_id,
        amount_str,
        (pack_x, pack_y),
        1,
        true,
    )
}

pub(super) fn apply_gun_fill(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    amount_str: &str,
    pack_x: i32,
    pack_y: i32,
) -> CommandEffects {
    apply_charge_fill(
        state,
        player_id,
        session_id,
        amount_str,
        (pack_x, pack_y),
        5,
        false,
    )
}

fn apply_charge_fill(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    amount_str: &str,
    (pack_x, pack_y): (i32, i32),
    crystal_index: usize,
    is_resp: bool,
) -> CommandEffects {
    let request = match amount_str {
        "100" => Some(100_i64),
        "1000" => Some(1000_i64),
        "max" => None,
        _ => return CommandEffects::default(),
    };
    let batch = crate::net::session::wire::PacketBatch::default();
    let Some(view) = state.get_pack_at(pack_x, pack_y) else {
        return CommandEffects::default();
    };
    let expected_type = if is_resp {
        crate::game::PackType::Resp
    } else {
        crate::game::PackType::Gun
    };
    if view.pack_type != expected_type {
        return CommandEffects::default();
    }
    let Some(player_entity) = state.get_player_entity(player_id) else {
        send_charge_fill_error(&batch, is_resp);
        return charge_fill_effects(batch, session_id, player_id, Vec::new(), Vec::new());
    };
    let Some(building_entity) = state.building_entity_at(pack_x, pack_y) else {
        send_charge_fill_error(&batch, is_resp);
        return charge_fill_effects(batch, session_id, player_id, Vec::new(), Vec::new());
    };

    let result = {
        let mut ecs = state.ecs_write_profiled("commands.charge_fill");
        if ecs.get::<crate::game::PlayerStats>(player_entity).is_none()
            || ecs.get::<crate::game::PlayerFlags>(player_entity).is_none()
            || ecs
                .get::<crate::game::structures::buildings::BuildingStats>(building_entity)
                .is_none()
            || ecs
                .get::<crate::game::structures::buildings::BuildingFlags>(building_entity)
                .is_none()
            || crate::game::player::extract_player_row(&ecs, player_entity).is_none()
            || crate::game::structures::buildings::extract_building_row(&ecs, building_entity)
                .is_none()
        {
            None
        } else {
            let (charge, max_charge) = {
                let building_stats = ecs
                    .get::<crate::game::structures::buildings::BuildingStats>(building_entity)
                    .expect("BuildingStats checked before charge fill");
                (building_stats.charge, building_stats.max_charge)
            };
            if charge >= max_charge {
                Some(None)
            } else {
                let requested =
                    request.unwrap_or_else(|| i64::from(max_charge.saturating_sub(charge)).max(0));
                let available = ecs
                    .get::<crate::game::PlayerStats>(player_entity)
                    .expect("PlayerStats checked before charge fill")
                    .crystals[crystal_index];
                let to_take = requested.min(available);
                if to_take <= 0 {
                    Some(None)
                } else {
                    let crystals = {
                        let mut player_stats = ecs
                            .get_mut::<crate::game::PlayerStats>(player_entity)
                            .expect("PlayerStats checked before charge fill");
                        player_stats.crystals[crystal_index] -= to_take;
                        player_stats.crystals
                    };
                    let increment = i32::try_from(to_take).unwrap_or(i32::MAX);
                    ecs.get_mut::<crate::game::structures::buildings::BuildingStats>(
                        building_entity,
                    )
                    .expect("BuildingStats checked before charge fill")
                    .charge = charge.saturating_add(increment).min(max_charge);
                    ecs.get_mut::<crate::game::PlayerFlags>(player_entity)
                        .expect("PlayerFlags checked before charge fill")
                        .dirty = true;
                    ecs.get_mut::<crate::game::structures::buildings::BuildingFlags>(
                        building_entity,
                    )
                    .expect("BuildingFlags checked before charge fill")
                    .dirty = true;
                    let incarnation = ecs
                        .get::<crate::game::PlayerFlags>(player_entity)
                        .expect("PlayerFlags checked before charge fill")
                        .incarnation;
                    ecs.resource_mut::<crate::game::DirtyPlayers>()
                        .0
                        .insert((player_entity, incarnation));
                    ecs.resource_mut::<crate::game::DirtyBuildings>()
                        .0
                        .insert(building_entity);
                    let player = crate::game::player::extract_player_row(&ecs, player_entity);
                    let building = crate::game::structures::buildings::extract_building_row(
                        &ecs,
                        building_entity,
                    );
                    drop(ecs);
                    Some(Some((crystals, player, building)))
                }
            }
        }
    };
    let (crystals, player, building) = match result {
        Some(Some((crystals, Some(player), Some(building)))) => (crystals, player, building),
        Some(None) => return CommandEffects::default(),
        Some(Some(_)) | None => {
            tracing::error!(player_id = %player_id, pack_x, pack_y, "Charge fill state missing");
            send_charge_fill_error(&batch, is_resp);
            return charge_fill_effects(batch, session_id, player_id, Vec::new(), Vec::new());
        }
    };

    crate::net::session::wire::send_u_packet(
        &batch,
        "@B",
        &crate::protocol::packets::basket(&crystals, 1).1,
    );
    if is_resp {
        crate::game::logic::packs::open_resp_admin_gui(state, &batch, player_id, pack_x, pack_y);
    } else {
        crate::game::logic::packs::open_gun_gui(state, &batch, player_id, pack_x, pack_y);
    }
    let broadcasts = if is_resp {
        Vec::new()
    } else {
        vec![crate::game::BroadcastEffect::BlockUpdate(
            crate::game::WorldPos(pack_x, pack_y),
        )]
    };
    charge_fill_effects(
        batch,
        session_id,
        player_id,
        vec![crate::game::SaveCommand::ChargeFill {
            player: Box::new(player),
            building: Box::new(building),
        }],
        broadcasts,
    )
}

fn send_charge_fill_error(batch: &crate::net::session::wire::PacketBatch, is_resp: bool) {
    if is_resp {
        crate::game::logic::packs::send_resp_state_error(batch);
    } else {
        crate::net::session::wire::send_u_packet(
            batch,
            "OK",
            &crate::protocol::packets::ok_message("Пушка", "Состояние пушки недоступно.").1,
        );
    }
}

fn charge_fill_effects(
    batch: crate::net::session::wire::PacketBatch,
    session_id: crate::game::SessionId,
    player_id: crate::game::PlayerId,
    saves: Vec<crate::game::SaveCommand>,
    broadcasts: Vec<crate::game::BroadcastEffect>,
) -> CommandEffects {
    CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: batch.into_packets(),
        }],
        saves,
        broadcasts,
    }
}

pub(super) fn apply_resp_profit(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    pack_x: i32,
    pack_y: i32,
) -> CommandEffects {
    let batch = crate::net::session::wire::PacketBatch::default();
    let Some(view) = state.get_pack_at(pack_x, pack_y) else {
        return CommandEffects::default();
    };
    if view.owner_id != player_id {
        return CommandEffects::default();
    }
    let Some(player_entity) = state.get_player_entity(player_id) else {
        crate::game::logic::packs::send_resp_state_error(&batch);
        return CommandEffects {
            events: vec![crate::game::GameEvent::SessionBatch {
                session_id,
                player_id,
                packets: batch.into_packets(),
            }],
            ..CommandEffects::default()
        };
    };
    let Some(building_entity) = state.building_entity_at(pack_x, pack_y) else {
        crate::game::logic::packs::send_resp_state_error(&batch);
        return CommandEffects {
            events: vec![crate::game::GameEvent::SessionBatch {
                session_id,
                player_id,
                packets: batch.into_packets(),
            }],
            ..CommandEffects::default()
        };
    };

    let result = {
        let mut ecs = state.ecs_write_profiled("commands.resp_profit");
        if ecs.get::<crate::game::PlayerStats>(player_entity).is_none()
            || ecs.get::<crate::game::PlayerFlags>(player_entity).is_none()
            || ecs
                .get::<crate::game::structures::buildings::BuildingStorage>(building_entity)
                .is_none()
            || ecs
                .get::<crate::game::structures::buildings::BuildingFlags>(building_entity)
                .is_none()
        {
            None
        } else {
            let amount = ecs
                .get::<crate::game::structures::buildings::BuildingStorage>(building_entity)
                .expect("BuildingStorage checked before resp profit")
                .money;
            ecs.get_mut::<crate::game::structures::buildings::BuildingStorage>(building_entity)
                .expect("BuildingStorage checked before resp profit")
                .money = 0;
            let (money_now, creds_now) = {
                let mut player_stats = ecs
                    .get_mut::<crate::game::PlayerStats>(player_entity)
                    .expect("PlayerStats checked before resp profit");
                player_stats.money = player_stats.money.saturating_add(amount);
                (player_stats.money, player_stats.creds)
            };
            if amount > 0 {
                ecs.get_mut::<crate::game::PlayerFlags>(player_entity)
                    .expect("PlayerFlags checked before resp profit")
                    .dirty = true;
                ecs.get_mut::<crate::game::structures::buildings::BuildingFlags>(building_entity)
                    .expect("BuildingFlags checked before resp profit")
                    .dirty = true;
                let incarnation = ecs
                    .get::<crate::game::PlayerFlags>(player_entity)
                    .expect("PlayerFlags checked before resp profit")
                    .incarnation;
                ecs.resource_mut::<crate::game::DirtyPlayers>()
                    .0
                    .insert((player_entity, incarnation));
            }
            let player = crate::game::player::extract_player_row(&ecs, player_entity);
            let building =
                crate::game::structures::buildings::extract_building_row(&ecs, building_entity);
            drop(ecs);
            Some((amount, money_now, creds_now, player, building))
        }
    };
    let Some((amount, money_now, creds_now, Some(player), Some(building))) = result else {
        tracing::error!(player_id = %player_id, pack_x, pack_y, "Resp profit state missing");
        crate::game::logic::packs::send_resp_state_error(&batch);
        return CommandEffects {
            events: vec![crate::game::GameEvent::SessionBatch {
                session_id,
                player_id,
                packets: batch.into_packets(),
            }],
            ..CommandEffects::default()
        };
    };
    if amount > 0 {
        assert!(state.mark_building_dirty(building_entity));
        crate::net::session::wire::send_u_packet(
            &batch,
            "P$",
            &crate::protocol::packets::money(money_now, creds_now).1,
        );
    }
    crate::game::logic::packs::open_resp_admin_gui(state, &batch, player_id, pack_x, pack_y);
    CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: batch.into_packets(),
        }],
        saves: if amount > 0 {
            vec![crate::game::SaveCommand::RespProfit {
                player: Box::new(player),
                building: Box::new(building),
            }]
        } else {
            Vec::new()
        },
        ..CommandEffects::default()
    }
}

pub(super) fn apply_teleport(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    coords: &str,
) -> CommandEffects {
    let batch = crate::net::session::wire::PacketBatch::default();
    crate::game::logic::teleport::apply(state, &batch, player_id, coords);
    CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: batch.into_packets(),
        }],
        ..CommandEffects::default()
    }
}

pub(super) fn apply_up_button(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    button: &str,
) -> CommandEffects {
    let batch = crate::net::session::wire::PacketBatch::default();
    if let Some(slot) = button
        .strip_prefix("skill:")
        .and_then(|slot| slot.parse().ok())
    {
        return apply_up_skill_select(state, player_id, session_id, slot, batch);
    }
    if button == "buyslot" {
        return apply_up_buy_slot(state, player_id, session_id, batch);
    }
    let durable = button == "upgrade"
        || button == "buyslot"
        || button.starts_with("delete:")
        || button.starts_with("install:");
    if durable
        && !state
            .query_player_opt(player_id, |ecs, entity| {
                Some(
                    crate::game::player::extract_player_row(ecs, entity).is_some()
                        && ecs.get::<crate::game::PlayerFlags>(entity).is_some(),
                )
            })
            .unwrap_or(false)
    {
        crate::net::session::wire::send_u_packet(
            &batch,
            "OK",
            &crate::protocol::packets::ok_message("UP", "Состояние апгрейда недоступно.").1,
        );
        return CommandEffects {
            events: vec![crate::game::GameEvent::SessionBatch {
                session_id,
                player_id,
                packets: batch.into_packets(),
            }],
            ..CommandEffects::default()
        };
    }
    if button == "upgrade" {
        return apply_up_skill_upgrade(state, player_id, session_id, batch);
    }
    if let Some(slot) = button
        .strip_prefix("delete:")
        .and_then(|slot| slot.parse().ok())
    {
        return apply_up_skill_delete(state, player_id, session_id, slot, batch);
    }
    if let Some(rest) = button.strip_prefix("install:")
        && let Some(hash_pos) = rest.find('#')
        && let Ok(slot) = rest[hash_pos + 1..].parse()
    {
        return apply_up_skill_install(
            state,
            player_id,
            session_id,
            &rest[..hash_pos],
            slot,
            batch,
        );
    }
    crate::game::logic::up_building::handle_up_button(state, &batch, player_id, button);
    let mut effects = CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: batch.into_packets(),
        }],
        ..CommandEffects::default()
    };
    if durable
        && let Some(entity) = state.get_player_entity(player_id)
        && let Some(row) = crate::game::player::extract_player_row(
            &state.ecs_read_profiled("commands.up_snapshot"),
            entity,
        )
    {
        effects
            .saves
            .push(crate::game::SaveCommand::Player { row: Box::new(row) });
    }
    effects
}

fn apply_up_skill_select(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    slot: i32,
    batch: crate::net::session::wire::PacketBatch,
) -> CommandEffects {
    let Some(payload) = crate::game::logic::up_building::prepare_up_page(state, player_id, slot)
    else {
        crate::game::logic::up_building::send_up_state_error(&batch);
        return CommandEffects {
            events: vec![crate::game::GameEvent::SessionBatch {
                session_id,
                player_id,
                packets: batch.into_packets(),
            }],
            ..CommandEffects::default()
        };
    };
    crate::net::session::wire::send_u_packet(&batch, "GU", payload.as_bytes());
    if !crate::game::logic::up_building::update_selected_slot(state, player_id, slot) {
        crate::game::logic::up_building::send_up_state_error(&batch);
    }
    CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: batch.into_packets(),
        }],
        ..CommandEffects::default()
    }
}

fn apply_up_buy_slot(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    batch: crate::net::session::wire::PacketBatch,
) -> CommandEffects {
    let state_ready = state
        .query_player_opt(player_id, |ecs, entity| {
            Some(
                crate::game::player::extract_player_row(ecs, entity).is_some()
                    && ecs.get::<crate::game::PlayerFlags>(entity).is_some()
                    && ecs.get::<crate::game::PlayerStats>(entity).is_some()
                    && ecs
                        .get::<crate::game::player::PlayerSkillsComp>(entity)
                        .is_some(),
            )
        })
        .unwrap_or(false);
    if !state_ready {
        crate::game::logic::up_building::send_up_state_error(&batch);
        return CommandEffects {
            events: vec![crate::game::GameEvent::SessionBatch {
                session_id,
                player_id,
                packets: batch.into_packets(),
            }],
            ..CommandEffects::default()
        };
    }

    let bought = state
        .modify_player(player_id, |ecs, entity| {
            let player_stats = ecs.get::<crate::game::PlayerStats>(entity)?;
            let skills = ecs.get::<crate::game::player::PlayerSkillsComp>(entity)?;
            if player_stats.creds <= 1000 || skills.states.total_slots >= 34 {
                return Some(false);
            }
            ecs.get_mut::<crate::game::player::PlayerSkillsComp>(entity)?
                .states
                .total_slots += 1;
            ecs.get_mut::<crate::game::PlayerFlags>(entity)?.dirty = true;
            Some(true)
        })
        .flatten()
        .unwrap_or(false);
    if !bought {
        return CommandEffects {
            events: vec![crate::game::GameEvent::SessionBatch {
                session_id,
                player_id,
                packets: batch.into_packets(),
            }],
            ..CommandEffects::default()
        };
    }

    let Some(payload) = crate::game::logic::up_building::prepare_up_page(state, player_id, -1)
    else {
        crate::game::logic::up_building::send_up_state_error(&batch);
        return CommandEffects {
            events: vec![crate::game::GameEvent::SessionBatch {
                session_id,
                player_id,
                packets: batch.into_packets(),
            }],
            ..CommandEffects::default()
        };
    };
    crate::net::session::wire::send_u_packet(&batch, "GU", payload.as_bytes());
    if !crate::game::logic::up_building::update_selected_slot(state, player_id, -1) {
        crate::game::logic::up_building::send_up_state_error(&batch);
    }
    let row = state.get_player_entity(player_id).and_then(|entity| {
        crate::game::player::extract_player_row(
            &state.ecs_read_profiled("commands.up_buyslot_snapshot"),
            entity,
        )
    });
    CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: batch.into_packets(),
        }],
        saves: row.map_or_else(Vec::new, |row| {
            vec![crate::game::SaveCommand::Player { row: Box::new(row) }]
        }),
        ..CommandEffects::default()
    }
}

fn apply_up_skill_delete(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    slot: i32,
    batch: crate::net::session::wire::PacketBatch,
) -> CommandEffects {
    let changed =
        crate::game::logic::up_building::handle_skill_delete(state, &batch, player_id, slot);
    let row = changed
        .then(|| {
            state.get_player_entity(player_id).and_then(|entity| {
                crate::game::player::extract_player_row(
                    &state.ecs_read_profiled("commands.up_delete_snapshot"),
                    entity,
                )
            })
        })
        .flatten();
    CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: batch.into_packets(),
        }],
        saves: row.map_or_else(Vec::new, |row| {
            vec![crate::game::SaveCommand::Player { row: Box::new(row) }]
        }),
        ..CommandEffects::default()
    }
}

fn apply_up_skill_upgrade(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    batch: crate::net::session::wire::PacketBatch,
) -> CommandEffects {
    let changed = crate::game::logic::up_building::handle_skill_upgrade(state, &batch, player_id);
    let row = changed
        .then(|| {
            state.get_player_entity(player_id).and_then(|entity| {
                crate::game::player::extract_player_row(
                    &state.ecs_read_profiled("commands.up_upgrade_snapshot"),
                    entity,
                )
            })
        })
        .flatten();
    CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: batch.into_packets(),
        }],
        saves: row.map_or_else(Vec::new, |row| {
            vec![crate::game::SaveCommand::Player { row: Box::new(row) }]
        }),
        ..CommandEffects::default()
    }
}

fn apply_up_skill_install(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    code: &str,
    slot: i32,
    batch: crate::net::session::wire::PacketBatch,
) -> CommandEffects {
    let changed =
        crate::game::logic::up_building::handle_skill_install(state, &batch, player_id, code, slot);
    let row = changed
        .then(|| {
            state.get_player_entity(player_id).and_then(|entity| {
                crate::game::player::extract_player_row(
                    &state.ecs_read_profiled("commands.up_install_snapshot"),
                    entity,
                )
            })
        })
        .flatten();
    CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: batch.into_packets(),
        }],
        saves: row.map_or_else(Vec::new, |row| {
            vec![crate::game::SaveCommand::Player { row: Box::new(row) }]
        }),
        ..CommandEffects::default()
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_persistence_completion, apply_player_command};
    use bytes::Bytes;

    #[tokio::test]
    async fn open_box_returns_typed_gui_effect() {
        let test = crate::test_support::ServerTestHarness::new("typed_open_box", "open-box").await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(3);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::OpenBox,
        );

        assert!(receiver.try_recv().is_err());
        assert!(effects.events.iter().any(|event| matches!(
            event,
            crate::game::GameEvent::SessionBatch { packets, .. }
                if packets.iter().any(|packet| {
                    openmines_protocol::Packet::try_decode(
                        &mut bytes::BytesMut::from(packet.as_slice()),
                    )
                    .is_ok_and(|decoded| decoded.is_some_and(|packet| packet.event_name == *b"GU"))
                })
        )));
        let window = test.state.query_player_opt(player_id, |ecs, entity| {
            ecs.get::<crate::game::player::PlayerUI>(entity)?
                .current_window
                .clone()
        });
        assert_eq!(window.as_deref(), Some("open_box"));
    }

    #[tokio::test]
    async fn known_noop_help_returns_typed_legacy_ok() {
        let test = crate::test_support::ServerTestHarness::new("known_noop_help", "help").await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(4);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::KnownNoopTy {
                event: "Help".to_owned(),
                payload: Bytes::new(),
            },
        );

        assert!(receiver.try_recv().is_err());
        let [crate::game::GameEvent::SessionBatch { packets, .. }] = effects.events.as_slice()
        else {
            panic!("known Help command must return one typed session batch");
        };
        let mut encoded = bytes::BytesMut::from(packets[0].as_slice());
        let packet = openmines_protocol::Packet::try_decode(&mut encoded)
            .expect("typed Help packet must decode")
            .expect("typed Help packet must be complete");
        assert_eq!(packet.event_name, *b"OK");
        assert_eq!(
            packet.payload,
            "Справка#Справка пока не подключена на сервере.".as_bytes()
        );
    }

    #[tokio::test]
    async fn geology_state_error_returns_typed_legacy_ok() {
        let test = crate::test_support::ServerTestHarness::new("geology_state_error", "geo").await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(7);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        test.state.modify_player(player_id, |ecs, entity| {
            ecs.entity_mut(entity)
                .remove::<crate::game::programmator::ProgrammatorState>();
            Some(())
        });

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Geology {
                programmatic: false,
            },
        );

        assert!(receiver.try_recv().is_err());
        let [crate::game::GameEvent::SessionBatch { packets, .. }] = effects.events.as_slice()
        else {
            panic!("geology state error must return one typed session batch");
        };
        let mut encoded = bytes::BytesMut::from(packets[0].as_slice());
        let packet = openmines_protocol::Packet::try_decode(&mut encoded)
            .expect("typed geology error packet must decode")
            .expect("typed geology error packet must be complete");
        assert_eq!(packet.event_name, *b"OK");
        assert_eq!(
            packet.payload,
            "ГЕОЛОГИЯ#Состояние игрока недоступно.".as_bytes()
        );
    }

    #[tokio::test]
    async fn heal_state_error_returns_typed_legacy_ok() {
        let test = crate::test_support::ServerTestHarness::new("heal_state_error", "heal").await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(8);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        test.state.modify_player(player_id, |ecs, entity| {
            ecs.entity_mut(entity)
                .remove::<crate::game::programmator::ProgrammatorState>();
            Some(())
        });

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Heal {
                programmatic: false,
            },
        );

        assert!(receiver.try_recv().is_err());
        let [crate::game::GameEvent::SessionBatch { packets, .. }] = effects.events.as_slice()
        else {
            panic!("heal state error must return one typed session batch");
        };
        let mut encoded = bytes::BytesMut::from(packets[0].as_slice());
        let packet = openmines_protocol::Packet::try_decode(&mut encoded)
            .expect("typed heal error packet must decode")
            .expect("typed heal error packet must be complete");
        assert_eq!(packet.event_name, *b"OK");
        assert_eq!(
            packet.payload,
            "ЛЕЧЕНИЕ#Состояние игрока недоступно.".as_bytes()
        );
    }

    #[tokio::test]
    async fn dig_state_error_returns_typed_legacy_ok() {
        let test = crate::test_support::ServerTestHarness::new("dig_state_error", "dig").await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(9);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        test.state.modify_player(player_id, |ecs, entity| {
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerPosition>();
            Some(())
        });

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Dig {
                direction: 0,
                programmatic: false,
            },
        );

        assert!(receiver.try_recv().is_err());
        let [crate::game::GameEvent::SessionBatch { packets, .. }] = effects.events.as_slice()
        else {
            panic!("dig state error must return one typed session batch");
        };
        let mut encoded = bytes::BytesMut::from(packets[0].as_slice());
        let packet = openmines_protocol::Packet::try_decode(&mut encoded)
            .expect("typed dig error packet must decode")
            .expect("typed dig error packet must be complete");
        assert_eq!(packet.event_name, *b"OK");
        assert_eq!(
            packet.payload,
            "СТРОЙКА#Состояние игрока недоступно.".as_bytes()
        );
    }

    #[tokio::test]
    async fn build_state_error_returns_typed_legacy_ok() {
        let test = crate::test_support::ServerTestHarness::new("build_state_error", "build").await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(10);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        test.state.modify_player(player_id, |ecs, entity| {
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerPosition>();
            Some(())
        });

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Build {
                direction: 0,
                block_type: "G".to_owned(),
                programmatic: false,
            },
        );

        assert!(receiver.try_recv().is_err());
        let [crate::game::GameEvent::SessionBatch { packets, .. }] = effects.events.as_slice()
        else {
            panic!("build state error must return one typed session batch");
        };
        let mut encoded = bytes::BytesMut::from(packets[0].as_slice());
        let packet = openmines_protocol::Packet::try_decode(&mut encoded)
            .expect("typed build error packet must decode")
            .expect("typed build error packet must be complete");
        assert_eq!(packet.event_name, *b"OK");
        assert_eq!(
            packet.payload,
            "СТРОЙКА#Состояние игрока недоступно.".as_bytes()
        );
    }

    #[tokio::test]
    async fn local_chat_state_error_returns_typed_legacy_ok() {
        let test = crate::test_support::ServerTestHarness::new(
            "local_chat_state_error",
            "local-chat-state-error",
        )
        .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(5);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        test.state.modify_player(player_id, |ecs, entity| {
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerUI>();
            Some(())
        });

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::LocalChat {
                message: "state error".to_owned(),
            },
        );

        assert!(receiver.try_recv().is_err());
        let [crate::game::GameEvent::SessionBatch { packets, .. }] = effects.events.as_slice()
        else {
            panic!("local chat state error must return one typed session batch");
        };
        let mut encoded = bytes::BytesMut::from(packets[0].as_slice());
        let packet = openmines_protocol::Packet::try_decode(&mut encoded)
            .expect("typed chat error packet must decode")
            .expect("typed chat error packet must be complete");
        assert_eq!(packet.event_name, *b"OK");
        assert_eq!(packet.payload, "ЧАТ#Состояние чата недоступно.".as_bytes());
    }

    #[tokio::test]
    async fn channel_chat_state_error_returns_typed_legacy_ok() {
        let test = crate::test_support::ServerTestHarness::new(
            "channel_chat_state_error",
            "channel-chat-state-error",
        )
        .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(6);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        test.state.modify_player(player_id, |ecs, entity| {
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerUI>();
            Some(())
        });

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::ChannelChat {
                payload: bytes::Bytes::from_static(b"_:hello"),
            },
        );

        assert!(receiver.try_recv().is_err());
        let [crate::game::GameEvent::SessionBatch { packets, .. }] = effects.events.as_slice()
        else {
            panic!("channel chat state error must return one typed session batch");
        };
        let mut encoded = bytes::BytesMut::from(packets[0].as_slice());
        let packet = openmines_protocol::Packet::try_decode(&mut encoded)
            .expect("typed chat error packet must decode")
            .expect("typed chat error packet must be complete");
        assert_eq!(packet.event_name, *b"OK");
        assert_eq!(packet.payload, "ЧАТ#Состояние чата недоступно.".as_bytes());
    }

    #[tokio::test]
    async fn inventory_building_db_failure_returns_typed_legacy_error() {
        let test = crate::test_support::ServerTestHarness::new(
            "inventory_building_db_failure",
            "inventory-building-db-failure",
        )
        .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(4);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::InventoryBuildingPlacementFailed,
        );

        assert!(receiver.try_recv().is_err());
        let [crate::game::GameEvent::SessionBatch { packets, .. }] = effects.events.as_slice()
        else {
            panic!("inventory building DB failure must return one typed session batch");
        };
        let mut encoded = bytes::BytesMut::from(packets[0].as_slice());
        let packet = openmines_protocol::Packet::try_decode(&mut encoded)
            .expect("typed error packet must decode")
            .expect("typed error packet must be complete");
        assert_eq!(packet.event_name, *b"OK");
        assert_eq!(packet.payload, "Ошибка#Ошибка БД".as_bytes());
    }

    #[tokio::test]
    async fn local_chat_slash_is_applied_as_a_typed_command() {
        let test =
            crate::test_support::ServerTestHarness::new("local_chat_slash", "local-slash").await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(2);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        test.state.modify_player(player_id, |ecs, entity| {
            ecs.get_mut::<crate::game::player::PlayerStats>(entity)
                .expect("connected player stats")
                .role = 2;
        });

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::LocalChat {
                message: "/moneyall 15".to_owned(),
            },
        );

        assert!(matches!(
            effects.saves.as_slice(),
            [crate::game::SaveCommand::AdminMoneyAll { request }]
                if request.player_id == player_id
                    && request.session_id == session_id
                    && request.amount == 15
        ));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn slash_ok_error_returns_typed_session_effect() {
        let test =
            crate::test_support::ServerTestHarness::new("slash_ok_typed", "slash-ok-typed").await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(212);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Slash {
                command: crate::game::SlashCommand::Invalid {
                    title: "Ошибка".to_owned(),
                    message: "Некорректная команда.".to_owned(),
                },
            },
        );
        let [crate::game::GameEvent::SessionBatch { packets, .. }] = effects.events.as_slice()
        else {
            panic!("slash OK must return one typed session batch");
        };
        let packet = openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(
            packets[0].as_slice(),
        ))
        .expect("slash OK packet must decode")
        .expect("slash OK packet must be complete");
        assert_eq!(packet.event_name, *b"OK");
        assert_eq!(packet.payload, "Ошибка#Некорректная команда.".as_bytes());
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn bonus_missing_state_returns_typed_legacy_ok() {
        let test = crate::test_support::ServerTestHarness::new(
            "bonus_missing_state_typed",
            "bonus-missing-state",
        )
        .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(213);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        test.state.modify_player(player_id, |ecs, entity| {
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerStats>();
            Some(())
        });

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::ClaimBonus,
        );
        let [crate::game::GameEvent::SessionBatch { packets, .. }] = effects.events.as_slice()
        else {
            panic!("bonus error must return one typed session batch");
        };
        let packet = openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(
            packets[0].as_slice(),
        ))
        .expect("bonus error packet must decode")
        .expect("bonus error packet must be complete");
        assert_eq!(packet.event_name, *b"OK");
        assert_eq!(
            packet.payload,
            "Бонус#Состояние бонуса недоступно.".as_bytes()
        );
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn settings_save_is_delivered_as_typed_session_effect() {
        let test =
            crate::test_support::ServerTestHarness::new("settings_typed_save", "settings").await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(201);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::parse("save:isca:1#mous:0#".to_owned()),
            },
        );

        assert!(effects.saves.is_empty());
        assert!(matches!(
            effects.events.as_slice(),
            [crate::game::GameEvent::SessionBatch {
                session_id: event_session,
                player_id: event_player,
                packets,
            }] if *event_session == session_id
                && *event_player == player_id
                && packets.iter().any(|packet| {
                    openmines_protocol::Packet::try_decode(
                        &mut bytes::BytesMut::from(packet.as_slice()),
                    )
                    .is_ok_and(|decoded| decoded.is_some_and(|packet| packet.event_name == *b"#S"))
                })
        ));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn settings_toggles_and_open_are_typed_legacy_wire_effects() {
        let test = crate::test_support::ServerTestHarness::new(
            "settings_typed_effects",
            "settings-effects",
        )
        .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(202);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);

        let auto_dig = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::ToggleAutoDig,
        );
        let aggression = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::ToggleAggression,
        );
        let settings = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::SettingsSave {
                payload: bytes::Bytes::new(),
            },
        );

        let packet_event = |effects: &crate::game::CommandEffects| {
            let [crate::game::GameEvent::SessionBatch { packets, .. }] = effects.events.as_slice()
            else {
                panic!("setting command must emit one session batch")
            };
            let mut bytes = bytes::BytesMut::from(packets[0].as_slice());
            openmines_protocol::Packet::try_decode(&mut bytes)
                .expect("setting packet must decode")
                .expect("setting packet must be complete")
        };

        let auto_packet = packet_event(&auto_dig);
        assert_eq!(auto_packet.event_name, *b"BD");
        assert_eq!(auto_packet.payload, &b"1"[..]);
        let aggression_packet = packet_event(&aggression);
        assert_eq!(aggression_packet.event_name, *b"BA");
        assert_eq!(aggression_packet.payload, &b"1"[..]);
        let settings_packet = packet_event(&settings);
        assert_eq!(settings_packet.event_name, *b"GU");
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn admin_action_returns_typed_session_effect_without_direct_write() {
        let test = crate::test_support::ServerTestHarness::new("admin_typed", "admin").await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(203);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        test.state.modify_player(player_id, |ecs, entity| {
            ecs.get_mut::<crate::game::player::PlayerStats>(entity)
                .expect("connected player stats")
                .role = 2;
            Some(())
        });

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::AdminAction,
        );

        assert!(receiver.try_recv().is_err());
        let [crate::game::GameEvent::SessionBatch { packets, .. }] = effects.events.as_slice()
        else {
            panic!("admin action must return one session batch");
        };
        let mut encoded = bytes::BytesMut::from(packets[0].as_slice());
        let packet = openmines_protocol::Packet::try_decode(&mut encoded)
            .unwrap()
            .unwrap();
        assert_eq!(packet.event_name, *b"OK");
    }

    #[tokio::test]
    async fn inventory_commands_deliver_legacy_packets_as_typed_session_effects() {
        let test =
            crate::test_support::ServerTestHarness::new("inventory_typed_effect", "inventory")
                .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(204);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);

        let toggle = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::InventoryToggle,
        );
        let choose = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::InventoryChoose {
                payload: Bytes::from_static(b"2"),
            },
        );

        let packet_events = |effects: &crate::game::CommandEffects| {
            let [
                crate::game::GameEvent::SessionBatch {
                    session_id: event_session,
                    player_id: event_player,
                    packets,
                },
            ] = effects.events.as_slice()
            else {
                panic!("inventory command must return one typed session batch");
            };
            assert_eq!(*event_session, session_id);
            assert_eq!(*event_player, player_id);
            packets
                .iter()
                .map(|packet| {
                    openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(
                        packet.as_slice(),
                    ))
                    .expect("inventory packet must decode")
                    .expect("inventory packet must be present")
                    .event_name
                })
                .collect::<Vec<_>>()
        };

        assert_eq!(packet_events(&toggle), vec![*b"IN"]);
        assert_eq!(packet_events(&choose), vec![*b"IN", *b"IN"]);
        assert!(toggle.saves.is_empty() && choose.saves.is_empty());
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn chat_choose_resyncs_selected_channel_through_typed_persistence() {
        let test = crate::test_support::ServerTestHarness::new("chat_choose_typed", "chat").await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(205);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        let before = test
            .state
            .query_player_opt(player_id, |ecs, entity| {
                ecs.get::<crate::game::player::PlayerUI>(entity)
                    .map(|ui| ui.current_chat.clone())
            })
            .expect("player UI");

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::ChatChoose {
                payload: Bytes::from_static(b"DNO"),
            },
        );

        assert!(effects.events.is_empty());
        assert!(matches!(
            effects.saves.as_slice(),
            [crate::game::SaveCommand::ChatResync { request }]
                if request.player_id == player_id
                    && request.session_id == session_id
                    && request.channel_tag == "DNO"
                    && request.last_id == 0
        ));
        let after = test
            .state
            .query_player_opt(player_id, |ecs, entity| {
                ecs.get::<crate::game::player::PlayerUI>(entity)
                    .map(|ui| ui.current_chat.clone())
            })
            .expect("player UI");
        assert_eq!(after, before);
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn chat_resync_completion_preserves_typed_legacy_order_and_errors() {
        let test = crate::test_support::ServerTestHarness::new(
            "chat_resync_completion_typed",
            "chat-resync",
        )
        .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(206);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);

        let success = apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::ChatResynced {
                request: crate::game::ChatResyncRequest {
                    player_id,
                    session_id,
                    channel_tag: "DNO".to_owned(),
                    last_id: 0,
                },
                result: crate::game::ChatResyncResult::Success {
                    channel_name: "Общий".to_owned(),
                    messages: Vec::new(),
                },
            },
        );
        let packet_events = |effects: &crate::game::CommandEffects| {
            let [crate::game::GameEvent::SessionBatch { packets, .. }] = effects.events.as_slice()
            else {
                panic!("chat resync completion must return one typed session batch");
            };
            packets
                .iter()
                .map(|packet| {
                    openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(
                        packet.as_slice(),
                    ))
                    .expect("chat packet must decode")
                    .expect("chat packet must be complete")
                    .event_name
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(packet_events(&success), vec![*b"mO", *b"mU"]);
        assert_eq!(
            test.state.query_player_opt(player_id, |ecs, entity| {
                ecs.get::<crate::game::player::PlayerUI>(entity)
                    .map(|ui| ui.current_chat.clone())
            }),
            Some("DNO".to_owned())
        );
        assert!(receiver.try_recv().is_err());

        let failure = apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::ChatResynced {
                request: crate::game::ChatResyncRequest {
                    player_id,
                    session_id,
                    channel_tag: "DNO".to_owned(),
                    last_id: 0,
                },
                result: crate::game::ChatResyncResult::PermanentFailure {
                    message: "database unavailable".to_owned(),
                },
            },
        );
        assert_eq!(packet_events(&failure), vec![*b"OK"]);
        let [crate::game::GameEvent::SessionBatch { packets, .. }] = failure.events.as_slice()
        else {
            unreachable!();
        };
        let packet = openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(
            packets[0].as_slice(),
        ))
        .expect("chat error packet must decode")
        .expect("chat error packet must be complete");
        assert_eq!(
            packet.payload,
            "ЧАТ#Не удалось прочитать данные чата.".as_bytes()
        );
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn chat_menu_completion_preserves_typed_legacy_order_and_errors() {
        let test =
            crate::test_support::ServerTestHarness::new("chat_menu_completion_typed", "chat-menu")
                .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(207);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);

        let success = apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::ChatMenuLoaded {
                request: crate::game::ChatMenuRequest {
                    player_id,
                    session_id,
                },
                result: crate::game::ChatMenuResult::Success {
                    channels: vec![(
                        "DNO".to_owned(),
                        true,
                        "Общий".to_owned(),
                        "preview".to_owned(),
                    )],
                },
            },
        );
        let packets = |effects: &crate::game::CommandEffects| {
            let [crate::game::GameEvent::SessionBatch { packets, .. }] = effects.events.as_slice()
            else {
                panic!("chat menu completion must return one typed session batch");
            };
            packets
                .iter()
                .map(|packet| {
                    openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(
                        packet.as_slice(),
                    ))
                    .expect("chat menu packet must decode")
                    .expect("chat menu packet must be complete")
                    .event_name
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(packets(&success), vec![*b"mL", *b"mN"]);
        assert!(receiver.try_recv().is_err());

        let failure = apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::ChatMenuLoaded {
                request: crate::game::ChatMenuRequest {
                    player_id,
                    session_id,
                },
                result: crate::game::ChatMenuResult::PermanentFailure {
                    message: "database unavailable".to_owned(),
                },
            },
        );
        assert_eq!(packets(&failure), vec![*b"OK"]);
        let [crate::game::GameEvent::SessionBatch { packets, .. }] = failure.events.as_slice()
        else {
            unreachable!();
        };
        let packet = openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(
            packets[0].as_slice(),
        ))
        .expect("chat menu error packet must decode")
        .expect("chat menu error packet must be complete");
        assert_eq!(
            packet.payload,
            "ЧАТ#Не удалось прочитать данные чата.".as_bytes()
        );
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn chat_private_completion_preserves_typed_legacy_order_and_errors() {
        let test = crate::test_support::ServerTestHarness::new(
            "chat_private_completion_typed",
            "chat-private",
        )
        .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(208);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);

        let success = apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::ChatPrivateOpened {
                request: crate::game::ChatPrivateRequest {
                    player_id,
                    session_id,
                    target_uid: crate::game::PlayerId(42),
                },
                result: crate::game::ChatPrivateResult::Success {
                    target_name: "Друг".to_owned(),
                    channel_tag: "P42".to_owned(),
                    messages: Vec::new(),
                },
            },
        );
        let packet_events = |effects: &crate::game::CommandEffects| {
            let [crate::game::GameEvent::SessionBatch { packets, .. }] = effects.events.as_slice()
            else {
                panic!("private chat completion must return one typed session batch");
            };
            packets
                .iter()
                .map(|packet| {
                    openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(
                        packet.as_slice(),
                    ))
                    .expect("private chat packet must decode")
                    .expect("private chat packet must be complete")
                    .event_name
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(packet_events(&success), vec![*b"mO", *b"mU"]);
        assert_eq!(
            test.state.query_player_opt(player_id, |ecs, entity| {
                ecs.get::<crate::game::player::PlayerUI>(entity)
                    .map(|ui| ui.current_chat.clone())
            }),
            Some("P42".to_owned())
        );
        assert!(receiver.try_recv().is_err());

        let missing = apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::ChatPrivateOpened {
                request: crate::game::ChatPrivateRequest {
                    player_id,
                    session_id,
                    target_uid: crate::game::PlayerId(404),
                },
                result: crate::game::ChatPrivateResult::TargetNotFound,
            },
        );
        assert!(missing.events.is_empty());
        assert!(receiver.try_recv().is_err());

        let failure = apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::ChatPrivateOpened {
                request: crate::game::ChatPrivateRequest {
                    player_id,
                    session_id,
                    target_uid: crate::game::PlayerId(42),
                },
                result: crate::game::ChatPrivateResult::PermanentFailure {
                    message: "database unavailable".to_owned(),
                },
            },
        );
        assert_eq!(packet_events(&failure), vec![*b"OK"]);
        let [crate::game::GameEvent::SessionBatch { packets, .. }] = failure.events.as_slice()
        else {
            unreachable!();
        };
        let packet = openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(
            packets[0].as_slice(),
        ))
        .expect("private chat error packet must decode")
        .expect("private chat error packet must be complete");
        assert_eq!(packet.payload, "Ошибка#Ошибка БД".as_bytes());
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn clan_gui_mutation_is_admitted_without_legacy_session_delivery() {
        let test = crate::test_support::ServerTestHarness::new("clan_gui_typed", "clan-gui").await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(202);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::parse("clan_request:17".to_owned()),
            },
        );

        assert!(matches!(
            effects.saves.as_slice(),
            [crate::game::SaveCommand::ClanCommand { request }]
                if request.player_id == player_id
                    && request.session_id == session_id
                    && request.action == (crate::game::ClanAction::Request { clan_id: 17 })
        ));
        assert!(effects.events.is_empty());
        assert!(effects.broadcasts.is_empty());
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn program_editor_open_is_admitted_without_legacy_gui_task() {
        let test =
            crate::test_support::ServerTestHarness::new("program_open_durable", "programmer").await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(203);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::parse("openprog:42".to_owned()),
            },
        );

        assert!(effects.events.is_empty());
        assert!(matches!(
            effects.saves.as_slice(),
            [crate::game::SaveCommand::ProgramOpen { request }]
                if request.player_id == player_id
                    && request.session_id == session_id
                    && request.program == 42
        ));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn program_editor_rename_is_admitted_without_legacy_gui_task() {
        let test =
            crate::test_support::ServerTestHarness::new("program_rename_durable", "programmer")
                .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(207);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::parse("rename:42:new-name".to_owned()),
            },
        );

        assert!(effects.events.is_empty());
        assert!(matches!(
            effects.saves.as_slice(),
            [crate::game::SaveCommand::ProgramRename { request }]
                if request.player_id == player_id
                    && request.session_id == session_id
                    && request.program_id == 42
                    && request.name == "new-name"
        ));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn program_delete_is_admitted_without_legacy_gui_task() {
        let test =
            crate::test_support::ServerTestHarness::new("program_delete_durable", "programmer")
                .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(208);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        test.state.modify_player(player_id, |ecs, entity| {
            ecs.get_mut::<crate::game::programmator::ProgrammatorState>(entity)
                .expect("programmator state")
                .selected_id = Some(42);
        });

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::ProgramAction {
                event: "PDEL".to_owned(),
                payload: bytes::Bytes::from_static(b"42"),
            },
        );

        assert!(effects.events.is_empty());
        assert!(matches!(
            effects.saves.as_slice(),
            [crate::game::SaveCommand::ProgramDelete { request }]
                if request.player_id == player_id
                    && request.session_id == session_id
                    && request.program_id == 42
                    && request.clear_selected
        ));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn program_delete_completion_clears_runtime_without_wire() {
        let test =
            crate::test_support::ServerTestHarness::new("program_delete_completion", "programmer")
                .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(209);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        test.state.modify_player(player_id, |ecs, entity| {
            let mut program = ecs
                .get_mut::<crate::game::programmator::ProgrammatorState>(entity)
                .expect("programmator state");
            program.selected_id = Some(42);
            program.selected_data = Some("source".to_owned());
            program.running = true;
        });

        let effects = apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::ProgramDeleted {
                request: crate::game::ProgramDeleteRequest {
                    player_id,
                    session_id,
                    program_id: 42,
                    clear_selected: true,
                },
                result: crate::game::ProgramDeleteResult::Deleted,
            },
        );

        assert!(effects.events.is_empty());
        assert_eq!(
            test.state.query_player(player_id, |ecs, entity| {
                let program = ecs
                    .get::<crate::game::programmator::ProgrammatorState>(entity)
                    .expect("programmator state");
                (
                    program.selected_id,
                    program.selected_data.clone(),
                    program.running,
                )
            }),
            Some((None, None, false))
        );
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn program_copy_is_admitted_without_legacy_gui_task() {
        let test =
            crate::test_support::ServerTestHarness::new("program_copy_durable", "programmer").await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(210);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::ProgramAction {
                event: "PCOP".to_owned(),
                payload: bytes::Bytes::from_static(b"42"),
            },
        );

        assert!(effects.events.is_empty());
        assert!(matches!(
            effects.saves.as_slice(),
            [crate::game::SaveCommand::ProgramCopy { request }]
                if request.player == player_id
                    && request.session == session_id
                    && request.program == 42
        ));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn malformed_program_save_returns_typed_legacy_error() {
        let test = crate::test_support::ServerTestHarness::new(
            "program_malformed_save",
            "programmer-malformed",
        )
        .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(209);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::ProgramAction {
                event: "PROG".to_owned(),
                payload: bytes::Bytes::from_static(b"malformed"),
            },
        );
        let [crate::game::GameEvent::SessionBatch { packets, .. }] = effects.events.as_slice()
        else {
            panic!("malformed program save must return one typed session batch");
        };
        let events = packets
            .iter()
            .map(|packet| {
                openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(
                    packet.as_slice(),
                ))
                .expect("program error packet must decode")
                .expect("program error packet must be complete")
                .event_name
            })
            .collect::<Vec<_>>();
        assert_eq!(events, vec![*b"@P", *b"OK"]);
        assert!(effects.saves.is_empty());
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn program_reset_returns_typed_legacy_packet_order() {
        let test =
            crate::test_support::ServerTestHarness::new("program_reset_typed", "programmer-reset")
                .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(210);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        test.state.modify_player(player_id, |ecs, entity| {
            let mut program =
                ecs.get_mut::<crate::game::programmator::ProgrammatorState>(entity)?;
            program.running = true;
            program.hand_mode_active = true;
            Some(())
        });

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::ProgramAction {
                event: "pRST".to_owned(),
                payload: Bytes::new(),
            },
        );
        let [crate::game::GameEvent::SessionBatch { packets, .. }] = effects.events.as_slice()
        else {
            panic!("program reset must return one typed session batch");
        };
        let events = packets
            .iter()
            .map(|packet| {
                openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(
                    packet.as_slice(),
                ))
                .expect("program reset packet must decode")
                .expect("program reset packet must be complete")
                .event_name
            })
            .collect::<Vec<_>>();
        assert_eq!(events, vec![*b"Gu", *b"@P", *b"BH"]);
        assert!(receiver.try_recv().is_err());
        assert_eq!(
            test.state.query_player_opt(player_id, |ecs, entity| {
                let program = ecs.get::<crate::game::programmator::ProgrammatorState>(entity)?;
                Some((program.running, program.hand_mode_active))
            }),
            Some((false, false))
        );
    }

    #[tokio::test]
    async fn program_rename_prompt_returns_typed_gui_or_status_effect() {
        let test = crate::test_support::ServerTestHarness::new(
            "program_rename_prompt_typed",
            "programmer-rename",
        )
        .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(211);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);

        let valid = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::ProgramAction {
                event: "PREN".to_owned(),
                payload: Bytes::from_static(b"42"),
            },
        );
        let [crate::game::GameEvent::SessionBatch { packets, .. }] = valid.events.as_slice() else {
            panic!("valid rename prompt must return one typed session batch");
        };
        let packet = openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(
            packets[0].as_slice(),
        ))
        .expect("rename prompt packet must decode")
        .expect("rename prompt packet must be complete");
        assert_eq!(packet.event_name, *b"GU");
        let payload = std::str::from_utf8(&packet.payload).expect("HORB payload must be UTF-8");
        assert!(payload.contains("ПЕРЕИМЕНОВАТЬ"));
        assert!(payload.contains("rename:42:%I%"));
        assert_eq!(
            test.state.query_player_opt(player_id, |ecs, entity| {
                ecs.get::<crate::game::player::PlayerUI>(entity)
                    .map(|ui| ui.current_window.clone())
            }),
            Some(Some("pren:42".to_owned()))
        );
        assert!(receiver.try_recv().is_err());

        let invalid = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::ProgramAction {
                event: "PREN".to_owned(),
                payload: Bytes::from_static(b"not-an-id"),
            },
        );
        let [crate::game::GameEvent::SessionBatch { packets, .. }] = invalid.events.as_slice()
        else {
            panic!("invalid rename prompt must return one typed session batch");
        };
        let packet = openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(
            packets[0].as_slice(),
        ))
        .expect("rename status packet must decode")
        .expect("rename status packet must be complete");
        assert_eq!(packet.event_name, *b"@P");
        assert_eq!(packet.payload, b"0".as_slice());
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn empty_program_save_reopens_list_through_typed_persistence() {
        let test =
            crate::test_support::ServerTestHarness::new("program_empty_save", "programmer").await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(211);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        let mut payload = Vec::new();
        payload.extend_from_slice(&0_i32.to_le_bytes());
        payload.extend_from_slice(&0_i32.to_le_bytes());

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::ProgramAction {
                event: "PROG".to_owned(),
                payload: bytes::Bytes::from(payload),
            },
        );

        assert!(effects.events.is_empty());
        assert!(matches!(
            effects.saves.as_slice(),
            [crate::game::SaveCommand::ProgramMenu { request }]
                if request.player_id == player_id && request.session_id == session_id
        ));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn program_editor_completion_preserves_typed_legacy_packet_order() {
        let test =
            crate::test_support::ServerTestHarness::new("program_editor_completion", "programmer")
                .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(212);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);

        let open = apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::ProgramOpened {
                request: crate::game::ProgramOpenRequest {
                    player_id,
                    session_id,
                    program: 42,
                },
                result: crate::game::ProgramOpenResult::Opened {
                    program: crate::db::ProgramRow {
                        id: 42,
                        player_id: test.player.id,
                        name: "demo".to_owned(),
                        code: "source".to_owned(),
                    },
                },
            },
        );
        let rename = apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::ProgramRenamed {
                request: crate::game::ProgramRenameRequest {
                    player_id,
                    session_id,
                    program_id: 42,
                    name: "renamed".to_owned(),
                },
                result: crate::game::ProgramRenameResult::Renamed {
                    program: crate::db::ProgramRow {
                        id: 42,
                        player_id: test.player.id,
                        name: "renamed".to_owned(),
                        code: "source".to_owned(),
                    },
                },
            },
        );

        let events = |effects: &crate::game::CommandEffects| {
            let [crate::game::GameEvent::SessionBatch { packets, .. }] = effects.events.as_slice()
            else {
                panic!("program editor completion must return one typed session batch");
            };
            packets
                .iter()
                .map(|packet| {
                    openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(
                        packet.as_slice(),
                    ))
                    .expect("program packet must decode")
                    .expect("program packet must be complete")
                    .event_name
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(events(&open), vec![*b"Gu", *b"#P", *b"Gu"]);
        assert_eq!(events(&rename), vec![*b"#p", *b"Gu"]);
        assert!(receiver.try_recv().is_err());
        assert_eq!(
            test.state.query_player_opt(player_id, |ecs, entity| {
                let program = ecs.get::<crate::game::programmator::ProgrammatorState>(entity)?;
                let ui = ecs.get::<crate::game::player::PlayerUI>(entity)?;
                Some((
                    program.selected_id,
                    program.selected_data.clone(),
                    ui.current_window.clone(),
                ))
            }),
            Some((Some(42), Some("source".to_owned()), None))
        );
    }

    #[tokio::test]
    async fn programmator_persistence_errors_return_typed_legacy_ok() {
        let test = crate::test_support::ServerTestHarness::new(
            "programmator_persistence_errors",
            "programmer-errors",
        )
        .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(213);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);

        let open = apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::ProgramOpened {
                request: crate::game::ProgramOpenRequest {
                    player_id,
                    session_id,
                    program: 42,
                },
                result: crate::game::ProgramOpenResult::Rejected,
            },
        );
        let rename = apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::ProgramRenamed {
                request: crate::game::ProgramRenameRequest {
                    player_id,
                    session_id,
                    program_id: 42,
                    name: "renamed".to_owned(),
                },
                result: crate::game::ProgramRenameResult::PermanentFailure {
                    message: "db failure".to_owned(),
                },
            },
        );
        let create = apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::ProgramCreated {
                request: crate::game::ProgramCreateRequest {
                    player_id,
                    session_id,
                    name: "new".to_owned(),
                },
                result: crate::game::ProgramCreateResult::PermanentFailure {
                    message: "db failure".to_owned(),
                },
            },
        );

        let packet = |effects: &crate::game::CommandEffects| {
            let [crate::game::GameEvent::SessionBatch { packets, .. }] = effects.events.as_slice()
            else {
                panic!("programmator error must return one typed session batch");
            };
            let mut encoded = bytes::BytesMut::from(packets[0].as_slice());
            openmines_protocol::Packet::try_decode(&mut encoded)
                .expect("programmator error packet must decode")
                .expect("programmator error packet must be complete")
        };
        assert_eq!(packet(&open).event_name, *b"OK");
        assert_eq!(
            packet(&open).payload,
            "ПРОГРАММАТОР#Программа недоступна.".as_bytes()
        );
        assert_eq!(
            packet(&rename).payload,
            "ПРОГРАММАТОР#Не удалось переименовать программу.".as_bytes()
        );
        assert_eq!(
            packet(&create).payload,
            "ПРОГРАММАТОР#Не удалось создать программу.".as_bytes()
        );
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn programmator_save_errors_return_typed_legacy_ok() {
        let test = crate::test_support::ServerTestHarness::new(
            "programmator_save_errors",
            "programmer-save-errors",
        )
        .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(214);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);

        let effects = apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::ProgramSaved {
                request: crate::game::ProgramSaveRequest {
                    player_id,
                    session_id,
                    program_id: 42,
                    source: "source".to_owned(),
                },
                result: crate::game::ProgramSaveResult::Rejected,
            },
        );

        let [crate::game::GameEvent::SessionBatch { packets, .. }] = effects.events.as_slice()
        else {
            panic!("program save error must return one typed session batch");
        };
        let mut encoded = bytes::BytesMut::from(packets[0].as_slice());
        let packet = openmines_protocol::Packet::try_decode(&mut encoded)
            .expect("program save error packet must decode")
            .expect("program save error packet must be complete");
        assert_eq!(packet.event_name, *b"OK");
        assert_eq!(
            packet.payload,
            "ПРОГРАММАТОР#Не удалось сохранить программу.".as_bytes()
        );
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn programmator_save_success_preserves_typed_legacy_packet_order() {
        let test = crate::test_support::ServerTestHarness::new(
            "programmator_save_success",
            "programmer-save-success",
        )
        .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(215);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);

        let effects = apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::ProgramSaved {
                request: crate::game::ProgramSaveRequest {
                    player_id,
                    session_id,
                    program_id: 42,
                    source: String::new(),
                },
                result: crate::game::ProgramSaveResult::Saved {
                    program_name: "empty".to_owned(),
                },
            },
        );

        let [crate::game::GameEvent::SessionBatch { packets, .. }] = effects.events.as_slice()
        else {
            panic!("program save success must return one typed session batch");
        };
        let names = packets
            .iter()
            .map(|packet| {
                openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(
                    packet.as_slice(),
                ))
                .expect("program save packet must decode")
                .expect("program save packet must be complete")
                .event_name
            })
            .collect::<Vec<_>>();
        assert_eq!(names, vec![*b"Gu", *b"@P", *b"BH", *b"OK"]);
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn auction_grid_is_admitted_without_legacy_gui_task() {
        let test =
            crate::test_support::ServerTestHarness::new("auction_grid", "auction-grid").await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(3);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        test.state.modify_player(player_id, |ecs, entity| {
            if let Some(mut ui) = ecs.get_mut::<crate::game::player::PlayerUI>(entity) {
                ui.current_window = Some("market:12:34:sell".to_owned());
            }
        });

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::parse("auc".to_owned()),
            },
        );

        assert!(matches!(
            effects.saves.as_slice(),
            [crate::game::SaveCommand::AuctionGrid { request }]
                if request.player_id == player_id
                    && request.session_id == session_id
                    && request.building_x == 12
                    && request.building_y == 34
        ));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn auction_grid_completion_preserves_legacy_horb_payload() {
        let test = crate::test_support::ServerTestHarness::new(
            "auction_grid_completion",
            "auction-grid-completion",
        )
        .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(4);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);

        let effects = apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::AuctionGridLoaded {
                request: crate::game::AuctionGridRequest {
                    player_id,
                    session_id,
                    building_x: 12,
                    building_y: 34,
                },
                result: crate::game::AuctionGridResult::Loaded {
                    counts: vec![(1, 2, 50), (50, 1, 70)],
                },
            },
        );

        let packet = effects
            .events
            .into_iter()
            .find_map(|event| match event {
                crate::game::GameEvent::SessionBatch { packets, .. } => packets.into_iter().next(),
                _ => None,
            })
            .expect("auction completion must emit GU");
        let decoded =
            openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(packet.as_slice()))
                .expect("GU packet must decode")
                .expect("GU packet must be complete");
        assert_eq!(decoded.event_name, *b"GU");
        let payload = String::from_utf8_lossy(&decoded.payload);
        assert!(payload.contains("1: 2;!50$"));
        assert!(!payload.contains("49:"));
        assert!(payload.contains("50: 1;!70$"));
        assert_eq!(
            test.state.query_player(player_id, |ecs, entity| {
                ecs.get::<crate::game::player::PlayerUI>(entity)
                    .and_then(|ui| ui.current_window.clone())
            }),
            Some(Some("market:12:34:auc".to_owned()))
        );
    }

    #[tokio::test]
    async fn auction_item_orders_is_admitted_without_legacy_gui_task() {
        let test = crate::test_support::ServerTestHarness::new(
            "auction_item_orders",
            "auction-item-orders",
        )
        .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(5);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        test.state.modify_player(player_id, |ecs, entity| {
            if let Some(mut ui) = ecs.get_mut::<crate::game::player::PlayerUI>(entity) {
                ui.current_window = Some("market:12:34:auc".to_owned());
            }
        });

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::parse("choose:1".to_owned()),
            },
        );

        assert!(matches!(
            effects.saves.as_slice(),
            [crate::game::SaveCommand::AuctionItemOrders { request }]
                if request.player_id == player_id
                    && request.session_id == session_id
                    && request.building_x == 12
                    && request.building_y == 34
                    && request.item_id == 1
        ));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn auction_item_orders_completion_preserves_legacy_horb_payload() {
        let test = crate::test_support::ServerTestHarness::new(
            "auction_item_orders_completion",
            "auction-item-orders-completion",
        )
        .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(6);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);

        let effects = apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::AuctionItemOrdersLoaded {
                request: crate::game::AuctionItemOrdersRequest {
                    player_id,
                    session_id,
                    building_x: 12,
                    building_y: 34,
                    item_id: 1,
                },
                result: crate::game::AuctionItemOrdersResult::Loaded { orders: Vec::new() },
            },
        );

        let packet = effects
            .events
            .into_iter()
            .find_map(|event| match event {
                crate::game::GameEvent::SessionBatch { packets, .. } => packets.into_iter().next(),
                _ => None,
            })
            .expect("auction item orders completion must emit GU");
        let decoded =
            openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(packet.as_slice()))
                .expect("GU packet must decode")
                .expect("GU packet must be complete");
        assert_eq!(decoded.event_name, *b"GU");
        let payload = String::from_utf8_lossy(&decoded.payload);
        assert!(payload.contains("Auc "));
        assert!(payload.contains("auccreate:1"));
        assert_eq!(
            test.state.query_player(player_id, |ecs, entity| {
                ecs.get::<crate::game::player::PlayerUI>(entity)
                    .and_then(|ui| ui.current_window.clone())
            }),
            Some(Some("market:12:34:auc".to_owned()))
        );
    }

    #[tokio::test]
    async fn auction_order_creation_pages_are_typed_presentation_effects() {
        let test = crate::test_support::ServerTestHarness::new(
            "auction_order_creation_pages",
            "auction-order-pages",
        )
        .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(13);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        test.state.modify_player(player_id, |ecs, entity| {
            ecs.get_mut::<crate::game::player::PlayerUI>(entity)
                .expect("connected player UI")
                .current_window = Some("market:12:34:auc".to_owned());
        });

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::parse("auccreate:1".to_owned()),
            },
        );
        let packet = effects
            .events
            .into_iter()
            .find_map(|event| match event {
                crate::game::GameEvent::SessionBatch { packets, .. } => packets.into_iter().next(),
                _ => None,
            })
            .expect("creation page must be returned as an effect");
        let decoded =
            openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(packet.as_slice()))
                .expect("GU packet must decode")
                .expect("GU packet must be complete");
        assert_eq!(decoded.event_name, *b"GU");
        let payload = String::from_utf8_lossy(&decoded.payload);
        assert!(payload.contains("aucsetcost:1:%I%"));
        assert!(effects.saves.is_empty());
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn auction_order_is_admitted_without_legacy_gui_task() {
        let test =
            crate::test_support::ServerTestHarness::new("auction_order", "auction-order").await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(7);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        test.state.modify_player(player_id, |ecs, entity| {
            if let Some(mut ui) = ecs.get_mut::<crate::game::player::PlayerUI>(entity) {
                ui.current_window = Some("market:12:34:auc".to_owned());
            }
        });

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::parse("openorder:42".to_owned()),
            },
        );

        assert!(matches!(
            effects.saves.as_slice(),
            [crate::game::SaveCommand::AuctionOrder { request }]
                if request.player_id == player_id
                    && request.session_id == session_id
                    && request.building_x == 12
                    && request.building_y == 34
                    && request.order_id == 42
        ));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn auction_order_completion_preserves_legacy_horb_payload() {
        let test = crate::test_support::ServerTestHarness::new(
            "auction_order_completion",
            "auction-order-completion",
        )
        .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(8);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);

        let effects = apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::AuctionOrderLoaded {
                request: crate::game::AuctionOrderRequest {
                    player_id,
                    session_id,
                    building_x: 12,
                    building_y: 34,
                    order_id: 42,
                },
                result: crate::game::AuctionOrderResult::Loaded {
                    order: crate::db::orders::OrderRow {
                        id: 42,
                        initiator_id: 1,
                        item_id: 1,
                        num: 3,
                        cost: 100,
                        buyer_id: 9,
                        bet_time: crate::tasks::auction::now_unix(),
                    },
                    buyer_name: Some("Buyer".to_owned()),
                },
            },
        );

        let packet = effects
            .events
            .into_iter()
            .find_map(|event| match event {
                crate::game::GameEvent::SessionBatch { packets, .. } => packets.into_iter().next(),
                _ => None,
            })
            .expect("auction order completion must emit GU");
        let decoded =
            openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(packet.as_slice()))
                .expect("GU packet must decode")
                .expect("GU packet must be complete");
        assert_eq!(decoded.event_name, *b"GU");
        let payload = String::from_utf8_lossy(&decoded.payload);
        assert!(payload.contains("aucminbet:42"));
        assert!(payload.contains("aucbet:42:%I%"));
        assert!(payload.contains("choose:1"));
        assert!(payload.contains("by: Buyer"));
        assert_eq!(
            test.state.query_player(player_id, |ecs, entity| {
                ecs.get::<crate::game::player::PlayerUI>(entity)
                    .and_then(|ui| ui.current_window.clone())
            }),
            Some(Some("market:12:34:auc".to_owned()))
        );
    }

    #[tokio::test]
    async fn auction_order_create_deducts_inventory_and_preserves_completion_wire() {
        let test = crate::test_support::ServerTestHarness::new(
            "auction_order_create",
            "auction-order-create",
        )
        .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(9);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        test.state.modify_player(player_id, |ecs, entity| {
            ecs.get_mut::<crate::game::player::PlayerInventory>(entity)
                .expect("connected player inventory")
                .items
                .insert(1, 5);
            ecs.get_mut::<crate::game::player::PlayerUI>(entity)
                .expect("connected player UI")
                .current_window = Some("market:12:34:auc".to_owned());
        });

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::parse("aucsetnum:1:100:2".to_owned()),
            },
        );
        assert!(matches!(
            effects.saves.as_slice(),
            [crate::game::SaveCommand::AuctionOrderCreate { request }]
                if request.item_id == 1
                    && request.num == 2
                    && request.cost == 100
                    && request.building_x == 12
                    && request.building_y == 34
        ));
        assert_eq!(
            test.state.query_player(player_id, |ecs, entity| {
                ecs.get::<crate::game::player::PlayerInventory>(entity)
                    .and_then(|inventory| inventory.items.get(&1).copied())
            }),
            Some(Some(3))
        );

        let completion = apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::AuctionOrderCreated {
                request: crate::game::AuctionOrderCreateRequest {
                    player_id,
                    session_id,
                    building_x: 12,
                    building_y: 34,
                    item_id: 1,
                    num: 2,
                    cost: 100,
                },
                result: crate::game::AuctionOrderCreateResult::Created,
            },
        );
        let packet = completion
            .events
            .into_iter()
            .find_map(|event| match event {
                crate::game::GameEvent::SessionBatch { packets, .. } => packets.into_iter().next(),
                _ => None,
            })
            .expect("create completion must emit GU");
        let decoded =
            openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(packet.as_slice()))
                .expect("GU packet must decode")
                .expect("GU packet must be complete");
        assert_eq!(decoded.event_name, *b"GU");
        assert!(String::from_utf8_lossy(&decoded.payload).contains("u just created order"));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn auction_order_create_failure_refunds_inventory_without_success_page() {
        let test = crate::test_support::ServerTestHarness::new(
            "auction_order_create_failure",
            "auction-order-create-failure",
        )
        .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(10);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        test.state.modify_player(player_id, |ecs, entity| {
            ecs.get_mut::<crate::game::player::PlayerInventory>(entity)
                .expect("connected player inventory")
                .items
                .insert(1, 3);
            ecs.get_mut::<crate::game::player::PlayerUI>(entity)
                .expect("connected player UI")
                .current_window = Some("market:12:34:auc".to_owned());
        });
        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::parse("aucsetnum:1:100:2".to_owned()),
            },
        );
        assert_eq!(effects.saves.len(), 1);
        let effects = apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::AuctionOrderCreated {
                request: crate::game::AuctionOrderCreateRequest {
                    player_id,
                    session_id,
                    building_x: 12,
                    building_y: 34,
                    item_id: 1,
                    num: 2,
                    cost: 100,
                },
                result: crate::game::AuctionOrderCreateResult::PermanentFailure {
                    message: "db failure".to_owned(),
                },
            },
        );
        assert_eq!(
            test.state.query_player(player_id, |ecs, entity| {
                ecs.get::<crate::game::player::PlayerInventory>(entity)
                    .and_then(|inventory| inventory.items.get(&1).copied())
            }),
            Some(Some(3))
        );
        let packets = effects
            .events
            .into_iter()
            .find_map(|event| match event {
                crate::game::GameEvent::SessionBatch { packets, .. } => Some(packets),
                _ => None,
            })
            .expect("failure completion must emit refund packets");
        assert_eq!(
            packets.last().and_then(|packet| {
                openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(
                    packet.as_slice(),
                ))
                .ok()
                .flatten()
                .map(|packet| packet.event_name)
            }),
            Some(*b"OK")
        );
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn auction_bet_is_admitted_and_completion_preserves_money_then_order_wire() {
        let test = crate::test_support::ServerTestHarness::new("auction_bet", "auction-bet").await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(11);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        test.state.modify_player(player_id, |ecs, entity| {
            ecs.get_mut::<crate::game::player::PlayerStats>(entity)
                .expect("connected player stats")
                .money = 1_000;
            ecs.get_mut::<crate::game::player::PlayerUI>(entity)
                .expect("connected player UI")
                .current_window = Some("market:12:34:auc".to_owned());
        });

        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::parse("aucbet:42:100".to_owned()),
            },
        );
        assert!(matches!(
            effects.saves.as_slice(),
            [crate::game::SaveCommand::AuctionBet { request }]
                if request.order_id == 42
                    && request.requested_amount == Some(100)
                    && request.bidder_money == 1_000
        ));
        let effects = apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::AuctionBetCompleted {
                request: crate::game::AuctionBetRequest {
                    player_id,
                    session_id,
                    building_x: 12,
                    building_y: 34,
                    order_id: 42,
                    requested_amount: Some(100),
                    bidder_money: 1_000,
                },
                result: crate::game::AuctionBetResult::Won {
                    amount: 100,
                    previous_buyer_id: 0,
                    previous_cost: 50,
                    order: crate::db::orders::OrderRow {
                        id: 42,
                        initiator_id: 1,
                        item_id: 1,
                        num: 3,
                        cost: 100,
                        buyer_id: player_id.into(),
                        bet_time: crate::tasks::auction::now_unix(),
                    },
                    buyer_name: Some("auction-bet".to_owned()),
                },
            },
        );
        assert_eq!(
            test.state.query_player(player_id, |ecs, entity| {
                ecs.get::<crate::game::player::PlayerStats>(entity)
                    .map(|stats| stats.money)
            }),
            Some(Some(900))
        );
        let packets = effects
            .events
            .into_iter()
            .find_map(|event| match event {
                crate::game::GameEvent::SessionBatch { packets, .. } => Some(packets),
                _ => None,
            })
            .expect("bet completion must emit bidder packets");
        let decoded = packets
            .iter()
            .map(|packet| {
                openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(
                    packet.as_slice(),
                ))
                .expect("packet must decode")
                .expect("packet must be complete")
                .event_name
            })
            .collect::<Vec<_>>();
        assert_eq!(decoded, vec![*b"P$", *b"GU"]);
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn malformed_auction_bet_amount_reopens_order_through_typed_read() {
        let test = crate::test_support::ServerTestHarness::new(
            "auction_bet_malformed",
            "auction-bet-malformed",
        )
        .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(12);
        let _receiver = test.connect(session_id.get());
        test.state.modify_player(player_id, |ecs, entity| {
            ecs.get_mut::<crate::game::player::PlayerUI>(entity)
                .expect("connected player UI")
                .current_window = Some("market:12:34:auc".to_owned());
        });
        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::parse("aucbet:42:%I%".to_owned()),
            },
        );
        assert!(matches!(
            effects.saves.as_slice(),
            [crate::game::SaveCommand::AuctionOrder { request }]
                if request.order_id == 42
        ));
    }

    #[tokio::test]
    async fn chat_color_completion_delivers_only_to_the_current_session() {
        let test = crate::test_support::ServerTestHarness::new(
            "chat_color_completion_session_guard",
            "chat-color-player",
        )
        .await;
        let player_id = crate::game::PlayerId(test.player.id);
        let stale_session = crate::game::SessionId::new(101);
        let mut receiver = test.connect(stale_session.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);

        let effects = apply_player_command(
            &test.state,
            player_id,
            stale_session,
            crate::game::PlayerCommand::ChatSettings {
                payload: Bytes::from_static(b"_"),
            },
        );
        assert!(matches!(
            effects.saves.as_slice(),
            [crate::game::SaveCommand::ChatColorCycle {
                request: crate::game::ChatColorCycleRequest { player_id: id, session_id }
            }] if *id == player_id && *session_id == stale_session
        ));

        let effects = apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::ChatColorCycled {
                request: crate::game::ChatColorCycleRequest {
                    player_id,
                    session_id: stale_session,
                },
                result: crate::game::ChatColorCycleResult::Cycled { color: 3 },
            },
        );
        let [crate::game::GameEvent::SessionBatch { packets, .. }] = effects.events.as_slice()
        else {
            panic!("chat color completion must return one typed session batch");
        };
        let packet = openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(
            packets[0].as_slice(),
        ))
        .expect("chat color packet must decode")
        .expect("chat color packet must be complete");
        assert_eq!(packet.event_name, *b"mC");
        assert_eq!(packet.payload, &b"3"[..]);
        assert!(receiver.try_recv().is_err());

        let current_session = crate::game::SessionId::new(102);
        let mut current_receiver = test.connect(current_session.get());
        crate::test_support::ServerTestHarness::drain_events(&mut current_receiver);
        let effects = apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::ChatColorCycled {
                request: crate::game::ChatColorCycleRequest {
                    player_id,
                    session_id: stale_session,
                },
                result: crate::game::ChatColorCycleResult::Cycled { color: 4 },
            },
        );
        assert!(effects.events.is_empty());
        assert!(
            crate::test_support::ServerTestHarness::drain_events(&mut current_receiver).is_empty()
        );

        let effects = apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::ChatColorCycled {
                request: crate::game::ChatColorCycleRequest {
                    player_id,
                    session_id: current_session,
                },
                result: crate::game::ChatColorCycleResult::PermanentFailure {
                    message: "database unavailable".to_owned(),
                },
            },
        );
        let [crate::game::GameEvent::SessionBatch { packets, .. }] = effects.events.as_slice()
        else {
            panic!("chat color failure must return one typed session batch");
        };
        let packet = openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(
            packets[0].as_slice(),
        ))
        .expect("chat color error packet must decode")
        .expect("chat color error packet must be complete");
        assert_eq!(packet.event_name, *b"OK");
        assert_eq!(packet.payload, "Ошибка#Ошибка БД".as_bytes());
        assert!(current_receiver.try_recv().is_err());
    }
}

// === Pack operations (resp_bind, resp_fill, gun_fill, resp_profit, resp_save, pack_save) ===
