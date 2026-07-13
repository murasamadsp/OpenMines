//! One authoritative simulation tick.

use super::commands::{CommandPhase, run_command_phase};
use super::due::{DuePhase, run_due_action_phase};
use super::effects::{EffectSources, PreparedEffects, apply_side_effects, collect_pending_effects};
use super::profiler::{
    ProgrammatorActionProfile, QueueProfile, ScheduleRunProfile, ScheduleTailProfile, SideProfile,
    SimProfile, TickSample, record_tick_profile,
};
use super::scheduler::{ScheduleClock, ScheduleTickResult, run_schedule_phase};
use super::{TickPendingWork, TickProfileState, TickServices, TickStage};
use crate::game::GameState;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct SimulationPhase {
    schedule_started_at: Instant,
    schedule_elapsed: Duration,
    schedule_to_side: Duration,
    side_elapsed: Duration,
    side_accounting_gap: Duration,
    sched_select: Duration,
    sched_lock_wait: Duration,
    sched_run: Duration,
    sched_flush: Duration,
    side_profile: SideProfile,
    schedule_tail_profile: ScheduleTailProfile,
    sim_profile: SimProfile,
    queue_profile: QueueProfile,
    programmator_action_profile: ProgrammatorActionProfile,
    schedule_runs: Vec<ScheduleRunProfile>,
    has_work: bool,
}

struct TickEffects {
    command: crate::game::CommandEffects,
    due: Vec<super::due::DueEffect>,
}

pub(super) fn run_game_tick_sync(
    state: &Arc<GameState>,
    rx: &mut crate::game::CommandReceivers,
    due_actions: &mut crate::game::logic::due::DueActionQueue,
    tick_profile: &mut TickProfileState,
    schedule_clock: &mut ScheduleClock,
    pending_work: &mut TickPendingWork,
    services: &TickServices,
) {
    let thread_cpu_started = cpu_time::ThreadTime::now();
    let tick_budget = Duration::from_millis(state.config.gameplay.schedules.game_loop_tick_rate_ms);
    let schedule_warn_threshold =
        Duration::from_millis(state.config.gameplay.schedules.schedule_warn_threshold_ms);
    let tick_started_at = Instant::now();
    let setup_started_at = Instant::now();
    services.heartbeat.begin_tick();
    let setup = setup_started_at.elapsed();

    let CommandPhase {
        effects: command_effects,
        started_at: dispatch_started_at,
        elapsed: command_elapsed,
        actions: command_actions,
        deferred_commands,
        mut top_command_name,
        mut top_command_elapsed,
    } = run_command_phase(state, rx, due_actions, pending_work, services, tick_budget);
    let DuePhase {
        effects: due_effects,
        elapsed: due_elapsed,
        executed: due_actions_executed,
    } = run_due_action_phase(state, due_actions);
    if due_elapsed > top_command_elapsed {
        top_command_name = "due_actions";
        top_command_elapsed = due_elapsed;
    }
    let dispatch = dispatch_started_at.elapsed();
    debug_assert!(dispatch >= command_elapsed.saturating_add(due_elapsed));
    let actions = command_actions.saturating_add(due_actions_executed);
    let simulation = run_simulation_phase(
        state,
        schedule_clock,
        pending_work,
        services,
        TickEffects {
            command: command_effects,
            due: due_effects,
        },
        tick_budget,
        schedule_warn_threshold,
    );
    let total = tick_started_at.elapsed();
    let dispatch_to_schedule = simulation
        .schedule_started_at
        .saturating_duration_since(dispatch_started_at)
        .saturating_sub(dispatch);
    record_tick_profile(
        &mut tick_profile.window,
        &mut tick_profile.last_warn,
        &services.tick_log_tx,
        TickSample {
            total,
            thread_cpu: thread_cpu_started.elapsed(),
            dispatch,
            schedule: simulation.schedule_elapsed,
            side: simulation.side_elapsed,
            setup,
            dispatch_to_schedule,
            schedule_to_side: simulation.schedule_to_side,
            side_accounting_gap: simulation.side_accounting_gap,
            actions,
            deferred_commands,
            tick_budget,
            schedule_warn_threshold,
            top_command_name,
            top_command_elapsed,
            sched_select: simulation.sched_select,
            sched_lock_wait: simulation.sched_lock_wait,
            sched_run: simulation.sched_run,
            sched_flush: simulation.sched_flush,
            side_profile: simulation.side_profile,
            schedule_tail_profile: simulation.schedule_tail_profile,
            sim_profile: simulation.sim_profile,
            queue_profile: simulation.queue_profile,
            programmator_action_profile: simulation.programmator_action_profile,
            schedule_runs: simulation.schedule_runs,
        },
        simulation.has_work,
    );
}

pub(super) fn run_quiescing_cycle(
    state: &Arc<GameState>,
    rx: &mut crate::game::CommandReceivers,
    due_actions: &mut crate::game::logic::due::DueActionQueue,
    pending_work: &mut TickPendingWork,
    services: &TickServices,
    tick_budget: Duration,
) {
    let command = run_command_phase(state, rx, due_actions, pending_work, services, tick_budget);
    let due = run_due_action_phase(state, due_actions);
    super::effects::apply_quiescing_effects(
        state,
        services,
        pending_work,
        command.effects,
        due.effects,
    );
}

fn run_simulation_phase(
    state: &Arc<GameState>,
    schedule_clock: &mut ScheduleClock,
    pending_work: &mut TickPendingWork,
    services: &TickServices,
    tick_effects: TickEffects,
    tick_budget: Duration,
    schedule_warn_threshold: Duration,
) -> SimulationPhase {
    let schedule_started_at = Instant::now();
    let online_count = state.online_count();
    let ScheduleTickResult {
        broadcasts: schedule_broadcasts,
        programmator_actions,
        cell_conversions,
        pack_resends,
        sched_select,
        sched_lock_wait,
        sched_run,
        schedule_tail_profile,
        schedule_runs,
        sim_profile,
    } = run_schedule_phase(
        state,
        &services.heartbeat,
        schedule_clock,
        online_count,
        state.player_entity_count(),
        schedule_warn_threshold,
    );
    let schedule_elapsed = schedule_started_at.elapsed();
    let side_started_at = Instant::now();
    let schedule_to_side = side_started_at
        .saturating_duration_since(schedule_started_at)
        .saturating_sub(schedule_elapsed);
    let sched_flush = schedule_elapsed
        .saturating_sub(sched_select)
        .saturating_sub(sched_lock_wait)
        .saturating_sub(sched_run);
    let PreparedEffects {
        effects,
        mut side_profile,
        mut queue_profile,
        programmator_action_profile,
        has_work,
    } = collect_pending_effects(
        state,
        services,
        pending_work,
        EffectSources {
            command_events: tick_effects.command.events,
            command_broadcasts: tick_effects.command.broadcasts,
            due_effects: tick_effects.due,
            schedule_broadcasts,
            pack_resends,
            cell_conversions,
            programmator_actions,
            online_count,
        },
        Instant::now(),
    );
    let side_end = if has_work {
        apply_side_effects(
            state,
            services,
            effects,
            tick_budget,
            &mut side_profile,
            &mut queue_profile,
        );
        let side_end = Instant::now();
        services.heartbeat.mark(TickStage::Summary);
        side_end
    } else {
        services.heartbeat.mark(TickStage::Summary);
        Instant::now()
    };
    let side_elapsed = side_started_at.elapsed();
    SimulationPhase {
        schedule_started_at,
        schedule_elapsed,
        schedule_to_side,
        side_elapsed,
        side_accounting_gap: side_elapsed
            .saturating_sub(side_end.saturating_duration_since(side_started_at)),
        sched_select,
        sched_lock_wait,
        sched_run,
        sched_flush,
        side_profile,
        schedule_tail_profile,
        sim_profile,
        queue_profile,
        programmator_action_profile,
        schedule_runs,
        has_work,
    }
}

#[cfg(test)]
#[path = "tick/tests.rs"]
mod tests;
