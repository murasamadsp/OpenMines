//! Selection and execution of configured Bevy schedules.

use super::profiler::{ScheduleRunProfile, ScheduleTailProfile, SimProfile, empty_sim_profile};
use super::{TickHeartbeat, TickStage};
use crate::game::{GameState, ScheduleActivity};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(super) struct ScheduleTickResult {
    pub(super) broadcasts: Vec<crate::game::BroadcastEffect>,
    pub(super) programmator_actions: Vec<crate::game::ProgrammatorAction>,
    pub(super) cell_conversions: Vec<crate::game::PendingConversion>,
    pub(super) pack_resends: Vec<(i32, i32)>,
    pub(super) sched_select: Duration,
    pub(super) sched_lock_wait: Duration,
    pub(super) sched_run: Duration,
    pub(super) schedule_tail_profile: ScheduleTailProfile,
    pub(super) schedule_runs: Vec<ScheduleRunProfile>,
    pub(super) sim_profile: SimProfile,
}

struct ScheduleTailOutput {
    broadcasts: Vec<crate::game::BroadcastEffect>,
    programmator_actions: Vec<crate::game::ProgrammatorAction>,
    cell_conversions: Vec<crate::game::PendingConversion>,
    pack_resends: Vec<(i32, i32)>,
    profile: ScheduleTailProfile,
    sim_profile: SimProfile,
}

impl ScheduleTickResult {
    fn idle(
        sched_select: Duration,
        player_entity_count: usize,
        online_count: usize,
        schedule_runs: Vec<ScheduleRunProfile>,
    ) -> Self {
        Self {
            broadcasts: Vec::new(),
            programmator_actions: Vec::new(),
            cell_conversions: Vec::new(),
            pack_resends: Vec::new(),
            sched_select,
            sched_lock_wait: Duration::ZERO,
            sched_run: Duration::ZERO,
            schedule_tail_profile: ScheduleTailProfile::default(),
            schedule_runs,
            sim_profile: empty_sim_profile(player_entity_count, online_count),
        }
    }
}

pub(super) struct ScheduleClock {
    pub(super) last_runs: Vec<Instant>,
}

#[derive(Clone, Copy)]
pub(super) struct ScheduleWorkload {
    pub(super) online_count: usize,
    pub(super) player_entity_count: usize,
    pub(super) crafting_due: bool,
}

#[derive(Clone, Copy)]
pub(super) struct ScheduleCandidate {
    pub(super) activity: ScheduleActivity,
    pub(super) interval: Duration,
}

impl ScheduleClock {
    pub(super) fn new(len: usize, now: Instant) -> Self {
        Self {
            last_runs: vec![now; len],
        }
    }

    fn sync_len(&mut self, len: usize, now: Instant) {
        self.last_runs.resize(len, now);
    }

    pub(super) fn last_run_mut(&mut self, idx: usize, now: Instant) -> &mut Instant {
        if idx >= self.last_runs.len() {
            self.last_runs.resize(idx + 1, now);
        }
        &mut self.last_runs[idx]
    }

    pub(super) fn select_due<F>(
        &mut self,
        total_len: usize,
        now: Instant,
        workload: ScheduleWorkload,
        mut candidate_at: F,
    ) -> Vec<usize>
    where
        F: FnMut(usize) -> Option<ScheduleCandidate>,
    {
        self.sync_len(total_len, now);
        let mut due_schedules = Vec::new();
        for idx in 0..total_len {
            let Some(schedule) = candidate_at(idx) else {
                continue;
            };
            let last_run = self.last_run_mut(idx, now);
            if now.duration_since(*last_run) < schedule.interval {
                continue;
            }
            if schedule_due_but_idle(schedule.activity, workload) {
                *last_run = now;
                continue;
            }
            due_schedules.push(idx);
        }
        due_schedules
    }
}

const fn schedule_due_but_idle(activity: ScheduleActivity, workload: ScheduleWorkload) -> bool {
    match activity {
        ScheduleActivity::Always => false,
        ScheduleActivity::OnlinePlayers => workload.online_count == 0,
        ScheduleActivity::PlayerEntities => workload.player_entity_count == 0,
        ScheduleActivity::DueCrafting => !workload.crafting_due,
    }
}

fn drain_schedule_tail(
    heartbeat: &TickHeartbeat,
    ecs: &mut bevy_ecs::prelude::World,
    player_entity_count: usize,
    online_count: usize,
) -> ScheduleTailOutput {
    let mut profile = ScheduleTailProfile::default();
    heartbeat.mark(TickStage::FlushQueues);
    let started = Instant::now();
    let broadcasts = std::mem::take(&mut ecs.resource_mut::<crate::game::BroadcastQueue>().0);
    profile.broadcast_queue = started.elapsed();

    let started = Instant::now();
    let programmator_actions =
        std::mem::take(&mut ecs.resource_mut::<crate::game::ProgrammatorQueue>().0);
    profile.programmator_queue = started.elapsed();

    let started = Instant::now();
    let cell_conversions =
        std::mem::take(&mut ecs.resource_mut::<crate::game::PendingCellConversions>().0);
    profile.cell_conversion_queue = started.elapsed();

    let started = Instant::now();
    let pack_resends = std::mem::take(&mut ecs.resource_mut::<crate::game::PackResendQueue>().0);
    profile.pack_resend_queue = started.elapsed();

    let started = Instant::now();
    let sim_profile = empty_sim_profile(player_entity_count, online_count);
    profile.sim_profile = started.elapsed();

    ScheduleTailOutput {
        broadcasts,
        programmator_actions,
        cell_conversions,
        pack_resends,
        profile,
        sim_profile,
    }
}

fn warn_slow_schedule(name: &str, lock_wait: Duration, run: Duration, threshold: Duration) {
    let total = lock_wait + run;
    if total <= threshold {
        return;
    }
    tracing::warn!(
        target: "scheduler",
        schedule = name,
        duration = ?total,
        ?lock_wait,
        ?run,
        ?threshold,
        "System schedule execution exceeded warning threshold"
    );
}

fn record_schedule_run(
    runs: &mut Vec<ScheduleRunProfile>,
    name: &str,
    lock_wait: Duration,
    run: Duration,
    threshold: Duration,
) -> Duration {
    runs.push(ScheduleRunProfile {
        name: name.to_owned(),
        lock_wait,
        run,
    });
    warn_slow_schedule(name, lock_wait, run, threshold);
    lock_wait + run
}

pub(super) fn run_schedule_phase(
    state: &Arc<GameState>,
    heartbeat: &TickHeartbeat,
    schedule_clock: &mut ScheduleClock,
    online_count: usize,
    player_entity_count: usize,
    schedule_warn_threshold: Duration,
) -> ScheduleTickResult {
    let mut schedule_runs: Vec<ScheduleRunProfile> = Vec::new();
    let now = Instant::now();
    let now_ts = crate::time::now_unix();

    let select_t0 = Instant::now();
    let due_schedules = schedule_clock.select_due(
        state.schedules.len(),
        now,
        ScheduleWorkload {
            online_count,
            player_entity_count,
            crafting_due: state.has_due_crafting(now_ts),
        },
        |idx| {
            let gs = state.schedules.get(idx)?;
            let interval_ms = gs.interval_ms.load(std::sync::atomic::Ordering::Relaxed);
            (interval_ms != 0).then(|| ScheduleCandidate {
                activity: gs.activity,
                interval: Duration::from_millis(interval_ms),
            })
        },
    );
    let sched_select = select_t0.elapsed();

    if due_schedules.is_empty() {
        return ScheduleTickResult::idle(
            sched_select,
            player_entity_count,
            online_count,
            schedule_runs,
        );
    }

    heartbeat.mark(TickStage::EcsLockWait);
    let lock_t0 = Instant::now();
    let mut ecs = state.ecs_write_profiled("tick.schedule");
    let lw = lock_t0.elapsed();
    let mut schedule_run_total = Duration::ZERO;

    for idx in due_schedules {
        let Some(gs) = state.schedules.get(idx) else {
            continue;
        };
        heartbeat.mark_schedule(TickStage::ScheduleRun, idx.try_into().unwrap_or(u64::MAX));
        let crafting_due_remaining = if gs.activity == ScheduleActivity::DueCrafting {
            let (due, due_remaining, depth) = state.take_due_crafting(now_ts);
            crate::metrics::CRAFTING_DUE_BATCH_TOTAL
                .inc_by(u64::try_from(due.len()).unwrap_or(u64::MAX));
            crate::metrics::CRAFTING_DUE_DEPTH.set(i64::try_from(depth).unwrap_or(i64::MAX));
            ecs.resource_mut::<crate::game::building_damage::CraftingDueBatch>()
                .0 = due;
            due_remaining
        } else {
            false
        };
        let (schedule_lock_wait, schedule_run) = {
            let schedule_lock_t0 = Instant::now();
            let mut schedule = gs.schedule.write();
            let schedule_lock_wait = schedule_lock_t0.elapsed();
            let schedule_run_t0 = Instant::now();
            schedule.run(&mut ecs);
            let schedule_run = schedule_run_t0.elapsed();
            drop(schedule);
            (schedule_lock_wait, schedule_run)
        };
        let total = record_schedule_run(
            &mut schedule_runs,
            &gs.name,
            schedule_lock_wait,
            schedule_run,
            schedule_warn_threshold,
        );
        schedule_run_total += total;
        if !crafting_due_remaining {
            *schedule_clock.last_run_mut(idx, now) = now;
        }
    }

    let tail = drain_schedule_tail(heartbeat, &mut ecs, player_entity_count, online_count);

    let tail_t0 = Instant::now();
    drop(ecs);
    let mut tail_profile = tail.profile;
    tail_profile.drop_ecs_lock = tail_t0.elapsed();
    ScheduleTickResult {
        broadcasts: tail.broadcasts,
        programmator_actions: tail.programmator_actions,
        cell_conversions: tail.cell_conversions,
        pack_resends: tail.pack_resends,
        sched_select,
        sched_lock_wait: lw,
        sched_run: schedule_run_total,
        schedule_tail_profile: tail_profile,
        schedule_runs,
        sim_profile: tail.sim_profile,
    }
}
