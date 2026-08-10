//! Typed dispatch for connection and movement commands.

use crate::game::{CommandEffects, GameState, PlayerCommand};
use std::sync::Arc;

pub(super) fn apply_session_command(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    command: PlayerCommand,
) -> CommandEffects {
    let mut effects = CommandEffects::default();
    match command {
        PlayerCommand::Connect { row } => {
            effects.append(crate::game::logic::player_init::connect_entity_in_tick(
                state, &row, session_id,
            ));
        }
        PlayerCommand::Disconnect => {
            effects.append(crate::game::logic::player_init::disconnect_in_tick(
                state, player_id, session_id,
            ));
        }
        PlayerCommand::Move {
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
