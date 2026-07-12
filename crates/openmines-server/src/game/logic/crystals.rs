use crate::game::{GameState, PlayerId};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum CrystalKind {
    Green = 0,
    Blue = 1,
    Red = 2,
    Violet = 3,
    White = 4,
    Cyan = 5,
}

impl CrystalKind {
    pub const fn index(self) -> usize {
        self as usize
    }

    pub fn from_programmator_variable(code: &str) -> Option<(Self, bool)> {
        match code {
            "G" => Some((Self::Green, false)),
            "B" => Some((Self::Blue, false)),
            "R" => Some((Self::Red, false)),
            "V" => Some((Self::Violet, false)),
            "W" => Some((Self::White, false)),
            "C" => Some((Self::Cyan, false)),
            "GP" => Some((Self::Green, true)),
            "BP" => Some((Self::Blue, true)),
            "RP" => Some((Self::Red, true)),
            "VP" => Some((Self::Violet, true)),
            "WP" => Some((Self::White, true)),
            "CP" => Some((Self::Cyan, true)),
            _ => None,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum CrystalSpendResult {
    Spent { crystals: [i64; 6] },
    Insufficient,
    MissingState(&'static str),
    MissingEntity,
}

pub fn spend_crystal(
    state: &Arc<GameState>,
    pid: PlayerId,
    idx: usize,
    amount: i64,
) -> CrystalSpendResult {
    state
        .modify_player(pid, |ecs, entity| {
            if ecs
                .get::<crate::game::player::PlayerStats>(entity)
                .is_none()
            {
                return Some(CrystalSpendResult::MissingState("PlayerStats"));
            }
            if ecs
                .get::<crate::game::player::PlayerFlags>(entity)
                .is_none()
            {
                return Some(CrystalSpendResult::MissingState("PlayerFlags"));
            }

            let mut player_stats = ecs
                .get_mut::<crate::game::player::PlayerStats>(entity)
                .expect("PlayerStats checked before crystal spend");
            if player_stats.crystals[idx] < amount {
                return Some(CrystalSpendResult::Insufficient);
            }
            player_stats.crystals[idx] -= amount;
            let crystals = player_stats.crystals;
            ecs.get_mut::<crate::game::player::PlayerFlags>(entity)
                .expect("PlayerFlags checked before crystal spend")
                .dirty = true;

            Some(CrystalSpendResult::Spent { crystals })
        })
        .flatten()
        .unwrap_or(CrystalSpendResult::MissingEntity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ServerTestHarness;

    async fn make_crystal_test_state(label: &str) -> ServerTestHarness {
        let test = ServerTestHarness::new(&format!("crystal_{label}"), "crystal-user").await;
        let mut rx = test.connect(1);
        while rx.try_recv().is_ok() {}
        test
    }

    #[tokio::test]
    async fn spend_crystal_updates_crystals_and_marks_dirty() {
        let test = make_crystal_test_state("spend").await;
        let pid = PlayerId(test.player.id);
        test.state.modify_player(pid, |ecs, entity| {
            ecs.get_mut::<crate::game::player::PlayerStats>(entity)
                .expect("test player stats")
                .crystals[0] = 3;
        });

        let result = spend_crystal(&test.state, pid, 0, 2);

        assert_eq!(
            result,
            CrystalSpendResult::Spent {
                crystals: [1, 0, 0, 0, 0, 0],
            }
        );
        let (crystals, dirty) = test
            .state
            .query_player_opt(pid, |ecs, entity| {
                let stats = ecs.get::<crate::game::player::PlayerStats>(entity)?;
                let flags = ecs.get::<crate::game::player::PlayerFlags>(entity)?;
                Some((stats.crystals, flags.dirty))
            })
            .unwrap();
        assert_eq!(crystals[0], 1);
        assert!(dirty);
    }

    #[tokio::test]
    async fn spend_crystal_insufficient_is_silent_without_dirty() {
        let test = make_crystal_test_state("insufficient").await;
        let pid = PlayerId(test.player.id);
        let result = spend_crystal(&test.state, pid, 0, 1);

        assert_eq!(result, CrystalSpendResult::Insufficient);
        let dirty = test
            .state
            .query_player_opt(pid, |ecs, entity| {
                Some(ecs.get::<crate::game::player::PlayerFlags>(entity)?.dirty)
            })
            .unwrap();
        assert!(!dirty);
    }
}
