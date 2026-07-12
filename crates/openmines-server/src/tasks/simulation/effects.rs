//! Ordered application of effects produced by commands and schedules.

use super::due::DueEffect;
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
    pub(super) command_broadcasts: Vec<crate::game::BroadcastEffect>,
    pub(super) due_effects: Vec<DueEffect>,
    pub(super) schedule_broadcasts: Vec<crate::game::BroadcastEffect>,
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
    sources: EffectSources,
    now: Instant,
) -> PreparedEffects {
    let mut side_profile = SideProfile::default();
    let dirty_flush_ran =
        flush_due_dirty_snapshots(state, &services.persistence, pending_work, now);
    side_profile.persistence_flush = now.elapsed();

    let started_at = Instant::now();
    let mut broadcasts = sources.command_broadcasts;
    adapt_due_effects(state, sources.due_effects, &mut broadcasts);
    broadcasts.extend(sources.schedule_broadcasts);
    side_profile.broadcasts = started_at.elapsed();

    let started_at = Instant::now();
    pending_work.box_pickups.extend(state.drain_box_pickups());
    apply_pending_box_pickups(
        state,
        &services.persistence,
        &mut pending_work.box_pickups,
        &mut broadcasts,
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
        broadcasts: broadcasts.len(),
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
        broadcasts,
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
    publish_command_events(&services.presentation, command_events);
    side_profile.broadcasts += started_at.elapsed();

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

pub(super) fn apply_shutdown_command_effects(
    state: &Arc<GameState>,
    presentation: &crate::net::presentation::PresentationRuntime,
    effects: crate::game::CommandEffects,
) {
    assert!(
        effects.saves.is_empty(),
        "persistence completion produced durable work after shutdown admission closed"
    );
    publish_command_events(presentation, effects.events);
    broadcast_world_effects(state, effects.broadcasts);
}

fn publish_command_events(
    presentation: &crate::net::presentation::PresentationRuntime,
    events: Vec<crate::game::GameEvent>,
) {
    for event in events {
        presentation.publish(event);
    }
}

fn adapt_due_effects(
    state: &Arc<GameState>,
    effects: Vec<DueEffect>,
    broadcasts: &mut Vec<crate::game::BroadcastEffect>,
) {
    for effect in effects {
        match effect {
            DueEffect::Boom(effect) => adapt_boom_effects(state, effect, broadcasts),
        }
    }
}

fn adapt_boom_effects(
    state: &Arc<GameState>,
    effects: crate::game::logic::consumables::BoomApplyEffects,
    broadcasts: &mut Vec<crate::game::BroadcastEffect>,
) {
    broadcasts.extend(
        effects
            .changed_cells
            .into_iter()
            .map(crate::game::BroadcastEffect::CellUpdate),
    );
    for health_effect in effects.player_health {
        let Some(session_id) = state.active_session_for_player(health_effect.player_id) else {
            continue;
        };
        if let Some(skill_progress) = health_effect.skill_progress {
            let packet = crate::protocol::packets::skills_packet(&skill_progress.entries);
            broadcasts.push(crate::game::BroadcastEffect::Direct {
                session_id,
                data: crate::net::session::wire::make_u_packet_bytes(packet.0, &packet.1),
            });
        }
        let packet =
            crate::protocol::packets::health(health_effect.health, health_effect.max_health);
        broadcasts.push(crate::game::BroadcastEffect::Direct {
            session_id,
            data: crate::net::session::wire::make_u_packet_bytes(packet.0, &packet.1),
        });
    }
    for fx in effects.fx {
        let (position, packet) = match fx {
            crate::game::logic::consumables::BoomFxEffect::Hurt {
                player_id,
                position,
            } => (
                position,
                crate::protocol::packets::hb_hurt_fx(crate::net::session::util::net_u16_nonneg(
                    player_id,
                )),
            ),
            crate::game::logic::consumables::BoomFxEffect::Blast { position } => (
                position,
                crate::protocol::packets::hb_world_blast_fx(
                    crate::net::session::util::net_u16_nonneg(position.0),
                    crate::net::session::util::net_u16_nonneg(position.1),
                    3,
                    0,
                ),
            ),
        };
        broadcasts.push(nearby_hb_effect(position, packet));
    }
    broadcasts.push(crate::game::BroadcastEffect::BlockUpdate(
        effects.cleared_pack,
    ));
}

fn nearby_hb_effect(
    position: crate::game::WorldPos,
    subpacket: Vec<u8>,
) -> crate::game::BroadcastEffect {
    let (chunk_x, chunk_y) = crate::world::World::chunk_pos(position.0, position.1);
    let bundle = crate::protocol::packets::hb_bundle(&[subpacket]);
    crate::game::BroadcastEffect::Nearby {
        cx: chunk_x,
        cy: chunk_y,
        data: crate::net::session::wire::encode_hb_bundle(&bundle.1),
        exclude: None,
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
            crate::game::BroadcastEffect::BlockUpdate(pos) => {
                let (x, y): (i32, i32) = pos.into();
                crate::net::session::social::buildings::broadcast_block_at(state, x, y);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::player::{PlayerCooldowns, PlayerInventory, PlayerPosition, PlayerStats};
    use crate::world::WorldProvider;
    use std::time::Duration;

    #[tokio::test]
    async fn boom_flows_from_admission_through_deadline_to_ordered_wire_effects() {
        let test = crate::test_support::ServerTestHarness::new("boom_e2e", "boom-e2e-user").await;
        let mut receiver = test.connect(1);
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        let player_id = crate::game::PlayerId(test.player.id);
        let entity = test.state.get_player_entity(player_id).unwrap();
        let center = crate::game::WorldPos(10, 11);
        {
            let mut ecs = test.state.ecs.write();
            let mut position = ecs.get_mut::<PlayerPosition>(entity).unwrap();
            position.x = 10;
            position.y = 10;
            position.dir = 0;
            ecs.get_mut::<PlayerStats>(entity).unwrap().health = 40;
            ecs.get_mut::<PlayerCooldowns>(entity)
                .unwrap()
                .last_inventory_use = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
            {
                let mut inventory = ecs.get_mut::<PlayerInventory>(entity).unwrap();
                inventory.selected = 5;
                inventory.items.insert(5, 1);
            }
            drop(ecs);
        }
        for x in (center.0 - 4)..=(center.0 + 4) {
            for y in (center.1 - 4)..=(center.1 + 4) {
                test.state.world.destroy_cell_and_road(x, y);
            }
        }

        let mut due_actions = crate::game::logic::due::DueActionQueue::new(1);
        let admitted = crate::game::logic::commands::apply_player_command_with_due(
            &test.state,
            crate::game::PlayerCommand::InventoryUse {
                session_id: crate::game::SessionId::new(1),
                player_id,
            },
            &mut due_actions,
        );
        broadcast_world_effects(&test.state, admitted.broadcasts);
        let admission_events = crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        assert_eq!(
            admission_events
                .iter()
                .map(|(event, _)| event.as_str())
                .collect::<Vec<_>>(),
            ["HB", "IN"]
        );

        let deadline = due_actions.next_due_at().expect("admitted Boom deadline");
        let due =
            super::super::due::run_due_action_phase_at(&test.state, &mut due_actions, deadline);
        assert_eq!(due.executed, 1);
        assert_eq!(due.effects.len(), 1);
        assert_eq!(due.effects[0].deaths(), &[player_id]);
        let mut broadcasts = Vec::new();
        adapt_due_effects(&test.state, due.effects, &mut broadcasts);
        broadcast_world_effects(&test.state, broadcasts);

        let detonation_events = crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        assert_eq!(
            detonation_events
                .iter()
                .map(|(event, _)| event.as_str())
                .collect::<Vec<_>>(),
            ["@S", "@L", "HB", "HB"]
        );
        assert_eq!(test.state.world.get_solid_cell(center.0, center.1), 0);
        assert!(!test.state.consumable_packs.contains_key(&center));
    }
}
