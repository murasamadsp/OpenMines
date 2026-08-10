//! Typed dispatch and output collection for gameplay commands.

use super::{apply_geology_command, apply_heal_command};
use crate::game::{CommandEffects, GameState, PlayerCommand};
use std::sync::Arc;

pub(super) fn apply_gameplay_command(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    command: PlayerCommand,
) -> CommandEffects {
    match command {
        PlayerCommand::Dig {
            direction,
            programmatic,
        } => apply_gameplay_output(state, session_id, player_id, |batch| {
            crate::game::logic::dig_build::handle_dig(
                state,
                batch,
                player_id,
                direction,
                programmatic,
            );
        }),
        PlayerCommand::Build {
            direction,
            block_type,
            programmatic,
        } => apply_gameplay_output(state, session_id, player_id, |batch| {
            let bld = crate::protocol::packets::XbldClient {
                direction,
                block_type: &block_type,
            };
            crate::game::logic::dig_build::handle_build(
                state,
                batch,
                player_id,
                &bld,
                programmatic,
            );
        }),
        PlayerCommand::Geology { programmatic } => {
            apply_gameplay_output(state, session_id, player_id, |batch| {
                apply_geology_command(state, batch, player_id, programmatic);
            })
        }
        PlayerCommand::Heal { programmatic } => {
            apply_gameplay_output(state, session_id, player_id, |batch| {
                apply_heal_command(state, batch, player_id, programmatic);
            })
        }
        PlayerCommand::Respawn => {
            crate::game::logic::death::request_death(state, player_id);
            CommandEffects::default()
        }
        _ => unreachable!("non-gameplay command routed to gameplay command handler"),
    }
}

fn apply_gameplay_output<F>(
    state: &Arc<GameState>,
    session_id: crate::game::SessionId,
    player_id: crate::game::PlayerId,
    apply: F,
) -> CommandEffects
where
    F: FnOnce(&crate::net::session::wire::PacketBatch),
{
    if state.sessions.session_for_player(player_id) != Some(session_id) {
        return CommandEffects::default();
    }
    let batch = crate::net::session::wire::PacketBatch::default();
    apply(&batch);
    let packets = batch.into_packets();
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
