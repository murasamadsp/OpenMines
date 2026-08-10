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

pub(super) mod completion;
pub(super) mod completion_clan;
pub(super) mod gui;
pub(super) mod slash;

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
        | PlayerCommand::Respawn
        | PlayerCommand::OpenBox) => {
            apply_gameplay_command(state, player_id, command);
            CommandEffects::default()
        }
        PlayerCommand::ClaimBonus => apply_bonus_claim(state, player_id),
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
        PlayerCommand::AdminAction => {
            gui::apply_presentation_command(state, player_id, &PlayerCommand::AdminAction);
            CommandEffects::default()
        }
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
            if let Some(tx) = state.player_sender(player_id) {
                handle_known_noop_ty(&tx, player_id, &event, &payload);
            }
            CommandEffects::default()
        }
        PlayerCommand::Slash { command } => {
            let context = crate::game::logic::kernel_context::KernelContext::new(state);
            slash::apply_slash_command(&context, player_id, session_id, command)
        }
    }
}

fn apply_session_command(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    command: PlayerCommand,
) -> CommandEffects {
    let mut effects = CommandEffects::default();
    match command {
        crate::game::PlayerCommand::Connect { row } => {
            effects.append(crate::game::logic::player_init::connect_entity_in_tick(
                state, &row, session_id,
            ));
        }
        crate::game::PlayerCommand::Disconnect => {
            effects.append(crate::game::logic::player_init::disconnect_in_tick(
                state, player_id, session_id,
            ));
        }
        crate::game::PlayerCommand::Move {
            time: _,
            x,
            y,
            direction,
            programmatic,
        } => {
            effects.append(crate::game::logic::movement::apply_move_command(
                state,
                player_id,
                session_id,
                crate::game::logic::movement::MoveRequest {
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

fn apply_gameplay_command(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    command: PlayerCommand,
) {
    match command {
        crate::game::PlayerCommand::Dig {
            direction,
            programmatic,
        } => {
            if let Some(tx) = state.player_sender(player_id) {
                crate::game::logic::dig_build::handle_dig(
                    state,
                    &tx,
                    player_id,
                    direction,
                    programmatic,
                );
            }
        }
        crate::game::PlayerCommand::Build {
            direction,
            block_type,
            programmatic,
        } => {
            if let Some(tx) = state.player_sender(player_id) {
                let bld = crate::protocol::packets::XbldClient {
                    direction,
                    block_type: &block_type,
                };
                crate::game::logic::dig_build::handle_build(
                    state,
                    &tx,
                    player_id,
                    &bld,
                    programmatic,
                );
            }
        }
        crate::game::PlayerCommand::Geology { programmatic } => {
            if let Some(tx) = state.player_sender(player_id) {
                apply_geology_command(state, &tx, player_id, programmatic);
            }
        }
        crate::game::PlayerCommand::Heal { programmatic } => {
            if let Some(tx) = state.player_sender(player_id) {
                apply_heal_command(state, &tx, player_id, programmatic);
            }
        }
        crate::game::PlayerCommand::Respawn => {
            crate::game::logic::death::request_death(state, player_id);
        }
        crate::game::PlayerCommand::OpenBox => {
            if let Some(tx) = state.player_sender(player_id) {
                crate::game::logic::buildings::handle_dpbx_crystal_box(state, &tx, player_id);
            }
        }
        _ => unreachable!("non-gameplay command routed to gameplay command handler"),
    }
}

fn apply_inventory_command(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    command: PlayerCommand,
    due_actions: &mut crate::game::logic::due::DueActionQueue,
) -> CommandEffects {
    match command {
        PlayerCommand::InventoryToggle => {
            if let Some(tx) = state.player_sender(player_id) {
                apply_inventory_result(
                    &tx,
                    player_id,
                    crate::game::logic::inventory::toggle_inventory(state, player_id),
                    "toggle",
                );
            }
            CommandEffects::default()
        }
        PlayerCommand::InventoryChoose { payload } => {
            if let Some(tx) = state.player_sender(player_id) {
                apply_inventory_result(
                    &tx,
                    player_id,
                    crate::game::logic::inventory::choose_inventory(state, player_id, &payload),
                    "choose",
                );
            }
            CommandEffects::default()
        }
        PlayerCommand::InventoryUse => {
            apply_inventory_use(state, session_id, player_id, due_actions)
        }
        PlayerCommand::ToggleAutoDig => {
            if let Some(tx) = state.player_sender(player_id) {
                apply_auto_dig_result(
                    &tx,
                    player_id,
                    crate::game::logic::settings::toggle_auto_dig(state, player_id),
                    "toggle",
                );
            }
            CommandEffects::default()
        }
        PlayerCommand::ToggleAggression => {
            if let Some(tx) = state.player_sender(player_id) {
                apply_aggression_result(
                    &tx,
                    player_id,
                    crate::game::logic::settings::toggle_aggression(state, player_id),
                    "toggle",
                );
            }
            CommandEffects::default()
        }
        PlayerCommand::SettingsSave { payload } => {
            if let Some(tx) = state.player_sender(player_id) {
                if !payload.is_empty() {
                    tracing::debug!(player_id = %player_id, bytes = payload.len(), "Sett TY payload ignored");
                }
                crate::net::session::ui::settings::open(state, &tx, player_id);
            }
            CommandEffects::default()
        }
        _ => unreachable!("non-inventory command routed to inventory command handler"),
    }
}

fn apply_inventory_use(
    state: &Arc<GameState>,
    session_id: crate::game::SessionId,
    player_id: crate::game::PlayerId,
    due_actions: &mut crate::game::logic::due::DueActionQueue,
) -> CommandEffects {
    if state
        .active_player_entity_for_session(player_id, session_id)
        .is_none()
    {
        return CommandEffects::default();
    }
    let Some(tx) = state.sessions.outbox_for_session(session_id) else {
        return CommandEffects::default();
    };
    let mut effects = CommandEffects::default();
    if crate::game::logic::heal_inventory::handle_inventory_use_sync_nonbuilding(
        state,
        &tx,
        player_id,
        session_id,
        due_actions,
        &mut effects.broadcasts,
    ) {
        return effects;
    }
    if let Some(placement) =
        crate::game::logic::heal_inventory::prepare_inventory_building_use(state, &tx, player_id)
    {
        spawn_inventory_building_insert_task(state, tx, placement);
    }
    effects
}

#[allow(clippy::too_many_lines)]
fn apply_chat_command(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    command: PlayerCommand,
) -> CommandEffects {
    match command {
        crate::game::PlayerCommand::LocalChat { message } => {
            apply_local_chat_command(state, player_id, session_id, message)
        }
        crate::game::PlayerCommand::ChannelChat { payload } => {
            apply_channel_chat_command(state, player_id, session_id, payload)
        }
        crate::game::PlayerCommand::ChatResync { payload } => {
            let channel_tag = state
                .query_player_opt(player_id, |w, e| {
                    w.get::<crate::game::player::PlayerUI>(e)
                        .map(|ui| ui.current_chat.clone())
                })
                .unwrap_or_default();
            let (channel_tag, last_id) = match crate::game::logic::chat::parse_chin_resync_payload(
                String::from_utf8_lossy(&payload).trim(),
            ) {
                Some(crate::game::logic::chat::ChinResync::Incremental { current, lastid }) => {
                    (current, lastid)
                }
                _ => (channel_tag, 0),
            };
            CommandEffects {
                events: Vec::new(),
                saves: vec![crate::game::SaveCommand::ChatResync {
                    request: crate::game::ChatResyncRequest {
                        player_id,
                        session_id,
                        channel_tag,
                        last_id,
                    },
                }],
                broadcasts: Vec::new(),
            }
        }
        crate::game::PlayerCommand::ChatMenu { .. } => CommandEffects {
            events: Vec::new(),
            saves: vec![crate::game::SaveCommand::ChatMenu {
                request: crate::game::ChatMenuRequest {
                    player_id,
                    session_id,
                },
            }],
            broadcasts: Vec::new(),
        },
        crate::game::PlayerCommand::ChatChoose { payload } => {
            let tag = String::from_utf8_lossy(&payload).trim().to_string();
            let channel_tag = if tag.is_empty() {
                state
                    .query_player_opt(player_id, |w, e| {
                        w.get::<crate::game::player::PlayerUI>(e)
                            .map(|ui| ui.current_chat.clone())
                    })
                    .unwrap_or_default()
            } else {
                tag
            };
            if !channel_tag.is_empty() {
                let _ = state.modify_player(player_id, |w, e| {
                    if let Some(mut ui) = w.get_mut::<crate::game::player::PlayerUI>(e) {
                        ui.current_chat = channel_tag.clone();
                    }
                });
            }
            CommandEffects {
                events: Vec::new(),
                saves: vec![crate::game::SaveCommand::ChatMenu {
                    request: crate::game::ChatMenuRequest {
                        player_id,
                        session_id,
                    },
                }],
                broadcasts: Vec::new(),
            }
        }
        crate::game::PlayerCommand::ChatSettings { .. } => CommandEffects {
            events: Vec::new(),
            saves: vec![crate::game::SaveCommand::ChatColorCycle {
                request: crate::game::ChatColorCycleRequest {
                    player_id,
                    session_id,
                },
            }],
            broadcasts: Vec::new(),
        },
        crate::game::PlayerCommand::ChatPrivate { payload } => {
            let target_uid = String::from_utf8_lossy(&payload).trim().parse::<i32>().ok();
            match target_uid {
                Some(uid) => CommandEffects {
                    events: Vec::new(),
                    saves: vec![crate::game::SaveCommand::ChatPrivate {
                        request: crate::game::ChatPrivateRequest {
                            player_id,
                            session_id,
                            target_uid: crate::game::PlayerId::from(uid),
                        },
                    }],
                    broadcasts: Vec::new(),
                },
                None => CommandEffects::default(),
            }
        }
        crate::game::PlayerCommand::Whois { ids } => {
            let online_names = state
                .active_player_ids()
                .into_iter()
                .filter_map(|pid| {
                    state.query_player_opt(pid, |ecs, e| {
                        ecs.get::<crate::game::player::PlayerMetadata>(e)
                            .map(|m| (pid.as_i32(), m.name.clone()))
                    })
                })
                .collect();
            CommandEffects {
                events: Vec::new(),
                saves: vec![crate::game::SaveCommand::Whois {
                    request: crate::game::WhoisRequest {
                        player_id,
                        session_id,
                        ids,
                        online_names,
                    },
                }],
                broadcasts: Vec::new(),
            }
        }
        _ => unreachable!("non-chat command routed to chat command handler"),
    }
}

fn apply_local_chat_command(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    message: String,
) -> CommandEffects {
    let effects = CommandEffects::default();
    let Some(tx) = state.player_sender(player_id) else {
        return effects;
    };
    if !state.check_chat_rate(player_id) {
        tracing::debug!(player_id = %player_id, "chat rate limited (Locl)");
        return effects;
    }
    if crate::game::logic::chat::handle_local_chat_non_command(state, &tx, player_id, &message) {
        return effects;
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
    let Some(tx) = state.player_sender(player_id) else {
        return effects;
    };
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
        crate::game::logic::chat::prepare_channel_chat_non_command(state, &tx, player_id, &text)
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
            if state.sessions.session_for_player(player_id) == Some(session_id)
                && let Some(tx) = state.sessions.outbox_for_session(session_id)
            {
                match event.as_str() {
                    "PROG" => {
                        if let Some(save) =
                            prepare_program_save(tx, player_id, session_id, &payload)
                        {
                            effects.saves.push(save);
                        }
                    }
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
                    "pRST" => crate::game::logic::misc::handle_prog_reset_ty(state, &tx, player_id),
                    "PREN" => crate::game::logic::misc::handle_prog_rename_prompt_ty(
                        state, &tx, player_id, &payload,
                    ),
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
            completion::apply_program_editor_completion(state, session_id, player_id, command);
        }
        _ => unreachable!("non-program command routed to program command handler"),
    }
    effects
}

fn prepare_program_save(
    tx: crate::net::session::outbox::Outbox,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    payload: &[u8],
) -> Option<crate::game::SaveCommand> {
    let (program_id, source) = decode_program_save(&tx, player_id, payload)?;
    if program_id <= 0 {
        tracing::warn!(
            player_id = %player_id,
            program_id,
            "PROG received no selected client program; opening program list"
        );
        return Some(crate::game::SaveCommand::ProgramMenu {
            request: crate::game::ProgramMenuRequest {
                player_id,
                session_id,
            },
        });
    }
    Some(crate::game::SaveCommand::Program {
        request: crate::game::ProgramSaveRequest {
            player_id,
            session_id,
            program_id,
            source,
        },
    })
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

fn apply_inventory_result(
    tx: &dyn crate::net::session::wire::PacketSink,
    player_id: crate::game::PlayerId,
    result: crate::game::logic::inventory::InventoryMutation,
    action: &'static str,
) {
    match result {
        crate::game::logic::inventory::InventoryMutation::Packets(packets) => {
            for (event, payload) in packets {
                tx.send_packet(crate::net::session::wire::make_u_packet_bytes(
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

fn send_inventory_state_error(tx: &dyn crate::net::session::wire::PacketSink) {
    let packet =
        crate::protocol::packets::ok_message("ИНВЕНТАРЬ", "Состояние инвентаря недоступно.");
    tx.send_packet(crate::net::session::wire::make_u_packet_bytes(
        packet.0, &packet.1,
    ));
}

fn handle_known_noop_ty(
    tx: &dyn crate::net::session::wire::PacketSink,
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
            crate::game::logic::commands_social::send_ok(
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

fn apply_bonus_claim(state: &Arc<GameState>, player_id: crate::game::PlayerId) -> CommandEffects {
    let Some(tx) = state.player_sender(player_id) else {
        return CommandEffects::default();
    };
    let mut effects = CommandEffects::default();
    match crate::game::logic::bonus::claim_bonus(state, player_id) {
        crate::game::logic::bonus::BonusClaim::Claimed {
            money: new_money,
            creds,
            reward_money,
            cooldown_hours,
            row,
        } => {
            crate::net::session::wire::send_u_packet(
                &tx,
                "P$",
                &crate::protocol::packets::money(new_money, creds).1,
            );
            crate::net::session::wire::send_u_packet(&tx, "DR", b"0");
            crate::game::logic::commands_social::send_ok(
                &tx,
                "Бонус",
                &format!(
                    "Вы получили {reward_money}$!\nВозвращайтесь через {cooldown_hours} часов."
                ),
            );

            effects.saves.push(crate::game::SaveCommand::Player { row });
        }
        crate::game::logic::bonus::BonusClaim::NotReady { hours, minutes } => {
            crate::game::logic::commands_social::send_ok(
                &tx,
                "Бонус",
                &format!("Бонус ещё не готов.\nПриходите через {hours}ч {minutes}м."),
            );
        }
        crate::game::logic::bonus::BonusClaim::MissingState => {
            crate::net::session::wire::send_u_packet(
                &tx,
                "OK",
                &crate::protocol::packets::ok_message("Бонус", "Состояние бонуса недоступно.").1,
            );
        }
    }
    effects
}

pub(super) fn apply_market_sell(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    sliders: &[i64; 6],
    building_x: i32,
    building_y: i32,
) -> CommandEffects {
    let batch = crate::net::session::wire::PacketBatch::default();

    let Some(building_entity) = state.building_entity_at(building_x, building_y) else {
        crate::net::session::wire::send_u_packet(
            &batch,
            "OK",
            &crate::protocol::packets::ok_message("РЫНОК", "Здание рынка не найдено.").1,
        );
        return CommandEffects {
            events: vec![crate::game::GameEvent::SessionBatch {
                session_id,
                player_id,
                packets: batch.into_packets(),
            }],
            ..CommandEffects::default()
        };
    };

    // Validate building has required components
    {
        let ecs = state.ecs_read_profiled("market.sell_check");
        if ecs
            .get::<crate::game::buildings::BuildingFlags>(building_entity)
            .is_none()
            || ecs
                .get::<crate::game::buildings::BuildingStorage>(building_entity)
                .is_none()
        {
            crate::net::session::wire::send_u_packet(
                &batch,
                "OK",
                &crate::protocol::packets::ok_message("РЫНОК", "Состояние здания недоступно.").1,
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
    }

    let Some(outcome) = crate::game::economy::market::sell_crystals(state, player_id, sliders)
    else {
        crate::net::session::wire::send_u_packet(
            &batch,
            "OK",
            &crate::protocol::packets::ok_message("РЫНОК", "Невозможно продать кристаллы.").1,
        );
        return CommandEffects {
            events: vec![crate::game::GameEvent::SessionBatch {
                session_id,
                player_id,
                packets: batch.into_packets(),
            }],
            ..CommandEffects::default()
        };
    };

    // Update building moneyinside
    {
        let mut ecs = state.ecs_write_profiled("market.sell_building");
        if let Some(mut storage) =
            ecs.get_mut::<crate::game::buildings::BuildingStorage>(building_entity)
        {
            storage.money += outcome.earned / 10;
        }
        if let Some(mut flags) =
            ecs.get_mut::<crate::game::buildings::BuildingFlags>(building_entity)
        {
            flags.dirty = true;
        }
    }
    state.mark_building_dirty(building_entity);

    crate::net::session::wire::send_u_packet(
        &batch,
        "@B",
        &crate::protocol::packets::basket(&outcome.crystals, 1).1,
    );
    crate::net::session::wire::send_u_packet(
        &batch,
        "P$",
        &crate::protocol::packets::money(outcome.money, outcome.creds).1,
    );

    // Re-render sell tab
    if let Some(view) = state.get_pack_at(building_x, building_y) {
        crate::game::logic::gui::market_gui::open_market_gui(
            state, &batch, player_id, &view, "sellcrys",
        );
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

pub(super) fn apply_market_sell_all(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    building_x: i32,
    building_y: i32,
) -> CommandEffects {
    let batch = crate::net::session::wire::PacketBatch::default();

    let Some(building_entity) = state.building_entity_at(building_x, building_y) else {
        crate::net::session::wire::send_u_packet(
            &batch,
            "OK",
            &crate::protocol::packets::ok_message("РЫНОК", "Здание рынка не найдено.").1,
        );
        return CommandEffects {
            events: vec![crate::game::GameEvent::SessionBatch {
                session_id,
                player_id,
                packets: batch.into_packets(),
            }],
            ..CommandEffects::default()
        };
    };

    let Some(outcome) = crate::game::economy::market::sell_all_crystals(state, player_id) else {
        crate::net::session::wire::send_u_packet(
            &batch,
            "OK",
            &crate::protocol::packets::ok_message("РЫНОК", "Невозможно продать кристаллы.").1,
        );
        return CommandEffects {
            events: vec![crate::game::GameEvent::SessionBatch {
                session_id,
                player_id,
                packets: batch.into_packets(),
            }],
            ..CommandEffects::default()
        };
    };

    // Update building moneyinside
    {
        let mut ecs = state.ecs_write_profiled("market.sell_all_building");
        if let Some(mut storage) =
            ecs.get_mut::<crate::game::buildings::BuildingStorage>(building_entity)
        {
            storage.money += outcome.earned / 10;
        }
        if let Some(mut flags) =
            ecs.get_mut::<crate::game::buildings::BuildingFlags>(building_entity)
        {
            flags.dirty = true;
        }
    }
    state.mark_building_dirty(building_entity);

    crate::net::session::wire::send_u_packet(
        &batch,
        "@B",
        &crate::protocol::packets::basket(&outcome.crystals, 1).1,
    );
    crate::net::session::wire::send_u_packet(
        &batch,
        "P$",
        &crate::protocol::packets::money(outcome.money, outcome.creds).1,
    );

    // Re-render sell tab
    if let Some(view) = state.get_pack_at(building_x, building_y) {
        crate::game::logic::gui::market_gui::open_market_gui(
            state, &batch, player_id, &view, "sellcrys",
        );
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

pub(super) fn apply_market_buy(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    sliders: &[i64; 6],
    building_x: i32,
    building_y: i32,
) -> CommandEffects {
    let batch = crate::net::session::wire::PacketBatch::default();

    let Some(outcome) = crate::game::economy::market::buy_crystals(state, player_id, sliders)
    else {
        crate::net::session::wire::send_u_packet(
            &batch,
            "OK",
            &crate::protocol::packets::ok_message("РЫНОК", "Невозможно купить кристаллы.").1,
        );
        return CommandEffects {
            events: vec![crate::game::GameEvent::SessionBatch {
                session_id,
                player_id,
                packets: batch.into_packets(),
            }],
            ..CommandEffects::default()
        };
    };

    crate::net::session::wire::send_u_packet(
        &batch,
        "@B",
        &crate::protocol::packets::basket(&outcome.crystals, 1).1,
    );
    crate::net::session::wire::send_u_packet(
        &batch,
        "P$",
        &crate::protocol::packets::money(outcome.money, outcome.creds).1,
    );

    // Re-render buy tab
    if let Some(view) = state.get_pack_at(building_x, building_y) {
        crate::game::logic::gui::market_gui::open_market_gui(
            state, &batch, player_id, &view, "buycrys",
        );
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

pub(super) fn apply_market_get_profit(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    building_x: i32,
    building_y: i32,
) -> CommandEffects {
    let batch = crate::net::session::wire::PacketBatch::default();

    // Validate building is market and player is owner
    let Some(view) = state.get_pack_at(building_x, building_y) else {
        return CommandEffects::default();
    };
    if view.pack_type != crate::game::structures::buildings::PackType::Market
        || view.owner_id != player_id
    {
        return CommandEffects::default();
    }

    // Validate player and building state
    let player_ready = state
        .query_player(player_id, |ecs, entity| {
            ecs.get::<crate::game::player::PlayerStats>(entity)
                .is_some()
                && ecs
                    .get::<crate::game::player::PlayerFlags>(entity)
                    .is_some()
        })
        .unwrap_or(false);
    if !player_ready {
        crate::net::session::wire::send_u_packet(
            &batch,
            "OK",
            &crate::protocol::packets::ok_message("РЫНОК", "Состояние игрока недоступно.").1,
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

    // Transfer profit from building to player
    let mut amount = 0i64;
    let updated = match crate::game::logic::buildings::modify_pack_with_db(
        state,
        building_x,
        building_y,
        |ecs, entity| {
            let mut storage = ecs
                .get_mut::<crate::game::buildings::BuildingStorage>(entity)
                .expect("BuildingStorage checked before market profit mutation");
            amount = storage.money;
            storage.money = 0;
            true
        },
    ) {
        Ok(updated) => updated,
        Err(e) => {
            tracing::error!(x = building_x, y = building_y, error = %e, "Market profit withdrawal failed");
            crate::net::session::wire::send_u_packet(
                &batch,
                "OK",
                &crate::protocol::packets::ok_message("РЫНОК", "Ошибка снятия прибыли.").1,
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
    };
    if !updated {
        crate::net::session::wire::send_u_packet(
            &batch,
            "OK",
            &crate::protocol::packets::ok_message("РЫНОК", "Здание не найдено.").1,
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

    if amount > 0 {
        let result = state.modify_player(player_id, |ecs, entity| {
            let mut s = ecs.get_mut::<crate::game::player::PlayerStats>(entity)?;
            s.money = s.money.saturating_add(amount);
            let money_now = s.money;
            let creds_now = s.creds;
            let mut f = ecs.get_mut::<crate::game::player::PlayerFlags>(entity)?;
            f.dirty = true;
            Some((money_now, creds_now))
        });
        if let Some(Some((money_now, creds_now))) = result {
            crate::net::session::wire::send_u_packet(
                &batch,
                "P$",
                &crate::protocol::packets::money(money_now, creds_now).1,
            );
        }
    }

    // Re-open admin page with updated profit (now 0)
    crate::game::logic::gui::market_gui::open_market_admin_gui(
        state, &batch, player_id, building_x, building_y,
    );

    CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: batch.into_packets(),
        }],
        ..CommandEffects::default()
    }
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

fn decode_program_save(
    tx: &dyn crate::net::session::wire::PacketSink,
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
                task_state
                    .enqueue_internal(
                        placement.owner_id,
                        session_id,
                        crate::game::PlayerCommand::ApplyInventoryBuildingPlaced {
                            placement,
                            db_id,
                        },
                    )
                    .await;
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
                task_state
                    .enqueue_internal(
                        placement.owner_id,
                        session_id,
                        crate::game::PlayerCommand::ApplyPaidBuildingPlaced { placement, db_id },
                    )
                    .await;
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
                task_state
                    .enqueue_internal(
                        placement.owner_id,
                        session_id,
                        crate::game::PlayerCommand::RefundPaidBuildingPlacement {
                            cost: placement.cost,
                        },
                    )
                    .await;
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

    CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: batch.into_packets(),
        }],
        ..CommandEffects::default()
    }
}

pub(super) fn apply_resp_fill(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    amount_str: &str,
    pack_x: i32,
    pack_y: i32,
) -> CommandEffects {
    let batch = crate::net::session::wire::PacketBatch::default();
    crate::game::logic::packs::handle_resp_fill(
        state, &batch, player_id, amount_str, pack_x, pack_y,
    );
    CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: batch.into_packets(),
        }],
        ..CommandEffects::default()
    }
}

pub(super) fn apply_gun_fill(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    amount_str: &str,
    pack_x: i32,
    pack_y: i32,
) -> CommandEffects {
    let batch = crate::net::session::wire::PacketBatch::default();
    crate::game::logic::packs::handle_gun_fill(
        state, &batch, player_id, amount_str, pack_x, pack_y,
    );
    CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: batch.into_packets(),
        }],
        ..CommandEffects::default()
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
    crate::game::logic::packs::handle_resp_profit(state, &batch, player_id, pack_x, pack_y);
    CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: batch.into_packets(),
        }],
        ..CommandEffects::default()
    }
}

pub(super) fn apply_resp_save(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    richlist_data: &str,
) -> CommandEffects {
    let batch = crate::net::session::wire::PacketBatch::default();
    crate::game::logic::packs::handle_resp_save(state, &batch, player_id, richlist_data);
    CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: batch.into_packets(),
        }],
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
    crate::game::logic::up_building::handle_up_button(state, &batch, player_id, button);
    CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: batch.into_packets(),
        }],
        ..CommandEffects::default()
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_persistence_completion, apply_player_command};
    use bytes::Bytes;

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

        apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::ChatColorCycled {
                request: crate::game::ChatColorCycleRequest {
                    player_id,
                    session_id: stale_session,
                },
                result: crate::game::ChatColorCycleResult::Cycled { color: 3 },
            },
        );
        assert_eq!(
            crate::test_support::ServerTestHarness::drain_events(&mut receiver),
            vec![("mC".to_owned(), b"3".to_vec())]
        );

        let current_session = crate::game::SessionId::new(102);
        let mut current_receiver = test.connect(current_session.get());
        crate::test_support::ServerTestHarness::drain_events(&mut current_receiver);
        apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::ChatColorCycled {
                request: crate::game::ChatColorCycleRequest {
                    player_id,
                    session_id: stale_session,
                },
                result: crate::game::ChatColorCycleResult::Cycled { color: 4 },
            },
        );
        assert!(
            crate::test_support::ServerTestHarness::drain_events(&mut current_receiver).is_empty()
        );
    }
}

// === Pack operations (resp_bind, resp_fill, gun_fill, resp_profit, resp_save, pack_save) ===
