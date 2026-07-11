//! Tick timing aggregation and off-thread logging.

use std::time::{Duration, Instant};

#[derive(Clone, Copy, Default)]
pub(super) struct SideProfile {
    pub(super) broadcasts: std::time::Duration,
    pub(super) pack_resends: std::time::Duration,
    pub(super) box_pickups: std::time::Duration,
    pub(super) persistence_flush: std::time::Duration,
    pub(super) cell_conversions: std::time::Duration,
    pub(super) programmator_actions: std::time::Duration,
    pub(super) death: std::time::Duration,
    pub(super) bots_render: std::time::Duration,
}

impl SideProfile {
    pub(super) fn update_max(&mut self, other: Self) {
        self.broadcasts = self.broadcasts.max(other.broadcasts);
        self.pack_resends = self.pack_resends.max(other.pack_resends);
        self.box_pickups = self.box_pickups.max(other.box_pickups);
        self.persistence_flush = self.persistence_flush.max(other.persistence_flush);
        self.cell_conversions = self.cell_conversions.max(other.cell_conversions);
        self.programmator_actions = self.programmator_actions.max(other.programmator_actions);
        self.death = self.death.max(other.death);
        self.bots_render = self.bots_render.max(other.bots_render);
    }

    fn dominant(self) -> (&'static str, Duration) {
        [
            ("broadcasts", self.broadcasts),
            ("pack_resends", self.pack_resends),
            ("box_pickups", self.box_pickups),
            ("persistence_flush", self.persistence_flush),
            ("cell_conversions", self.cell_conversions),
            ("programmator_actions", self.programmator_actions),
            ("death", self.death),
            ("bots_render", self.bots_render),
        ]
        .into_iter()
        .max_by_key(|(_, elapsed)| *elapsed)
        .unwrap_or(("unknown", Duration::ZERO))
    }
}

pub(super) struct TickWindowProfile {
    pub(super) start: Instant,
    pub(super) ticks: u64,
    pub(super) over_budget: u64,
    pub(super) max_total: Duration,
    pub(super) max_dispatch: Duration,
    pub(super) max_schedule: Duration,
    pub(super) max_side: Duration,
    pub(super) max_unprofiled: Duration,
    pub(super) max_unprofiled_profile: TickUnprofiledProfile,
    pub(super) max_side_profile: SideProfile,
    pub(super) max_schedule_tail_profile: ScheduleTailProfile,
    pub(super) max_actions: usize,
    pub(super) max_top_schedule: Duration,
    pub(super) max_top_schedule_name: String,
}

impl TickWindowProfile {
    pub(super) fn new(start: Instant) -> Self {
        Self {
            start,
            ticks: 0,
            over_budget: 0,
            max_total: Duration::ZERO,
            max_dispatch: Duration::ZERO,
            max_schedule: Duration::ZERO,
            max_side: Duration::ZERO,
            max_unprofiled: Duration::ZERO,
            max_unprofiled_profile: TickUnprofiledProfile::default(),
            max_side_profile: SideProfile::default(),
            max_schedule_tail_profile: ScheduleTailProfile::default(),
            max_actions: 0,
            max_top_schedule: Duration::ZERO,
            max_top_schedule_name: "-".to_string(),
        }
    }

    pub(super) fn record(
        &mut self,
        durations: TickDurations,
        side_profile: SideProfile,
        schedule_tail_profile: ScheduleTailProfile,
        actions: usize,
        top_schedule: Option<(&str, Duration)>,
        tick_budget: Duration,
    ) {
        self.ticks += 1;
        if durations.total > tick_budget {
            self.over_budget += 1;
        }
        self.max_total = self.max_total.max(durations.total);
        self.max_dispatch = self.max_dispatch.max(durations.dispatch);
        self.max_schedule = self.max_schedule.max(durations.schedule);
        self.max_side = self.max_side.max(durations.side);
        self.max_unprofiled = self.max_unprofiled.max(durations.unprofiled);
        self.max_unprofiled_profile
            .update_max(durations.unprofiled_profile);
        self.max_side_profile.update_max(side_profile);
        self.max_schedule_tail_profile
            .update_max(schedule_tail_profile);
        self.max_actions = self.max_actions.max(actions);
        if let Some((name, elapsed)) = top_schedule
            && elapsed > self.max_top_schedule
        {
            self.max_top_schedule = elapsed;
            self.max_top_schedule_name.clear();
            self.max_top_schedule_name.push_str(name);
        }
    }

    pub(super) fn reset(&mut self, start: Instant) {
        *self = Self::new(start);
    }

    fn log_and_reset_if_due(&mut self) {
        if self.start.elapsed() < Duration::from_secs(5) {
            return;
        }
        tracing::debug!(
            target: "tickprof",
            "5s summary: ticks={} over_budget={} \
             max_total={:?} max_dispatch={:?} \
             max_schedule={:?} max_side={:?} \
             max_unprofiled={:?} max_actions={} max_top_schedule={} max_top_schedule_elapsed={:?} max_side_broadcasts={:?} \
             max_side_pack_resends={:?} max_side_box_pickups={:?} max_side_persistence_flush={:?} \
             max_side_cell_conversions={:?} max_side_programmator_actions={:?} \
             max_side_death={:?} max_side_bots_render={:?} \
             max_sched_tail_broadcast_queue={:?} \
             max_sched_tail_programmator_queue={:?} \
             max_sched_tail_cell_conversion_queue={:?} max_sched_tail_pack_resend_queue={:?} \
             max_sched_tail_sim_profile={:?} max_sched_tail_drop_ecs_lock={:?} \
             max_unprofiled_setup={:?} max_unprofiled_dispatch_to_schedule={:?} \
             max_unprofiled_schedule_to_side={:?} max_unprofiled_side_accounting_gap={:?} \
             max_unprofiled_remainder={:?}",
            self.ticks,
            self.over_budget,
            self.max_total,
            self.max_dispatch,
            self.max_schedule,
            self.max_side,
            self.max_unprofiled,
            self.max_actions,
            self.max_top_schedule_name,
            self.max_top_schedule,
            self.max_side_profile.broadcasts,
            self.max_side_profile.pack_resends,
            self.max_side_profile.box_pickups,
            self.max_side_profile.persistence_flush,
            self.max_side_profile.cell_conversions,
            self.max_side_profile.programmator_actions,
            self.max_side_profile.death,
            self.max_side_profile.bots_render,
            self.max_schedule_tail_profile.broadcast_queue,
            self.max_schedule_tail_profile.programmator_queue,
            self.max_schedule_tail_profile.cell_conversion_queue,
            self.max_schedule_tail_profile.pack_resend_queue,
            self.max_schedule_tail_profile.sim_profile,
            self.max_schedule_tail_profile.drop_ecs_lock,
            self.max_unprofiled_profile.setup,
            self.max_unprofiled_profile.dispatch_to_schedule,
            self.max_unprofiled_profile.schedule_to_side,
            self.max_unprofiled_profile.side_accounting_gap,
            self.max_unprofiled_profile.remainder,
        );
        self.reset(Instant::now());
    }
}

#[derive(Clone, Copy)]
pub(super) struct TickDurations {
    pub(super) total: Duration,
    pub(super) dispatch: Duration,
    pub(super) schedule: Duration,
    pub(super) side: Duration,
    pub(super) unprofiled: Duration,
    pub(super) unprofiled_profile: TickUnprofiledProfile,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct TickUnprofiledProfile {
    pub(super) setup: Duration,
    pub(super) dispatch_to_schedule: Duration,
    pub(super) schedule_to_side: Duration,
    pub(super) side_accounting_gap: Duration,
    pub(super) remainder: Duration,
}

impl TickUnprofiledProfile {
    fn update_max(&mut self, other: Self) {
        self.setup = self.setup.max(other.setup);
        self.dispatch_to_schedule = self.dispatch_to_schedule.max(other.dispatch_to_schedule);
        self.schedule_to_side = self.schedule_to_side.max(other.schedule_to_side);
        self.side_accounting_gap = self.side_accounting_gap.max(other.side_accounting_gap);
        self.remainder = self.remainder.max(other.remainder);
    }

    pub(super) const fn total(self) -> Duration {
        self.setup
            .saturating_add(self.dispatch_to_schedule)
            .saturating_add(self.schedule_to_side)
            .saturating_add(self.side_accounting_gap)
            .saturating_add(self.remainder)
    }

    fn dominant(self) -> (&'static str, Duration) {
        [
            ("setup", self.setup),
            ("dispatch_to_schedule", self.dispatch_to_schedule),
            ("schedule_to_side", self.schedule_to_side),
            ("side_accounting_gap", self.side_accounting_gap),
            ("remainder", self.remainder),
        ]
        .into_iter()
        .max_by_key(|(_, elapsed)| *elapsed)
        .unwrap_or(("unknown", Duration::ZERO))
    }
}

#[derive(Clone, Debug)]
pub(super) struct ScheduleRunProfile {
    pub(super) name: String,
    pub(super) lock_wait: Duration,
    pub(super) run: Duration,
}

impl ScheduleRunProfile {
    fn total(&self) -> Duration {
        self.lock_wait + self.run
    }
}

pub(super) struct TickLogEvent {
    pub(super) full: bool,
    pub(super) total: Duration,
    pub(super) thread_cpu: Duration,
    pub(super) off_cpu: Duration,
    pub(super) dispatch: Duration,
    pub(super) schedule: Duration,
    pub(super) side: Duration,
    pub(super) unprofiled: Duration,
    pub(super) actions: usize,
    pub(super) deferred_commands: usize,
    pub(super) tick_budget: Duration,
    pub(super) schedule_warn_threshold: Duration,
    pub(super) dominant_section: &'static str,
    pub(super) top_command_name: &'static str,
    pub(super) top_command_elapsed: Duration,
    pub(super) top_schedule_name: String,
    pub(super) top_schedule_elapsed: Duration,
    pub(super) sched_select: Duration,
    pub(super) sched_lock_wait: Duration,
    pub(super) sched_run: Duration,
    pub(super) sched_flush: Duration,
    pub(super) side_profile: SideProfile,
    pub(super) schedule_tail_profile: ScheduleTailProfile,
    pub(super) unprofiled_profile: TickUnprofiledProfile,
    pub(super) sim_profile: SimProfile,
    pub(super) queue_profile: QueueProfile,
    pub(super) programmator_action_profile: ProgrammatorActionProfile,
    pub(super) schedule_runs: Vec<ScheduleRunProfile>,
}

pub(super) struct TickSample {
    pub(super) total: Duration,
    pub(super) thread_cpu: Duration,
    pub(super) dispatch: Duration,
    pub(super) schedule: Duration,
    pub(super) side: Duration,
    pub(super) setup: Duration,
    pub(super) dispatch_to_schedule: Duration,
    pub(super) schedule_to_side: Duration,
    pub(super) side_accounting_gap: Duration,
    pub(super) actions: usize,
    pub(super) deferred_commands: usize,
    pub(super) tick_budget: Duration,
    pub(super) schedule_warn_threshold: Duration,
    pub(super) top_command_name: &'static str,
    pub(super) top_command_elapsed: Duration,
    pub(super) sched_select: Duration,
    pub(super) sched_lock_wait: Duration,
    pub(super) sched_run: Duration,
    pub(super) sched_flush: Duration,
    pub(super) side_profile: SideProfile,
    pub(super) schedule_tail_profile: ScheduleTailProfile,
    pub(super) sim_profile: SimProfile,
    pub(super) queue_profile: QueueProfile,
    pub(super) programmator_action_profile: ProgrammatorActionProfile,
    pub(super) schedule_runs: Vec<ScheduleRunProfile>,
}

pub(super) fn record_tick_profile(
    window: &mut TickWindowProfile,
    last_warn: &mut Instant,
    log_tx: &std::sync::mpsc::SyncSender<TickLogEvent>,
    sample: TickSample,
    emit_window_summary: bool,
) {
    let off_cpu = sample.total.saturating_sub(sample.thread_cpu);
    let unprofiled = sample
        .total
        .saturating_sub(sample.dispatch)
        .saturating_sub(sample.schedule)
        .saturating_sub(sample.side);
    let mut unprofiled_profile = TickUnprofiledProfile {
        setup: sample.setup,
        dispatch_to_schedule: sample.dispatch_to_schedule,
        schedule_to_side: sample.schedule_to_side,
        side_accounting_gap: sample.side_accounting_gap,
        remainder: Duration::ZERO,
    };
    unprofiled_profile.remainder = unprofiled.saturating_sub(unprofiled_profile.total());
    let durations = TickDurations {
        total: sample.total,
        dispatch: sample.dispatch,
        schedule: sample.schedule,
        side: sample.side,
        unprofiled,
        unprofiled_profile,
    };
    let top_schedule = top_schedule_run(&sample.schedule_runs);
    let dominant_section = dominant_tick_section(durations);
    window.record(
        durations,
        sample.side_profile,
        sample.schedule_tail_profile,
        sample.actions,
        top_schedule,
        sample.tick_budget,
    );

    if sample.total > sample.tick_budget && last_warn.elapsed() >= Duration::from_millis(500) {
        *last_warn = Instant::now();
        let top_schedule_name = top_schedule.map_or("-", |(name, _)| name).to_owned();
        let top_schedule_elapsed = top_schedule.map_or(Duration::ZERO, |(_, elapsed)| elapsed);
        let event = TickLogEvent {
            full: sample.total > sample.schedule_warn_threshold,
            total: sample.total,
            thread_cpu: sample.thread_cpu,
            off_cpu,
            dispatch: sample.dispatch,
            schedule: sample.schedule,
            side: sample.side,
            unprofiled,
            actions: sample.actions,
            deferred_commands: sample.deferred_commands,
            tick_budget: sample.tick_budget,
            schedule_warn_threshold: sample.schedule_warn_threshold,
            dominant_section,
            top_command_name: sample.top_command_name,
            top_command_elapsed: sample.top_command_elapsed,
            top_schedule_name,
            top_schedule_elapsed,
            sched_select: sample.sched_select,
            sched_lock_wait: sample.sched_lock_wait,
            sched_run: sample.sched_run,
            sched_flush: sample.sched_flush,
            side_profile: sample.side_profile,
            schedule_tail_profile: sample.schedule_tail_profile,
            unprofiled_profile,
            sim_profile: sample.sim_profile,
            queue_profile: sample.queue_profile,
            programmator_action_profile: sample.programmator_action_profile,
            schedule_runs: sample.schedule_runs,
        };
        let _ = log_tx.try_send(event);
    }
    if emit_window_summary {
        window.log_and_reset_if_due();
    }
}

pub(super) fn spawn_tick_log_worker(rx: std::sync::mpsc::Receiver<TickLogEvent>) {
    std::thread::Builder::new()
        .name("openmines-tickprof-log".to_owned())
        .spawn(move || {
            while let Ok(event) = rx.recv() {
                log_tick_event(&event);
            }
        })
        .expect("spawn tickprof log worker");
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TickExecutionClass {
    CpuBound,
    Preempted,
    Mixed,
}

impl TickExecutionClass {
    const fn name(self) -> &'static str {
        match self {
            Self::CpuBound => "cpu_bound",
            Self::Preempted => "preempted",
            Self::Mixed => "mixed",
        }
    }
}

pub(super) fn classify_tick_execution(
    thread_cpu: Duration,
    off_cpu: Duration,
    tick_budget: Duration,
) -> TickExecutionClass {
    if thread_cpu > tick_budget {
        TickExecutionClass::CpuBound
    } else if thread_cpu <= tick_budget / 4 && off_cpu >= thread_cpu.saturating_mul(4) {
        TickExecutionClass::Preempted
    } else {
        TickExecutionClass::Mixed
    }
}

#[allow(clippy::too_many_lines)]
fn log_tick_event(event: &TickLogEvent) {
    let (dominant_schedule_tail, dominant_schedule_tail_elapsed) =
        event.schedule_tail_profile.dominant();
    let (dominant_side, dominant_side_elapsed) = event.side_profile.dominant();
    let (dominant_unprofiled, dominant_unprofiled_elapsed) = event.unprofiled_profile.dominant();
    let execution_class =
        classify_tick_execution(event.thread_cpu, event.off_cpu, event.tick_budget);
    if execution_class == TickExecutionClass::Preempted {
        tracing::debug!(
            target: "tickprof",
            tick_budget = ?event.tick_budget,
            total = ?event.total,
            thread_cpu = ?event.thread_cpu,
            off_cpu = ?event.off_cpu,
            execution_class = execution_class.name(),
            dominant_section = event.dominant_section,
            sim_player_entities = event.sim_profile.player_entities,
            sim_online_players = event.sim_profile.online_players,
            sched_select = ?event.sched_select,
            sched_run = ?event.sched_run,
            "Tick deadline missed while game thread was descheduled"
        );
        return;
    }
    let execution_class = execution_class.name();
    if !event.full {
        tracing::warn!(
            target: "tickprof",
            tick_budget = ?event.tick_budget,
            total = ?event.total,
            thread_cpu = ?event.thread_cpu,
            off_cpu = ?event.off_cpu,
            execution_class,
            dispatch = ?event.dispatch,
            schedule = ?event.schedule,
            side = ?event.side,
            unprofiled = ?event.unprofiled,
            dominant_section = event.dominant_section,
            top_command = event.top_command_name,
            top_command_elapsed = ?event.top_command_elapsed,
            top_schedule = event.top_schedule_name,
            top_schedule_elapsed = ?event.top_schedule_elapsed,
            deferred_commands = event.deferred_commands,
            sim_player_entities = event.sim_profile.player_entities,
            sim_online_players = event.sim_profile.online_players,
            sim_running_programmators = event.sim_profile.running_programmators,
            sched_select = ?event.sched_select,
            sched_lock_wait = ?event.sched_lock_wait,
            sched_run = ?event.sched_run,
            sched_flush = ?event.sched_flush,
            dominant_side,
            dominant_side_elapsed = ?dominant_side_elapsed,
            dominant_schedule_tail,
            dominant_schedule_tail_elapsed = ?dominant_schedule_tail_elapsed,
            dominant_unprofiled,
            dominant_unprofiled_elapsed = ?dominant_unprofiled_elapsed,
            schedule_runs = ?event.schedule_runs,
            "OVER-BUDGET tick compact"
        );
        return;
    }

    tracing::warn!(
        target: "tickprof",
        sim_player_entities = event.sim_profile.player_entities,
        sim_online_players = event.sim_profile.online_players,
        sim_offline_player_entities = event.sim_profile.offline_player_entities,
        sim_running_programmators = event.sim_profile.running_programmators,
        sim_online_running_programmators = event.sim_profile.online_running_programmators,
        sim_offline_running_programmators = event.sim_profile.offline_running_programmators,
        queue_broadcasts = event.queue_profile.broadcasts,
        queue_pack_resends = event.queue_profile.pack_resends,
        queue_cell_conversions_in = event.queue_profile.cell_conversions_in,
        queue_cell_conversions_remaining = event.queue_profile.cell_conversions_remaining,
        queue_cell_conversions_applied = event.queue_profile.cell_conversions_applied,
        queue_programmator_actions = event.queue_profile.programmator_actions,
        queue_deaths = event.queue_profile.deaths,
        deferred_commands = event.deferred_commands,
        prog_moves = event.programmator_action_profile.moves,
        prog_digs = event.programmator_action_profile.digs,
        prog_builds = event.programmator_action_profile.builds,
        prog_geo = event.programmator_action_profile.geo,
        prog_heal = event.programmator_action_profile.heal,
        prog_set_auto_dig = event.programmator_action_profile.set_auto_dig,
        prog_set_aggression = event.programmator_action_profile.set_aggression,
        prog_set_hand_mode = event.programmator_action_profile.set_hand_mode,
        prog_fill_gun = event.programmator_action_profile.fill_gun,
        prog_set_status = event.programmator_action_profile.set_status,
        tick_budget = ?event.tick_budget,
        thread_cpu = ?event.thread_cpu,
        off_cpu = ?event.off_cpu,
        execution_class,
        schedule_warn_threshold = ?event.schedule_warn_threshold,
        dominant_section = event.dominant_section,
        top_command = event.top_command_name,
        top_command_elapsed = ?event.top_command_elapsed,
        top_schedule = event.top_schedule_name,
        top_schedule_elapsed = ?event.top_schedule_elapsed,
        sched_select = ?event.sched_select,
        sched_lock_wait = ?event.sched_lock_wait,
        sched_run = ?event.sched_run,
        sched_flush = ?event.sched_flush,
        dominant_side,
        dominant_side_elapsed = ?dominant_side_elapsed,
        sched_tail_total = ?event.schedule_tail_profile.total(),
        sched_tail_broadcast_queue = ?event.schedule_tail_profile.broadcast_queue,
        sched_tail_programmator_queue = ?event.schedule_tail_profile.programmator_queue,
        sched_tail_cell_conversion_queue = ?event.schedule_tail_profile.cell_conversion_queue,
        sched_tail_pack_resend_queue = ?event.schedule_tail_profile.pack_resend_queue,
        sched_tail_sim_profile = ?event.schedule_tail_profile.sim_profile,
        sched_tail_drop_ecs_lock = ?event.schedule_tail_profile.drop_ecs_lock,
        dominant_schedule_tail,
        dominant_schedule_tail_elapsed = ?dominant_schedule_tail_elapsed,
        unprofiled_setup = ?event.unprofiled_profile.setup,
        unprofiled_dispatch_to_schedule = ?event.unprofiled_profile.dispatch_to_schedule,
        unprofiled_schedule_to_side = ?event.unprofiled_profile.schedule_to_side,
        unprofiled_side_accounting_gap = ?event.unprofiled_profile.side_accounting_gap,
        unprofiled_remainder = ?event.unprofiled_profile.remainder,
        dominant_unprofiled,
        dominant_unprofiled_elapsed = ?dominant_unprofiled_elapsed,
        schedule_runs = ?event.schedule_runs,
        "OVER-BUDGET tick: total={:?} dispatch={:?} schedule={:?} side={:?} unprofiled={:?} \
         actions={} side_broadcasts={:?} side_pack_resends={:?} side_box_pickups={:?} side_persistence_flush={:?} \
         side_cell_conversions={:?} side_programmator_actions={:?} side_death={:?} \
         side_bots_render={:?}",
        event.total,
        event.dispatch,
        event.schedule,
        event.side,
        event.unprofiled,
        event.actions,
        event.side_profile.broadcasts,
        event.side_profile.pack_resends,
        event.side_profile.box_pickups,
        event.side_profile.persistence_flush,
        event.side_profile.cell_conversions,
        event.side_profile.programmator_actions,
        event.side_profile.death,
        event.side_profile.bots_render,
    );
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ScheduleTailProfile {
    pub(super) broadcast_queue: Duration,
    pub(super) programmator_queue: Duration,
    pub(super) cell_conversion_queue: Duration,
    pub(super) pack_resend_queue: Duration,
    pub(super) sim_profile: Duration,
    pub(super) drop_ecs_lock: Duration,
}

impl ScheduleTailProfile {
    fn update_max(&mut self, other: Self) {
        self.broadcast_queue = self.broadcast_queue.max(other.broadcast_queue);
        self.programmator_queue = self.programmator_queue.max(other.programmator_queue);
        self.cell_conversion_queue = self.cell_conversion_queue.max(other.cell_conversion_queue);
        self.pack_resend_queue = self.pack_resend_queue.max(other.pack_resend_queue);
        self.sim_profile = self.sim_profile.max(other.sim_profile);
        self.drop_ecs_lock = self.drop_ecs_lock.max(other.drop_ecs_lock);
    }

    fn total(self) -> Duration {
        self.broadcast_queue
            + self.programmator_queue
            + self.cell_conversion_queue
            + self.pack_resend_queue
            + self.sim_profile
            + self.drop_ecs_lock
    }

    fn dominant(self) -> (&'static str, Duration) {
        [
            ("broadcast_queue", self.broadcast_queue),
            ("programmator_queue", self.programmator_queue),
            ("cell_conversion_queue", self.cell_conversion_queue),
            ("pack_resend_queue", self.pack_resend_queue),
            ("sim_profile", self.sim_profile),
            ("drop_ecs_lock", self.drop_ecs_lock),
        ]
        .into_iter()
        .max_by_key(|(_, elapsed)| *elapsed)
        .unwrap_or(("unknown", Duration::ZERO))
    }
}

pub(super) fn dominant_tick_section(durations: TickDurations) -> &'static str {
    [
        ("dispatch", durations.dispatch),
        ("schedule", durations.schedule),
        ("side", durations.side),
        ("unprofiled", durations.unprofiled),
    ]
    .into_iter()
    .max_by_key(|(_, elapsed)| *elapsed)
    .map_or("unknown", |(name, _)| name)
}

pub(super) fn top_schedule_run(schedule_runs: &[ScheduleRunProfile]) -> Option<(&str, Duration)> {
    schedule_runs
        .iter()
        .max_by_key(|profile| profile.total())
        .map(|profile| (profile.name.as_str(), profile.total()))
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct SimProfile {
    player_entities: usize,
    online_players: usize,
    offline_player_entities: usize,
    running_programmators: usize,
    online_running_programmators: usize,
    offline_running_programmators: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct QueueProfile {
    pub(super) broadcasts: usize,
    pub(super) pack_resends: usize,
    pub(super) cell_conversions_in: usize,
    pub(super) cell_conversions_remaining: usize,
    pub(super) cell_conversions_applied: usize,
    pub(super) programmator_actions: usize,
    pub(super) deaths: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ProgrammatorActionProfile {
    pub(super) moves: usize,
    pub(super) digs: usize,
    pub(super) builds: usize,
    pub(super) geo: usize,
    pub(super) heal: usize,
    pub(super) set_auto_dig: usize,
    pub(super) set_aggression: usize,
    pub(super) set_hand_mode: usize,
    pub(super) fill_gun: usize,
    pub(super) set_status: usize,
}

impl ProgrammatorActionProfile {
    pub(super) const fn count(&mut self, action: &crate::game::ProgrammatorAction) {
        match action {
            crate::game::ProgrammatorAction::Move { .. } => self.moves += 1,
            crate::game::ProgrammatorAction::Dig { .. } => self.digs += 1,
            crate::game::ProgrammatorAction::Build { .. } => self.builds += 1,
            crate::game::ProgrammatorAction::Geo { .. } => self.geo += 1,
            crate::game::ProgrammatorAction::Heal { .. } => self.heal += 1,
            crate::game::ProgrammatorAction::SetAutoDig { .. } => self.set_auto_dig += 1,
            crate::game::ProgrammatorAction::SetAggression { .. } => self.set_aggression += 1,
            crate::game::ProgrammatorAction::SetHandMode { .. } => self.set_hand_mode += 1,
            crate::game::ProgrammatorAction::FillGun { .. } => self.fill_gun += 1,
            crate::game::ProgrammatorAction::SetProgrammatorStatus { .. } => self.set_status += 1,
            crate::game::ProgrammatorAction::Send { .. } => {}
        }
    }
}

pub(super) const fn empty_sim_profile(player_entities: usize, online_players: usize) -> SimProfile {
    SimProfile {
        player_entities,
        online_players,
        offline_player_entities: player_entities.saturating_sub(online_players),
        running_programmators: 0,
        online_running_programmators: 0,
        offline_running_programmators: 0,
    }
}
