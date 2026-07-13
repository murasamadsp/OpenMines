//! Bounded command admission and authoritative dispatch.

use super::{TickPendingWork, TickServices, TickStage};
use crate::game::GameState;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(super) struct CommandPhase {
    pub(super) effects: crate::game::CommandEffects,
    pub(super) started_at: Instant,
    pub(super) elapsed: Duration,
    pub(super) actions: usize,
    pub(super) deferred_commands: usize,
    pub(super) top_command_name: &'static str,
    pub(super) top_command_elapsed: Duration,
}

pub(super) fn run_command_phase(
    state: &Arc<GameState>,
    rx: &mut crate::game::CommandReceivers,
    due_actions: &mut crate::game::logic::due::DueActionQueue,
    pending_work: &mut TickPendingWork,
    services: &TickServices,
    tick_budget: Duration,
) -> CommandPhase {
    let mut effects = crate::game::CommandEffects::default();
    while let Ok(completion) = pending_work.persistence_completions.try_recv() {
        effects.append(crate::game::logic::commands::apply_persistence_completion(
            state, completion,
        ));
    }

    state.refresh_command_ingress_oldest_ages();

    let started_at = Instant::now();
    services.heartbeat.mark(TickStage::Dispatch);
    let mut actions = 0;
    let mut deferred_commands = 0;
    let mut top_command_name = "-";
    let mut top_command_elapsed = Duration::ZERO;
    let simulation = state.config.gameplay.simulation;
    let mut class_remaining = [
        simulation.lifecycle_ingress_batch_budget,
        simulation.gameplay_ingress_batch_budget,
        simulation.internal_ingress_batch_budget,
    ];
    loop {
        let admission =
            take_next_admitted_command(rx, pending_work, &services.persistence, &class_remaining);
        let (queued, persistence_permit) = match admission {
            Ok(Some(admitted)) => (admitted.queued, admitted.permit),
            Ok(None) => break,
            Err(command_name) => {
                deferred_commands = rx
                    .len()
                    .saturating_add(pending_work.building_deletes.len())
                    .saturating_add(pending_work.commands.len())
                    .saturating_add(1);
                crate::metrics::COMMANDS_TOTAL
                    .with_label_values(&[command_name, "persistence_saturated"])
                    .inc();
                break;
            }
        };
        actions += 1;
        let (command_name, apply_started_at) = observe_command_start(state, &queued);
        if let Some(class) = queued.ingress_class {
            class_remaining[class.index()] = class_remaining[class.index()].saturating_sub(1);
        }
        let sequence = queued.sequence;
        let command = queued.command;
        let crate::game::GameCommand::Player(player_command) = command;
        let mut command_effects =
            crate::game::logic::commands::apply_queued_player_command_with_due(
                state,
                queued.player_id,
                queued.session_id,
                player_command,
                sequence,
                due_actions,
            );
        publish_command_saves(persistence_permit, &mut command_effects.saves, command_name);
        effects.append(command_effects);
        let command_elapsed = apply_started_at.elapsed();
        crate::metrics::COMMAND_APPLY_SECONDS
            .with_label_values(&[command_name])
            .observe(command_elapsed.as_secs_f64());
        crate::metrics::COMMANDS_TOTAL
            .with_label_values(&[command_name, "applied"])
            .inc();
        crate::metrics::COMMAND_SEQUENCE.set(i64::try_from(sequence.get()).unwrap_or(i64::MAX));
        if command_elapsed > top_command_elapsed {
            top_command_name = command_name;
            top_command_elapsed = command_elapsed;
        }
        if started_at.elapsed() >= tick_budget
            && (!rx.is_empty()
                || !pending_work.commands.is_empty()
                || !pending_work.building_deletes.is_empty())
        {
            deferred_commands = rx
                .len()
                .saturating_add(pending_work.building_deletes.len())
                .saturating_add(pending_work.commands.len());
            break;
        }
    }

    CommandPhase {
        effects,
        started_at,
        elapsed: started_at.elapsed(),
        actions,
        deferred_commands,
        top_command_name,
        top_command_elapsed,
    }
}

fn take_next_admitted_command(
    rx: &mut crate::game::CommandReceivers,
    pending_work: &mut TickPendingWork,
    persistence: &crate::persistence::PersistenceHandle,
    class_remaining: &[usize; 3],
) -> Result<Option<AdmittedCommand>, &'static str> {
    match take_admitted_internal_building_delete(&mut pending_work.building_deletes, persistence) {
        Ok(Some(admitted)) => Ok(Some(admitted)),
        Ok(None) => {
            take_admitted_command(rx, &mut pending_work.commands, persistence, class_remaining)
        }
        Err(_) => match take_admitted_command(
            rx,
            &mut pending_work.commands,
            persistence,
            class_remaining,
        ) {
            Ok(None) => Err("remove_pack"),
            result => result,
        },
    }
}

fn observe_command_start(
    state: &GameState,
    queued: &crate::game::QueuedGameCommand,
) -> (&'static str, Instant) {
    if let Some(class) = queued.ingress_class {
        state.record_command_dequeued(class);
    }
    let started_at = Instant::now();
    let command_name = queued.command.name();
    let queue_age = started_at
        .saturating_duration_since(queued.enqueued_at)
        .as_secs_f64();
    crate::metrics::COMMAND_QUEUE_RESIDENCE_SECONDS
        .with_label_values(&[command_name])
        .observe(queue_age);
    if let Some(class) = queued.ingress_class {
        crate::metrics::COMMAND_INGRESS_RESIDENCE_SECONDS
            .with_label_values(&[class.metric_name()])
            .observe(queue_age);
    }
    crate::metrics::COMMAND_RECEIVE_TO_APPLY_SECONDS
        .with_label_values(&[command_name])
        .observe(
            started_at
                .saturating_duration_since(queued.received_at)
                .as_secs_f64(),
        );
    (command_name, started_at)
}

pub(super) fn take_admitted_internal_building_delete(
    pending: &mut std::collections::VecDeque<crate::game::QueuedGameCommand>,
    persistence: &crate::persistence::PersistenceHandle,
) -> Result<Option<AdmittedCommand>, &'static str> {
    let Some(queued) = pending.front() else {
        return Ok(None);
    };
    let kind = queued
        .command
        .persistence_kind()
        .expect("internal building delete must be durable");
    match persistence.try_reserve(kind) {
        Ok(permit) => Ok(Some(AdmittedCommand {
            queued: pending
                .pop_front()
                .expect("internal building delete front disappeared"),
            permit: Some(permit),
        })),
        Err(crate::persistence::PersistenceAdmissionError::Full) => Err(queued.command.name()),
        Err(crate::persistence::PersistenceAdmissionError::Closed) => {
            panic!("persistence worker closed before internal building delete admission");
        }
    }
}

pub(super) struct AdmittedCommand {
    pub(super) queued: crate::game::QueuedGameCommand,
    pub(super) permit: Option<crate::persistence::PersistencePermit>,
}

pub(super) fn take_admitted_command(
    rx: &mut crate::game::CommandReceivers,
    pending: &mut super::PendingCommandHeads,
    persistence: &crate::persistence::PersistenceHandle,
    class_remaining: &[usize; 3],
) -> Result<Option<AdmittedCommand>, &'static str> {
    let mut saturated_command = None;
    for _ in 0..3 {
        let class = match pending.next_class {
            0 => crate::game::CommandIngressClass::Lifecycle,
            1 => crate::game::CommandIngressClass::Gameplay,
            2 => crate::game::CommandIngressClass::Internal,
            _ => unreachable!(),
        };
        pending.next_class = (pending.next_class + 1) % 3;
        if class_remaining[class.index()] == 0 {
            continue;
        }
        let Some(queued) = pending
            .take(class)
            .or_else(|| rx.try_recv_class(class).ok())
        else {
            continue;
        };
        let Some(kind) = queued.command.persistence_kind() else {
            return Ok(Some(AdmittedCommand {
                queued,
                permit: None,
            }));
        };
        match persistence.try_reserve(kind) {
            Ok(permit) => {
                return Ok(Some(AdmittedCommand {
                    queued,
                    permit: Some(permit),
                }));
            }
            Err(crate::persistence::PersistenceAdmissionError::Full) => {
                saturated_command = Some(queued.command.name());
                pending.put(queued);
            }
            Err(crate::persistence::PersistenceAdmissionError::Closed) => {
                panic!("persistence worker closed before durable command admission");
            }
        }
    }
    saturated_command.map_or(Ok(None), Err)
}

pub(super) fn publish_command_saves(
    permit: Option<crate::persistence::PersistencePermit>,
    saves: &mut Vec<crate::game::SaveCommand>,
    command_name: &str,
) {
    match (permit, saves.len()) {
        (Some(permit), 1) => permit.publish(saves.pop().expect("one save command")),
        (Some(_) | None, 0) => {}
        (Some(_), count) => {
            panic!("command {command_name} produced {count} saves for one reserved slot")
        }
        (None, count) => {
            panic!("command {command_name} produced {count} saves without persistence admission")
        }
    }
}
