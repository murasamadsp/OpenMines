use crate::game::{GameState, PlayerId};
use crate::tasks::auction::now_unix;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub enum BonusClaim {
    Claimed {
        money: i64,
        creds: i64,
        reward_money: i64,
        cooldown_hours: i64,
        row: Box<crate::db::PlayerRow>,
    },
    NotReady {
        hours: i64,
        minutes: i64,
    },
    MissingState,
}

#[must_use]
pub fn bonus_available(last_bonus_at: i64, cooldown_secs: i64) -> bool {
    now_unix() - last_bonus_at >= cooldown_secs
}

pub fn claim_bonus(state: &Arc<GameState>, pid: PlayerId) -> BonusClaim {
    let now = now_unix();
    let bonus = state.config.gameplay.bonus;

    state
        .modify_player(pid, |ecs, entity| {
            let Some(player_stats) = ecs.get::<crate::game::player::PlayerStats>(entity) else {
                tracing::error!(player_id = %pid, component = "PlayerStats", "Player component missing for bonus claim");
                return Some(BonusClaim::MissingState);
            };
            if ecs
                .get::<crate::game::player::PlayerFlags>(entity)
                .is_none()
            {
                tracing::error!(player_id = %pid, component = "PlayerFlags", "Player component missing for bonus claim");
                return Some(BonusClaim::MissingState);
            }

            let last = player_stats.last_bonus_at;
            if now - last < bonus.cooldown_secs {
                let remaining = bonus.cooldown_secs - (now - last);
                return Some(BonusClaim::NotReady {
                    hours: remaining / 3600,
                    minutes: (remaining % 3600) / 60,
                });
            }

            {
                let mut changed_stats = ecs
                    .get_mut::<crate::game::player::PlayerStats>(entity)
                    .expect("PlayerStats checked before bonus mutation");
                changed_stats.money += bonus.reward_money;
                changed_stats.last_bonus_at = now;
            }
            ecs.get_mut::<crate::game::player::PlayerFlags>(entity)
                .expect("PlayerFlags checked before bonus mutation")
                .dirty = true;

            let row = crate::game::player::extract_player_row(ecs, entity)?;
            ecs.get_mut::<crate::game::player::PlayerFlags>(entity)
                .expect("PlayerFlags checked before bonus snapshot handoff")
                .dirty = false;
            let result_stats = ecs
                .get::<crate::game::player::PlayerStats>(entity)
                .expect("PlayerStats checked before bonus result");

            Some(BonusClaim::Claimed {
                money: result_stats.money,
                creds: result_stats.creds,
                reward_money: bonus.reward_money,
                cooldown_hours: bonus.cooldown_secs / 3600,
                row: Box::new(row),
            })
        })
        .flatten()
        .unwrap_or(BonusClaim::MissingState)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bonus_available_after_cooldown() {
        assert!(bonus_available(0, 1));
    }
}
