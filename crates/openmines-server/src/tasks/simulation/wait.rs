//! Pure planning for the next simulation owner wake.

use std::time::Instant;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct WaitInputs {
    pub(super) command_ready: bool,
    pub(super) completion_ready: bool,
    pub(super) persistence_backlog_ready: bool,
    pub(super) due_deadline: Option<Instant>,
    pub(super) schedule_deadline: Option<Instant>,
    pub(super) bots_render_deadline: Option<Instant>,
    pub(super) player_flush_deadline: Option<Instant>,
    pub(super) building_flush_deadline: Option<Instant>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WaitPlan {
    Immediate,
    Until(Instant),
    Indefinite,
}

#[must_use]
pub(super) fn plan_wait(now: Instant, inputs: WaitInputs) -> WaitPlan {
    if inputs.command_ready || inputs.completion_ready || inputs.persistence_backlog_ready {
        return WaitPlan::Immediate;
    }

    let earliest_deadline = [
        inputs.due_deadline,
        inputs.schedule_deadline,
        inputs.bots_render_deadline,
        inputs.player_flush_deadline,
        inputs.building_flush_deadline,
    ]
    .into_iter()
    .flatten()
    .min();

    match earliest_deadline {
        Some(deadline) if deadline <= now => WaitPlan::Immediate,
        Some(deadline) => WaitPlan::Until(deadline),
        None => WaitPlan::Indefinite,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn ready_command_or_backlog_runs_immediately() {
        let now = Instant::now();

        assert_eq!(
            plan_wait(
                now,
                WaitInputs {
                    command_ready: true,
                    ..WaitInputs::default()
                }
            ),
            WaitPlan::Immediate
        );
        assert_eq!(
            plan_wait(
                now,
                WaitInputs {
                    persistence_backlog_ready: true,
                    ..WaitInputs::default()
                }
            ),
            WaitPlan::Immediate
        );
    }

    #[test]
    fn persistence_blocked_work_does_not_create_a_busy_loop() {
        let now = Instant::now();
        let inputs = WaitInputs::default();

        assert_eq!(plan_wait(now, inputs), WaitPlan::Indefinite);
    }

    #[test]
    fn completion_runs_independently_from_persistence_capacity() {
        let now = Instant::now();
        let inputs = WaitInputs {
            completion_ready: true,
            ..WaitInputs::default()
        };

        assert_eq!(plan_wait(now, inputs), WaitPlan::Immediate);
    }

    #[test]
    fn deadline_at_or_before_now_runs_immediately() {
        let now = Instant::now();

        for deadline in [now.checked_sub(Duration::from_nanos(1)).unwrap(), now] {
            assert_eq!(
                plan_wait(
                    now,
                    WaitInputs {
                        due_deadline: Some(deadline),
                        ..WaitInputs::default()
                    }
                ),
                WaitPlan::Immediate
            );
        }
    }

    #[test]
    fn earliest_future_deadline_wins_across_all_sources() {
        let now = Instant::now();
        let earliest = now + Duration::from_secs(1);
        let inputs = WaitInputs {
            due_deadline: Some(now + Duration::from_secs(4)),
            schedule_deadline: Some(now + Duration::from_secs(3)),
            bots_render_deadline: Some(now + Duration::from_secs(2)),
            player_flush_deadline: Some(earliest),
            building_flush_deadline: Some(now + Duration::from_secs(5)),
            ..WaitInputs::default()
        };

        assert_eq!(plan_wait(now, inputs), WaitPlan::Until(earliest));
    }

    #[test]
    fn independent_deadline_still_wakes_persistence_blocked_work() {
        let now = Instant::now();
        let deadline = now + Duration::from_secs(2);
        let inputs = WaitInputs {
            schedule_deadline: Some(deadline),
            ..WaitInputs::default()
        };

        assert_eq!(plan_wait(now, inputs), WaitPlan::Until(deadline));
    }

    #[test]
    fn empty_inputs_wait_indefinitely() {
        let now = Instant::now();

        assert_eq!(plan_wait(now, WaitInputs::default()), WaitPlan::Indefinite);
    }
}
