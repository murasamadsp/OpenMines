//! Ordered application of effects produced by commands and schedules.

use super::profiler::{ProgrammatorActionProfile, QueueProfile, SideProfile};
use super::snapshots::flush_due_dirty_snapshots;
use super::{
    PendingDeathEffect, TickPendingWork, TickServices, TickStage, apply_pending_box_pickups,
    apply_pending_deaths,
};
use crate::game::GameState;
use crate::world::WorldProvider;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(super) struct PendingEffects {
    pub(super) command_events: Vec<crate::game::GameEvent>,
    pub(super) broadcasts: Vec<crate::game::BroadcastEffect>,
    pub(super) pack_resends: Vec<(i32, i32)>,
    pub(super) cell_conversions: Vec<crate::game::PendingConversion>,
    pub(super) programmator_actions: Vec<crate::game::ProgrammatorAction>,
    pub(super) deaths: Vec<PendingDeathEffect>,
    pub(super) bots_render: Vec<crate::game::BotsRenderDue>,
}

impl PendingEffects {
    pub(super) const fn has_work(&self) -> bool {
        !self.command_events.is_empty()
            || !self.broadcasts.is_empty()
            || !self.pack_resends.is_empty()
            || !self.cell_conversions.is_empty()
            || !self.programmator_actions.is_empty()
            || !self.deaths.is_empty()
            || !self.bots_render.is_empty()
    }
}

pub(super) struct EffectSources {
    pub(super) command_events: Vec<crate::game::GameEvent>,
    pub(super) broadcasts: Vec<crate::game::BroadcastEffect>,
    pub(super) pack_resends: Vec<(i32, i32)>,
    pub(super) cell_conversions: Vec<crate::game::PendingConversion>,
    pub(super) programmator_actions: Vec<crate::game::ProgrammatorAction>,
    pub(super) online_count: usize,
}

pub(super) struct PreparedEffects {
    pub(super) effects: PendingEffects,
    pub(super) side_profile: SideProfile,
    pub(super) queue_profile: QueueProfile,
    pub(super) programmator_action_profile: ProgrammatorActionProfile,
    pub(super) has_work: bool,
}

pub(super) fn collect_pending_effects(
    state: &Arc<GameState>,
    services: &TickServices,
    pending_work: &mut TickPendingWork,
    mut sources: EffectSources,
    now: Instant,
) -> PreparedEffects {
    let mut side_profile = SideProfile::default();
    let dirty_flush_ran =
        flush_due_dirty_snapshots(state, &services.persistence, pending_work, now);
    side_profile.persistence_flush = now.elapsed();

    let started_at = Instant::now();
    pending_work.box_pickups.extend(state.drain_box_pickups());
    apply_pending_box_pickups(
        state,
        &services.persistence,
        &mut pending_work.box_pickups,
        &mut sources.broadcasts,
    );
    side_profile.box_pickups = started_at.elapsed();

    let started_at = Instant::now();
    pending_work.deaths.extend(state.drain_player_deaths());
    let deaths = apply_pending_deaths(state, &services.persistence, &mut pending_work.deaths);
    side_profile.death = started_at.elapsed();

    let mut programmator_action_profile = ProgrammatorActionProfile::default();
    for action in &sources.programmator_actions {
        programmator_action_profile.count(action);
    }
    let queue_profile = QueueProfile {
        broadcasts: sources.broadcasts.len(),
        pack_resends: sources.pack_resends.len(),
        cell_conversions_in: sources.cell_conversions.len(),
        programmator_actions: sources.programmator_actions.len(),
        deaths: deaths.len(),
        ..QueueProfile::default()
    };
    let bots_render = if sources.online_count > 0 {
        state.take_due_bots_render(
            Instant::now(),
            crate::game::GameState::BOTS_RENDER_OBSERVER_BUDGET,
        )
    } else {
        Vec::new()
    };
    let effects = PendingEffects {
        command_events: sources.command_events,
        broadcasts: sources.broadcasts,
        pack_resends: sources.pack_resends,
        cell_conversions: sources.cell_conversions,
        programmator_actions: sources.programmator_actions,
        deaths,
        bots_render,
    };
    let has_work = dirty_flush_ran || effects.has_work();
    PreparedEffects {
        effects,
        side_profile,
        queue_profile,
        programmator_action_profile,
        has_work,
    }
}

pub(super) fn apply_side_effects(
    state: &Arc<GameState>,
    services: &TickServices,
    effects: PendingEffects,
    tick_budget: Duration,
    side_profile: &mut SideProfile,
    queue_profile: &mut QueueProfile,
) {
    let PendingEffects {
        command_events,
        broadcasts,
        pack_resends,
        cell_conversions,
        programmator_actions,
        deaths,
        bots_render,
    } = effects;

    let started_at = Instant::now();
    services.heartbeat.mark(TickStage::SideBroadcasts);
    publish_command_events(services, command_events);
    side_profile.broadcasts = started_at.elapsed();

    let started_at = Instant::now();
    services.heartbeat.mark(TickStage::SideBroadcasts);
    broadcast_world_effects(state, broadcasts);
    side_profile.broadcasts += started_at.elapsed();

    let started_at = Instant::now();
    services.heartbeat.mark(TickStage::SidePackResends);
    resend_packs(state, pack_resends);
    side_profile.pack_resends = started_at.elapsed();

    let started_at = Instant::now();
    services.heartbeat.mark(TickStage::SideCellConversions);
    apply_cell_conversions(state, services, cell_conversions, queue_profile);
    side_profile.cell_conversions = started_at.elapsed();

    let started_at = Instant::now();
    services.heartbeat.mark(TickStage::SideProgrammatorActions);
    apply_programmator_actions(state, programmator_actions);
    side_profile.programmator_actions = started_at.elapsed();

    let started_at = Instant::now();
    services.heartbeat.mark(TickStage::SideDeath);
    apply_deaths(state, deaths);
    side_profile.death += started_at.elapsed();

    let started_at = Instant::now();
    services.heartbeat.mark(TickStage::SideBotsRender);
    render_bots(state, bots_render, tick_budget);
    side_profile.bots_render = started_at.elapsed();
}

fn publish_command_events(services: &TickServices, events: Vec<crate::game::GameEvent>) {
    for event in events {
        services.presentation.publish(event);
    }
}

fn broadcast_world_effects(state: &Arc<GameState>, effects: Vec<crate::game::BroadcastEffect>) {
    for effect in effects {
        match effect {
            crate::game::BroadcastEffect::Direct { session_id, data } => {
                if let Some(tx) = state.sessions.outbox_for_session(session_id) {
                    let _ = tx.send(data);
                }
            }
            crate::game::BroadcastEffect::CellUpdate(pos) => {
                let (x, y): (i32, i32) = pos.into();
                crate::game::broadcast_cell_update(state, x, y);
            }
            crate::game::BroadcastEffect::Nearby {
                cx,
                cy,
                data,
                exclude,
            } => state.broadcast_to_nearby(cx, cy, &data, exclude),
        }
    }
}

fn resend_packs(state: &Arc<GameState>, positions: Vec<(i32, i32)>) {
    for (x, y) in positions {
        if let Some(view) = state.get_pack_at(x, y) {
            crate::net::session::social::buildings::broadcast_pack_update(state, &view);
        }
    }
}

fn apply_cell_conversions(
    state: &Arc<GameState>,
    services: &TickServices,
    conversions: Vec<crate::game::PendingConversion>,
    queue_profile: &mut QueueProfile,
) {
    let mut remaining = Vec::new();
    let mut converted_owners = Vec::new();
    for mut conversion in conversions {
        if conversion.ticks_left > 1 {
            conversion.ticks_left -= 1;
            remaining.push(conversion);
            continue;
        }
        let (x, y): (i32, i32) = conversion.pos.into();
        let should_convert = state.world.valid_coord(x, y)
            && state.world.get_cell_typed(x, y) == conversion.required_cell;
        if should_convert {
            state.world.write_world_cell(
                x,
                y,
                crate::world::WorldCell {
                    cell_type: conversion.target_cell,
                    durability: conversion.durability,
                },
            );
            crate::game::broadcast_cell_update(state, x, y);
            queue_profile.cell_conversions_applied += 1;
            converted_owners.push(conversion.owner_pid);
        }
    }
    queue_profile.cell_conversions_remaining = remaining.len();
    update_buildwar_skills(state, services, remaining, converted_owners);
}

fn update_buildwar_skills(
    state: &Arc<GameState>,
    services: &TickServices,
    remaining: Vec<crate::game::PendingConversion>,
    converted_owners: Vec<crate::game::PlayerId>,
) {
    if remaining.is_empty() && converted_owners.is_empty() {
        return;
    }
    let context =
        (!converted_owners.is_empty()).then(|| crate::game::ExpContext::from_state(state));
    services
        .heartbeat
        .mark(TickStage::SideCellConversionsEcsLockWait);
    let mut ecs = state.ecs_write_profiled("tick.side_cell_conversions");
    services.heartbeat.mark(TickStage::SideCellConversions);
    ecs.resource_mut::<crate::game::PendingCellConversions>().0 = remaining;
    let mut packets = Vec::new();
    for owner in converted_owners {
        let Some(entity) = state.get_player_entity(owner) else {
            continue;
        };
        if let Some(mut skills) = ecs.get_mut::<crate::game::player::PlayerSkillsComp>(entity)
            && let Some(context) = context
            && let Some(packet) = context.add_skill_exp(
                &mut skills.states,
                crate::game::skills::SkillType::BuildWar.code(),
                1.0,
            )
        {
            packets.push((owner, packet));
        }
    }
    drop(ecs);
    for (owner, packet) in packets {
        if let Some(tx) = state.player_sender(owner) {
            let _ = tx.send(crate::net::session::wire::make_u_packet_bytes(
                packet.0, &packet.1,
            ));
        }
    }
}

fn apply_programmator_actions(
    state: &Arc<GameState>,
    actions: Vec<crate::game::ProgrammatorAction>,
) {
    for action in actions {
        apply_programmator_action(state, action);
    }
}

fn apply_programmator_action(state: &Arc<GameState>, action: crate::game::ProgrammatorAction) {
    match action {
        crate::game::ProgrammatorAction::Move {
            pid,
            session_id,
            x,
            y,
            dir,
        } => {
            let (tx, _rx) = programmator_action_tx(state, session_id);
            crate::net::session::play::movement::handle_move(state, &tx, pid, 0, x, y, dir, true);
        }
        crate::game::ProgrammatorAction::Dig {
            pid,
            session_id,
            dir,
        } => {
            let (tx, _rx) = programmator_action_tx(state, session_id);
            crate::net::session::play::dig_build::handle_dig(state, &tx, pid, dir, true);
        }
        crate::game::ProgrammatorAction::Build {
            pid,
            session_id,
            dir,
            block_type,
        } => {
            let (tx, _rx) = programmator_action_tx(state, session_id);
            let build = crate::protocol::packets::XbldClient {
                direction: dir,
                block_type: &block_type,
            };
            crate::net::session::play::dig_build::handle_build(state, &tx, pid, &build, true);
        }
        crate::game::ProgrammatorAction::Geo { pid, session_id } => {
            let (tx, _rx) = programmator_action_tx(state, session_id);
            crate::game::logic::commands::apply_programmator_geology(state, &tx, pid);
        }
        crate::game::ProgrammatorAction::Heal { pid, session_id } => {
            let (tx, _rx) = programmator_action_tx(state, session_id);
            crate::game::logic::commands::apply_programmator_heal(state, &tx, pid);
        }
        crate::game::ProgrammatorAction::SetAutoDig {
            pid,
            session_id,
            enabled,
        } => {
            let (tx, _rx) = programmator_action_tx(state, session_id);
            crate::game::logic::commands::apply_programmator_auto_dig_set(state, &tx, pid, enabled);
        }
        crate::game::ProgrammatorAction::SetAggression {
            pid,
            session_id,
            enabled,
        } => {
            let (tx, _rx) = programmator_action_tx(state, session_id);
            crate::game::logic::commands::apply_programmator_aggression_set(
                state, &tx, pid, enabled,
            );
        }
        crate::game::ProgrammatorAction::SetHandMode {
            session_id,
            enabled,
        } => {
            if let Some(tx) = session_id.and_then(|id| state.sessions.outbox_for_session(id)) {
                let packet = crate::protocol::packets::hand_mode(enabled);
                let _ = tx.send(crate::net::session::wire::make_u_packet_bytes(
                    packet.0, &packet.1,
                ));
            }
        }
        crate::game::ProgrammatorAction::FillGun {
            pid,
            session_id,
            x,
            y,
        } => {
            let (tx, _rx) = programmator_action_tx(state, session_id);
            crate::net::session::play::packs::handle_gun_fill_prog(state, &tx, pid, x, y);
        }
        crate::game::ProgrammatorAction::SetProgrammatorStatus {
            session_id,
            running,
        } => {
            if let Some(tx) = session_id.and_then(|id| state.sessions.outbox_for_session(id)) {
                let _ = tx.send(crate::net::session::wire::make_u_packet_bytes(
                    "@P",
                    &crate::protocol::packets::programmator_status(running).1,
                ));
            }
        }
        crate::game::ProgrammatorAction::Send { session_id, data } => {
            if let Some(tx) = state.sessions.outbox_for_session(session_id) {
                let _ = tx.send(data);
            }
        }
    }
}

fn apply_deaths(state: &Arc<GameState>, deaths: Vec<PendingDeathEffect>) {
    for (player_id, respawn_x, respawn_y, max_health, broadcasts) in deaths {
        crate::net::session::play::death::run_death_broadcasts(state, &broadcasts, player_id);
        if let Some(tx) = state.player_sender(player_id) {
            crate::net::session::play::death::send_respawn_after_death(
                &tx,
                player_id,
                respawn_x,
                respawn_y,
                max_health,
                &broadcasts,
            );
            crate::net::session::play::death::broadcast_self_after_respawn(
                state, player_id, respawn_x, respawn_y,
            );
            crate::net::session::play::chunks::check_chunk_changed(state, &tx, player_id);
        }
    }
}

fn render_bots(
    state: &Arc<GameState>,
    due: Vec<crate::game::BotsRenderDue>,
    tick_budget: Duration,
) {
    let result = crate::net::session::play::chunks::bots_render_batch(
        state,
        due,
        crate::game::GameState::BOTS_RENDER_BYTE_BUDGET,
    );
    crate::metrics::BOTS_RENDER_OBSERVERS_TOTAL
        .with_label_values(&["completed"])
        .inc_by(u64::try_from(result.completed.len()).unwrap_or(u64::MAX));
    crate::metrics::BOTS_RENDER_OBSERVERS_TOTAL
        .with_label_values(&["sent"])
        .inc_by(u64::try_from(result.observers_sent).unwrap_or(u64::MAX));
    crate::metrics::BOTS_RENDER_OBSERVERS_TOTAL
        .with_label_values(&["deferred"])
        .inc_by(u64::try_from(result.deferred.len()).unwrap_or(u64::MAX));
    crate::metrics::BOTS_RENDER_BYTES_TOTAL
        .inc_by(u64::try_from(result.bytes_enqueued).unwrap_or(u64::MAX));
    crate::metrics::BOTS_RENDER_SNAPSHOT_CHUNKS
        .set(i64::try_from(result.snapshot_chunks).unwrap_or(i64::MAX));
    let now = Instant::now();
    for observer in result.completed {
        state.reschedule_bots_render(observer, now + crate::game::GameState::BOTS_RENDER_INTERVAL);
    }
    for observer in result.deferred {
        state.reschedule_bots_render(observer, now + tick_budget);
    }
}

fn programmator_action_tx(
    state: &Arc<GameState>,
    session_id: Option<crate::game::SessionId>,
) -> (
    crate::net::session::outbox::Outbox,
    Option<tokio::sync::mpsc::Receiver<Vec<u8>>>,
) {
    session_id
        .and_then(|id| state.sessions.outbox_for_session(id))
        .map_or_else(
            || {
                let (tx, rx) = crate::net::session::outbox::channel();
                (tx, Some(rx))
            },
            |tx| (tx, None),
        )
}
