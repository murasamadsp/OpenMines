//! Bounded drain and presentation adaptation for simulation-owned delayed work.

use crate::game::logic::consumables::{AreaConsumableApplyEffects, BoomApplyEffects};
use crate::game::logic::due::{DueAction, DueActionQueue};
use crate::game::{GameState, PlayerId};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(super) struct DuePhase {
    pub(super) effects: Vec<DueEffect>,
    pub(super) elapsed: Duration,
    pub(super) executed: usize,
}

pub(super) enum DueEffect {
    Boom(BoomApplyEffects),
    Protector(AreaConsumableApplyEffects),
    Raz(AreaConsumableApplyEffects),
}

impl DueEffect {
    pub(super) fn deaths(&self) -> &[PlayerId] {
        match self {
            Self::Boom(effect) => &effect.deaths,
            Self::Protector(effect) | Self::Raz(effect) => &effect.deaths,
        }
    }
}

pub(super) fn run_due_action_phase(state: &Arc<GameState>, queue: &mut DueActionQueue) -> DuePhase {
    run_due_action_phase_at(state, queue, Instant::now())
}

pub(super) fn run_due_action_phase_at(
    state: &Arc<GameState>,
    queue: &mut DueActionQueue,
    eligible_at: Instant,
) -> DuePhase {
    let started_at = Instant::now();
    let config = state.config.gameplay.simulation;
    let time_budget = Duration::from_micros(config.due_action_time_budget_us);
    let mut effects = Vec::new();
    let mut executed = 0;

    while executed < config.due_action_batch_budget
        && (executed == 0 || started_at.elapsed() < time_budget)
    {
        let Some(scheduled) = queue.pop_due(eligible_at) else {
            break;
        };
        let kind = scheduled.action.kind();
        crate::metrics::DUE_ACTION_LATENESS_SECONDS
            .with_label_values(&[kind])
            .observe(
                Instant::now()
                    .saturating_duration_since(scheduled.due_at)
                    .as_secs_f64(),
            );
        match scheduled.action {
            DueAction::Boom(action) => {
                let effect = crate::game::logic::consumables::apply_boom(state, action);
                effects.push(DueEffect::Boom(effect));
            }
            DueAction::Protector(action) => {
                let effect = crate::game::logic::consumables::apply_protector(state, action);
                effects.push(DueEffect::Protector(effect));
            }
            DueAction::Raz(action) => {
                let effect = crate::game::logic::consumables::apply_raz(state, action);
                effects.push(DueEffect::Raz(effect));
            }
        }
        crate::metrics::DUE_ACTIONS_TOTAL
            .with_label_values(&[kind, "executed"])
            .inc();
        executed += 1;
    }

    if queue
        .next_due_at()
        .is_some_and(|deadline| deadline <= eligible_at)
    {
        crate::metrics::DUE_ACTIONS_TOTAL
            .with_label_values(&["all", "budget_exhausted"])
            .inc();
    }
    crate::metrics::DUE_ACTION_DEPTH.set(i64::try_from(queue.len()).unwrap_or(i64::MAX));
    let elapsed = started_at.elapsed();
    crate::metrics::DUE_ACTION_DRAIN_SECONDS.observe(elapsed.as_secs_f64());

    DuePhase {
        effects,
        elapsed,
        executed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::WorldPos;
    use crate::world::WorldProvider;

    #[tokio::test]
    async fn phase_keeps_future_work_and_executes_due_work_once() {
        let test = crate::test_support::ServerTestHarness::new("due_phase", "due-phase-user").await;
        let center = WorldPos(10, 10);
        for x in (center.0 - 4)..=(center.0 + 4) {
            for y in (center.1 - 4)..=(center.1 + 4) {
                test.state.world.destroy_cell_and_road(x, y);
            }
        }
        test.state.world.set_cell_typed(
            center.0,
            center.1,
            crate::world::CellType(crate::world::cells::cell_type::ROCK),
        );
        test.state.put_consumable_pack(center.0, center.1, b'B', 0);

        let mut queue = DueActionQueue::new(2);
        let future_due_at = Instant::now() + Duration::from_mins(1);
        queue.try_reserve().unwrap().publish(
            future_due_at,
            DueAction::Boom(crate::game::logic::consumables::BoomDueAction {
                center: WorldPos(20, 20),
                rng_seed: 1,
            }),
        );
        let early = run_due_action_phase(&test.state, &mut queue);
        assert_eq!(early.executed, 0);
        assert_eq!(queue.next_due_at(), Some(future_due_at));

        queue.try_reserve().unwrap().publish(
            Instant::now(),
            DueAction::Boom(crate::game::logic::consumables::BoomDueAction {
                center,
                rng_seed: 2,
            }),
        );
        let due = run_due_action_phase(&test.state, &mut queue);

        assert_eq!(due.executed, 1);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.next_due_at(), Some(future_due_at));
        assert_eq!(test.state.world.get_solid_cell(center.0, center.1), 0);
        assert!(!test.state.consumable_packs.contains_key(&center));
        let [DueEffect::Boom(effect)] = due.effects.as_slice() else {
            panic!("due Boom must stay a typed effect until side-effect adaptation");
        };
        assert_eq!(effect.changed_cells, vec![center]);
        assert_eq!(effect.cleared_pack, center);
        assert!(effect.deaths.is_empty());
    }

    #[tokio::test]
    async fn batch_budget_carries_due_actions_in_admission_order() {
        let mut gameplay = crate::config::GameplayConfig::runtime_baseline();
        gameplay.simulation.due_action_batch_budget = 1;
        let test = crate::test_support::ServerTestHarness::with_gameplay(
            "due_batch_carry",
            "due-batch-user",
            gameplay,
        )
        .await;
        let first = WorldPos(10, 10);
        let second = WorldPos(20, 20);
        for center in [first, second] {
            for x in (center.0 - 4)..=(center.0 + 4) {
                for y in (center.1 - 4)..=(center.1 + 4) {
                    test.state.world.destroy_cell_and_road(x, y);
                }
            }
            test.state.world.set_cell_typed(
                center.0,
                center.1,
                crate::world::CellType(crate::world::cells::cell_type::ROCK),
            );
            test.state.put_consumable_pack(center.0, center.1, b'B', 0);
        }

        let mut queue = DueActionQueue::new(2);
        let due_at = Instant::now();
        for (center, rng_seed) in [(first, 1), (second, 2)] {
            queue.try_reserve().unwrap().publish(
                due_at,
                DueAction::Boom(crate::game::logic::consumables::BoomDueAction {
                    center,
                    rng_seed,
                }),
            );
        }

        let first_phase = run_due_action_phase(&test.state, &mut queue);
        assert_eq!(first_phase.executed, 1);
        assert_eq!(queue.len(), 1);
        let [DueEffect::Boom(first_effect)] = first_phase.effects.as_slice() else {
            panic!("first carried effect must be Boom");
        };
        assert_eq!(first_effect.cleared_pack, first);
        assert_eq!(test.state.world.get_solid_cell(first.0, first.1), 0);
        assert_eq!(
            test.state.world.get_solid_cell(second.0, second.1),
            crate::world::cells::cell_type::ROCK
        );

        let second_phase = run_due_action_phase(&test.state, &mut queue);
        assert_eq!(second_phase.executed, 1);
        assert_eq!(queue.len(), 0);
        let [DueEffect::Boom(second_effect)] = second_phase.effects.as_slice() else {
            panic!("second carried effect must be Boom");
        };
        assert_eq!(second_effect.cleared_pack, second);
        assert_eq!(test.state.world.get_solid_cell(second.0, second.1), 0);
    }

    #[tokio::test]
    async fn time_budget_carries_remaining_due_work() {
        let mut gameplay = crate::config::GameplayConfig::runtime_baseline();
        gameplay.simulation.due_action_batch_budget = 2;
        gameplay.simulation.due_action_time_budget_us = 1;
        let test = crate::test_support::ServerTestHarness::with_gameplay(
            "due_time_carry",
            "due-time-user",
            gameplay,
        )
        .await;
        let centers = [WorldPos(10, 10), WorldPos(20, 20)];
        for center in centers {
            test.state.world.set_cell_typed(
                center.0,
                center.1,
                crate::world::CellType(crate::world::cells::cell_type::ROCK),
            );
            test.state.put_consumable_pack(center.0, center.1, b'B', 0);
        }

        let mut queue = DueActionQueue::new(2);
        let due_at = Instant::now();
        for (center, rng_seed) in centers.into_iter().zip([1, 2]) {
            queue.try_reserve().unwrap().publish(
                due_at,
                DueAction::Boom(crate::game::logic::consumables::BoomDueAction {
                    center,
                    rng_seed,
                }),
            );
        }

        let first_phase = run_due_action_phase(&test.state, &mut queue);
        assert_eq!(first_phase.executed, 1);
        assert_eq!(queue.len(), 1);
        let second_phase = run_due_action_phase(&test.state, &mut queue);
        assert_eq!(second_phase.executed, 1);
        assert_eq!(queue.len(), 0);
    }
}
