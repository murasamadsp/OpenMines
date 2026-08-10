//! Typed dispatch for chat state commands.

use super::{apply_channel_chat_command, apply_local_chat_command};
use crate::game::{CommandEffects, GameState, PlayerCommand};
use std::sync::Arc;

pub(super) fn apply_chat_command(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    command: PlayerCommand,
) -> CommandEffects {
    match command {
        PlayerCommand::LocalChat { message } => {
            apply_local_chat_command(state, player_id, session_id, message)
        }
        PlayerCommand::ChannelChat { payload } => {
            apply_channel_chat_command(state, player_id, session_id, payload)
        }
        PlayerCommand::ChatResync { payload } => {
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
        PlayerCommand::ChatMenu { .. } => CommandEffects {
            events: Vec::new(),
            saves: vec![crate::game::SaveCommand::ChatMenu {
                request: crate::game::ChatMenuRequest {
                    player_id,
                    session_id,
                },
            }],
            broadcasts: Vec::new(),
        },
        PlayerCommand::ChatChoose { payload } => {
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
            CommandEffects {
                events: Vec::new(),
                saves: vec![crate::game::SaveCommand::ChatResync {
                    request: crate::game::ChatResyncRequest {
                        player_id,
                        session_id,
                        channel_tag,
                        last_id: 0,
                    },
                }],
                broadcasts: Vec::new(),
            }
        }
        PlayerCommand::ChatSettings { .. } => CommandEffects {
            events: Vec::new(),
            saves: vec![crate::game::SaveCommand::ChatColorCycle {
                request: crate::game::ChatColorCycleRequest {
                    player_id,
                    session_id,
                },
            }],
            broadcasts: Vec::new(),
        },
        PlayerCommand::ChatPrivate { payload } => {
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
        PlayerCommand::Whois { ids } => {
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
