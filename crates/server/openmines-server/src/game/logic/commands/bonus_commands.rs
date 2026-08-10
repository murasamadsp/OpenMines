//! Typed dispatch for the legacy daily bonus command.

use crate::game::{CommandEffects, GameState};
use std::sync::Arc;

pub(super) fn apply_bonus_claim(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
) -> CommandEffects {
    if state.sessions.session_for_player(player_id) != Some(session_id) {
        return CommandEffects::default();
    }
    let batch = crate::net::session::wire::PacketBatch::default();
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
                &batch,
                "P$",
                &crate::protocol::packets::money(new_money, creds).1,
            );
            crate::net::session::wire::send_u_packet(&batch, "DR", b"0");
            crate::game::logic::commands_social::send_ok(
                &batch,
                "Бонус",
                &format!(
                    "Вы получили {reward_money}$!\nВозвращайтесь через {cooldown_hours} часов."
                ),
            );
            effects.saves.push(crate::game::SaveCommand::Player { row });
        }
        crate::game::logic::bonus::BonusClaim::NotReady { hours, minutes } => {
            crate::game::logic::commands_social::send_ok(
                &batch,
                "Бонус",
                &format!("Бонус ещё не готов.\nПриходите через {hours}ч {minutes}м."),
            );
        }
        crate::game::logic::bonus::BonusClaim::MissingState => {
            crate::net::session::wire::send_u_packet(
                &batch,
                "OK",
                &crate::protocol::packets::ok_message("Бонус", "Состояние бонуса недоступно.").1,
            );
        }
    }
    effects.events.push(crate::game::GameEvent::SessionBatch {
        session_id,
        player_id,
        packets: batch.into_packets(),
    });
    effects
}
