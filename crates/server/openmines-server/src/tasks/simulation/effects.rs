//! Ordered application of effects produced by commands and schedules.

use super::due::DueEffect;
use super::profiler::{ProgrammatorActionProfile, QueueProfile, SideProfile};
use super::snapshots::flush_due_dirty_snapshots;
use super::{
    PendingDeathEffect, TickHeartbeat, TickPendingWork, TickServices, TickStage,
    apply_pending_box_pickups, apply_pending_deaths,
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
    broadcasts.extend(state.drain_command_broadcasts());
    adapt_due_effects(state, pending_work, sources.due_effects, &mut broadcasts);
    broadcasts.extend(sources.schedule_broadcasts);
    side_profile.broadcasts = started_at.elapsed();

    // Respawn is a liveness boundary and must not wait behind pickup bursts.
    let started_at = Instant::now();
    pending_work.deaths.extend(state.drain_player_deaths());
    let mut command_events = sources.command_events;
    let deaths = apply_pending_deaths(
        state,
        &services.persistence,
        &mut pending_work.deaths,
        &mut command_events,
    );
    side_profile.death = started_at.elapsed();

    let started_at = Instant::now();
    pending_work.box_pickups.extend(state.drain_box_pickups());
    apply_pending_box_pickups(
        state,
        &services.persistence,
        &mut pending_work.box_pickups,
        &mut broadcasts,
    );
    side_profile.box_pickups = started_at.elapsed();

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
        command_events,
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
    publish_world_effects(&services.presentation, broadcasts);
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
    apply_programmator_actions(state, &services.presentation, programmator_actions);
    side_profile.programmator_actions = started_at.elapsed();

    let started_at = Instant::now();
    services.heartbeat.mark(TickStage::SideDeath);
    apply_deaths(state, &services.presentation, deaths);
    side_profile.death += started_at.elapsed();

    let started_at = Instant::now();
    services.heartbeat.mark(TickStage::SideBotsRender);
    render_bots(state, &services.presentation, bots_render, tick_budget);
    side_profile.bots_render = started_at.elapsed();
}

pub(super) fn apply_shutdown_command_effects(
    _state: &Arc<GameState>,
    presentation: &crate::net::presentation::PresentationRuntime,
    effects: crate::game::CommandEffects,
) {
    assert!(
        effects.saves.is_empty(),
        "persistence completion produced durable work after shutdown admission closed"
    );
    publish_command_events(presentation, effects.events);
    publish_world_effects(presentation, effects.broadcasts);
}

pub(super) fn apply_quiescing_effects(
    state: &Arc<GameState>,
    services: &TickServices,
    pending_work: &mut TickPendingWork,
    command_effects: crate::game::CommandEffects,
    due_effects: Vec<DueEffect>,
) {
    let mut broadcasts = command_effects.broadcasts;
    adapt_due_effects(state, pending_work, due_effects, &mut broadcasts);

    pending_work.deaths.extend(state.drain_player_deaths());
    let mut command_events = command_effects.events;
    let deaths = apply_pending_deaths(
        state,
        &services.persistence,
        &mut pending_work.deaths,
        &mut command_events,
    );
    pending_work.box_pickups.extend(state.drain_box_pickups());
    apply_pending_box_pickups(
        state,
        &services.persistence,
        &mut pending_work.box_pickups,
        &mut broadcasts,
    );

    let mut side_profile = SideProfile::default();
    let mut queue_profile = QueueProfile::default();
    apply_side_effects(
        state,
        services,
        PendingEffects {
            command_events,
            broadcasts,
            pack_resends: Vec::new(),
            cell_conversions: Vec::new(),
            programmator_actions: Vec::new(),
            deaths,
            bots_render: Vec::new(),
        },
        Duration::ZERO,
        &mut side_profile,
        &mut queue_profile,
    );
}

fn publish_command_events(
    presentation: &crate::net::presentation::PresentationRuntime,
    events: Vec<crate::game::GameEvent>,
) {
    for event in events {
        presentation.publish(event);
    }
}

fn publish_world_effects(
    presentation: &crate::net::presentation::PresentationRuntime,
    effects: Vec<crate::game::BroadcastEffect>,
) {
    if !effects.is_empty() {
        presentation.publish(crate::game::GameEvent::WorldEffects { effects });
    }
}

fn adapt_due_effects(
    state: &Arc<GameState>,
    pending_work: &mut TickPendingWork,
    effects: Vec<DueEffect>,
    broadcasts: &mut Vec<crate::game::BroadcastEffect>,
) {
    for effect in effects {
        for &player_id in effect.deaths() {
            state.request_player_death(player_id);
        }
        match effect {
            DueEffect::Boom(effect) => adapt_boom_effects(state, effect, broadcasts),
            DueEffect::Protector(effect) | DueEffect::Raz(effect) => {
                adapt_area_consumable_effects(state, pending_work, effect, broadcasts);
            }
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
    adapt_consumable_health(state, &effects.player_health, broadcasts);
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

fn adapt_consumable_health(
    state: &GameState,
    effects: &[crate::game::logic::consumables::ConsumablePlayerHealthEffect],
    broadcasts: &mut Vec<crate::game::BroadcastEffect>,
) {
    for health_effect in effects {
        let Some(session_id) = state.active_session_for_player(health_effect.player_id) else {
            continue;
        };
        if let Some(skill_progress) = &health_effect.skill_progress {
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
}

fn adapt_area_consumable_effects(
    state: &Arc<GameState>,
    pending_work: &mut TickPendingWork,
    effects: crate::game::logic::consumables::AreaConsumableApplyEffects,
    broadcasts: &mut Vec<crate::game::BroadcastEffect>,
) {
    broadcasts.extend(
        effects
            .changed_cells
            .into_iter()
            .map(crate::game::BroadcastEffect::CellUpdate),
    );
    adapt_consumable_health(state, &effects.player_health, broadcasts);
    broadcasts.extend(
        effects
            .player_health
            .iter()
            .filter(|effect| effect.health > 0)
            .map(|effect| {
                nearby_hb_effect(
                    effect.position,
                    crate::protocol::packets::hb_hurt_fx(
                        crate::net::session::util::net_u16_nonneg(effect.player_id),
                    ),
                )
            }),
    );
    let blast = crate::protocol::packets::hb_world_blast_fx(
        crate::net::session::util::net_u16_nonneg(effects.cleared_pack.0),
        crate::net::session::util::net_u16_nonneg(effects.cleared_pack.1),
        effects.blast_direction,
        effects.blast_color,
    );
    broadcasts.push(nearby_hb_effect(effects.cleared_pack, blast));
    for remove in effects.building_removals {
        let now = Instant::now();
        pending_work
            .building_deletes
            .push_back(crate::game::QueuedGameCommand {
                player_id: remove
                    .cause
                    .trigger_player_id()
                    .unwrap_or(crate::game::PlayerId(0)),
                session_id: crate::game::SessionId::new(0),
                ingress_class: None,
                sequence: state.allocate_command_sequence(),
                received_at: now,
                enqueued_at: now,
                command: crate::game::GameCommand::Player(crate::game::PlayerCommand::RemovePack {
                    remove,
                }),
            });
    }
    broadcasts.extend(
        effects
            .pack_resends
            .into_iter()
            .map(crate::game::BroadcastEffect::BlockUpdate),
    );
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

#[cfg(test)]
mod hb_batch_tests {
    use crate::net::presentation::hb_payload;
    use crate::net::session::wire::make_b_packet_bytes;

    #[test]
    fn hb_payload_extracts_only_complete_hb_frame() {
        let encoded = make_b_packet_bytes("HB", &[b'F', 1, 2, 3, 4]);
        assert_eq!(hb_payload(&encoded), Some(&[b'F', 1, 2, 3, 4][..]));
        assert_eq!(hb_payload(&make_b_packet_bytes("BI", &[])), None);
        assert_eq!(hb_payload(&encoded[..6]), None);
    }
}

fn resend_packs(state: &Arc<GameState>, positions: Vec<(i32, i32)>) {
    for (x, y) in positions {
        if let Some(view) = state.get_pack_at(x, y) {
            crate::game::logic::buildings::broadcast_pack_update(state, &view);
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
    update_buildwar_skills(
        state,
        &services.heartbeat,
        &services.presentation,
        remaining,
        converted_owners,
    );
}

fn update_buildwar_skills(
    state: &Arc<GameState>,
    heartbeat: &TickHeartbeat,
    presentation: &crate::net::presentation::PresentationRuntime,
    remaining: Vec<crate::game::PendingConversion>,
    converted_owners: Vec<crate::game::PlayerId>,
) {
    if remaining.is_empty() && converted_owners.is_empty() {
        return;
    }
    let context =
        (!converted_owners.is_empty()).then(|| crate::game::ExpContext::from_state(state));
    heartbeat.mark(TickStage::SideCellConversionsEcsLockWait);
    let mut ecs = state.ecs_write_profiled("tick.side_cell_conversions");
    heartbeat.mark(TickStage::SideCellConversions);
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
        if let Some(session_id) = state.sessions.session_for_player(owner) {
            presentation.publish(crate::game::GameEvent::SessionBatch {
                session_id,
                player_id: owner,
                packets: vec![crate::net::session::wire::make_u_packet_bytes(
                    packet.0, &packet.1,
                )],
            });
        }
    }
}

fn apply_programmator_actions(
    state: &Arc<GameState>,
    presentation: &crate::net::presentation::PresentationRuntime,
    actions: Vec<crate::game::ProgrammatorAction>,
) {
    for action in actions {
        apply_programmator_action(state, presentation, action);
    }
}

fn apply_programmator_action(
    state: &Arc<GameState>,
    presentation: &crate::net::presentation::PresentationRuntime,
    action: crate::game::ProgrammatorAction,
) {
    match action {
        crate::game::ProgrammatorAction::Move {
            pid,
            session_id,
            x,
            y,
            dir,
        } => apply_programmator_move(state, presentation, pid, session_id, x, y, dir),
        crate::game::ProgrammatorAction::Dig {
            pid,
            session_id,
            dir,
        } => capture_programmator_packets(presentation, session_id, pid, |tx| {
            crate::game::logic::dig_build::handle_dig(state, tx, pid, dir, true);
        }),
        crate::game::ProgrammatorAction::Build {
            pid,
            session_id,
            dir,
            block_type,
        } => capture_programmator_packets(presentation, session_id, pid, |tx| {
            let build = crate::protocol::packets::XbldClient {
                direction: dir,
                block_type: &block_type,
            };
            crate::game::logic::dig_build::handle_build(state, tx, pid, &build, true);
        }),
        crate::game::ProgrammatorAction::Geo { pid, session_id } => {
            capture_programmator_packets(presentation, session_id, pid, |tx| {
                crate::game::logic::commands::apply_programmator_geology(state, tx, pid);
            });
        }
        crate::game::ProgrammatorAction::Heal { pid, session_id } => {
            capture_programmator_packets(presentation, session_id, pid, |tx| {
                crate::game::logic::commands::apply_programmator_heal(state, tx, pid);
            });
        }
        crate::game::ProgrammatorAction::SetAutoDig {
            pid,
            session_id,
            enabled,
        } => capture_programmator_packets(presentation, session_id, pid, |tx| {
            crate::game::logic::commands::apply_programmator_auto_dig_set(state, tx, pid, enabled);
        }),
        crate::game::ProgrammatorAction::SetAggression {
            pid,
            session_id,
            enabled,
        } => capture_programmator_packets(presentation, session_id, pid, |tx| {
            crate::game::logic::commands::apply_programmator_aggression_set(
                state, tx, pid, enabled,
            );
        }),
        crate::game::ProgrammatorAction::SetHandMode {
            session_id,
            enabled,
        } => {
            let packet = crate::protocol::packets::hand_mode(enabled);
            publish_programmator_packet(
                presentation,
                session_id,
                crate::net::session::wire::make_u_packet_bytes(packet.0, &packet.1),
            );
        }
        crate::game::ProgrammatorAction::FillGun {
            pid,
            session_id,
            x,
            y,
        } => capture_programmator_packets(presentation, session_id, pid, |tx| {
            crate::game::logic::packs::handle_gun_fill_prog(state, tx, pid, x, y);
        }),
        crate::game::ProgrammatorAction::SetProgrammatorStatus {
            session_id,
            running,
        } => {
            publish_programmator_packet(
                presentation,
                session_id,
                crate::net::session::wire::make_u_packet_bytes(
                    "@P",
                    &crate::protocol::packets::programmator_status(running).1,
                ),
            );
        }
        crate::game::ProgrammatorAction::Send { session_id, data } => {
            publish_programmator_packet(presentation, Some(session_id), data);
        }
    }
}

fn publish_programmator_packet(
    presentation: &crate::net::presentation::PresentationRuntime,
    session_id: Option<crate::game::SessionId>,
    data: Vec<u8>,
) {
    if let Some(session_id) = session_id {
        presentation.publish(crate::game::GameEvent::Fanout {
            recipients: vec![session_id],
            data,
        });
    }
}

fn capture_programmator_packets<F>(
    presentation: &crate::net::presentation::PresentationRuntime,
    session_id: Option<crate::game::SessionId>,
    player_id: crate::game::PlayerId,
    apply: F,
) where
    F: FnOnce(&crate::net::session::outbox::Outbox),
{
    let (tx, mut rx) = crate::net::session::outbox::channel();
    apply(&tx);
    let Some(session_id) = session_id else {
        return;
    };
    let mut packets = Vec::new();
    while let Ok(packet) = rx.try_recv() {
        packets.push(packet);
    }
    if !packets.is_empty() {
        presentation.publish(crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets,
        });
    }
}

fn apply_programmator_move(
    state: &Arc<GameState>,
    presentation: &crate::net::presentation::PresentationRuntime,
    player_id: crate::game::PlayerId,
    session_id: Option<crate::game::SessionId>,
    x: i32,
    y: i32,
    direction: i32,
) {
    let Some(session_id) = session_id else {
        // Offline programmator actors still advance authoritative state; without
        // a session there is no presentation target.
        let (tx, _rx) = crate::net::session::outbox::channel();
        crate::game::logic::movement::handle_move(state, &tx, player_id, 0, x, y, direction, true);
        return;
    };
    let effects = crate::game::logic::movement::apply_move_command(
        state,
        player_id,
        session_id,
        crate::game::logic::movement::MoveRequest {
            target_x: x,
            target_y: y,
            direction,
            programmatic: true,
        },
    );
    debug_assert!(effects.saves.is_empty());
    debug_assert!(effects.broadcasts.is_empty());
    publish_command_events(presentation, effects.events);
}

fn apply_deaths(
    state: &Arc<GameState>,
    presentation: &crate::net::presentation::PresentationRuntime,
    deaths: Vec<PendingDeathEffect>,
) {
    for (player_id, respawn_x, respawn_y, max_health, broadcasts) in deaths {
        if let Some(entity) = state.get_player_entity(player_id) {
            state.schedule_hazard(entity, std::time::Instant::now());
        }
        state.seed_granular_region(respawn_x, respawn_y);
        state.seed_alive_region(respawn_x, respawn_y);
        crate::game::logic::death::run_death_broadcasts(state, &broadcasts, player_id);
        let batch = crate::net::session::wire::PacketBatch::default();
        crate::game::logic::death::send_respawn_after_death(
            &batch,
            player_id,
            respawn_x,
            respawn_y,
            max_health,
            &broadcasts,
        );
        crate::game::logic::death::broadcast_self_after_respawn(
            state, player_id, respawn_x, respawn_y,
        );
        crate::game::logic::chunks::check_chunk_changed(state, &batch, player_id);
        if let Some(session_id) = state.sessions.session_for_player(player_id) {
            let packets = batch.into_packets();
            if !packets.is_empty() {
                presentation.publish(crate::game::GameEvent::SessionBatch {
                    session_id,
                    player_id,
                    packets,
                });
            }
        }
    }
}

fn render_bots(
    state: &Arc<GameState>,
    presentation: &crate::net::presentation::PresentationRuntime,
    due: Vec<crate::game::BotsRenderDue>,
    tick_budget: Duration,
) {
    state.refresh_active_bots_render_players();
    let result = crate::game::logic::chunks::bots_render_batch(
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
    for (session_id, player_id, data) in result.deliveries {
        presentation.publish(crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: vec![data],
        });
    }
    let now = Instant::now();
    for observer in result.completed {
        state.reschedule_bots_render(observer, now + crate::game::GameState::BOTS_RENDER_INTERVAL);
    }
    for observer in result.deferred {
        state.reschedule_bots_render(observer, now + tick_budget);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::player::{PlayerCooldowns, PlayerInventory, PlayerPosition, PlayerStats};
    use crate::world::WorldProvider;
    use std::time::Duration;

    fn prepare_delayed_consumable(
        state: &Arc<GameState>,
        player_id: crate::game::PlayerId,
        item_id: i32,
        health: i32,
    ) -> crate::game::WorldPos {
        let center = crate::game::WorldPos(10, 11);
        state
            .modify_player(player_id, |ecs, entity| {
                {
                    let mut position = ecs.get_mut::<PlayerPosition>(entity)?;
                    position.x = 10;
                    position.y = 10;
                    position.dir = 0;
                }
                {
                    let mut player_stats = ecs.get_mut::<PlayerStats>(entity)?;
                    player_stats.health = health;
                    player_stats.max_health = health;
                }
                ecs.get_mut::<PlayerCooldowns>(entity)?.last_inventory_use =
                    Instant::now().checked_sub(Duration::from_secs(1))?;
                let mut inventory = ecs.get_mut::<PlayerInventory>(entity)?;
                inventory.selected = item_id;
                inventory.items.insert(item_id, 1);
                Some(())
            })
            .unwrap();
        for x in (center.0 - 1)..=(center.0 + 1) {
            for y in (center.1 - 1)..=(center.1 + 1) {
                state.world.destroy_cell_and_road(x, y);
            }
        }
        center
    }

    fn event_names(events: &[(String, Vec<u8>)]) -> Vec<&str> {
        events.iter().map(|(event, _)| event.as_str()).collect()
    }

    fn hb_tags(events: &[(String, Vec<u8>)]) -> Vec<u8> {
        events
            .iter()
            .filter(|(event, _)| event == "HB")
            .map(|(_, payload)| payload[0])
            .collect()
    }

    #[tokio::test]
    async fn programmator_send_is_delivered_by_presentation_worker() {
        let test =
            crate::test_support::ServerTestHarness::new("programmator_present", "prog").await;
        let session_id = crate::game::SessionId::new(1);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        let presentation = crate::net::presentation::PresentationRuntime::start(test.state.clone());
        let expected = crate::net::session::wire::make_u_packet_bytes("OK", b"program");

        apply_programmator_action(
            &test.state,
            &presentation,
            crate::game::ProgrammatorAction::Send {
                session_id,
                data: expected.clone(),
            },
        );

        let delivered = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
            .await
            .expect("presentation worker did not deliver programmator packet")
            .expect("test outbox closed");
        assert_eq!(delivered, expected);
        presentation.shutdown();
    }

    #[tokio::test]
    async fn buildwar_skill_update_uses_typed_session_effect() {
        let test = crate::test_support::ServerTestHarness::new("buildwar_effect", "builder").await;
        let player_id = crate::game::PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(1);
        let mut receiver = test.connect(session_id.get());
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        test.state.modify_player(player_id, |ecs, entity| {
            ecs.get_mut::<crate::game::player::PlayerSkillsComp>(entity)
                .map(|mut skills| {
                    skills.states.skills.insert(
                        0,
                        crate::db::SkillEntry {
                            code: crate::game::skills::SkillType::BuildWar.code().to_owned(),
                            level: 1,
                            exp: 0.0,
                        },
                    );
                })
        });

        let presentation = crate::net::presentation::PresentationRuntime::start(test.state.clone());
        let heartbeat = super::super::TickHeartbeat::new(Instant::now());
        update_buildwar_skills(
            &test.state,
            &heartbeat,
            &presentation,
            Vec::new(),
            vec![player_id],
        );

        let packet = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
            .await
            .expect("presentation worker did not deliver BuildWar update")
            .expect("test outbox closed");
        let mut encoded = bytes::BytesMut::from(packet.as_slice());
        let decoded = openmines_protocol::Packet::try_decode(&mut encoded)
            .expect("BuildWar packet must decode")
            .expect("BuildWar packet must be complete");
        assert_eq!(decoded.event_name, *b"@S");
        presentation.shutdown();
    }

    #[tokio::test]
    async fn nearby_hb_effects_are_batched_per_recipient() {
        let test =
            crate::test_support::ServerTestHarness::new("nearby_hb_batch", "batch-user").await;
        let mut receiver = test.connect(1);
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        let player_id = crate::game::PlayerId(test.player.id);
        let pos = test
            .state
            .query_player(player_id, |ecs, entity| {
                ecs.get::<PlayerPosition>(entity)
                    .map(|position| (position.x, position.y))
            })
            .flatten()
            .expect("connected player position");
        let (cx, cy) = crate::world::World::chunk_pos(pos.0, pos.1);
        let first = vec![b'F', 1, 2, 3, 4];
        let second = vec![b'Z', 5, 6, 7, 8];

        crate::net::presentation::deliver_world_effects_for_test(
            &test.state,
            vec![
                crate::game::BroadcastEffect::Nearby {
                    cx,
                    cy,
                    data: crate::net::session::wire::make_b_packet_bytes("HB", &first),
                    exclude: None,
                },
                crate::game::BroadcastEffect::Nearby {
                    cx,
                    cy,
                    data: crate::net::session::wire::make_b_packet_bytes("HB", &second),
                    exclude: None,
                },
            ],
        );

        let events = crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        assert_eq!(event_names(&events), ["HB"]);
        assert_eq!(events[0].1, [first, second].concat());
    }

    #[tokio::test]
    async fn direct_effect_flushes_the_preceding_hb_batch() {
        let test =
            crate::test_support::ServerTestHarness::new("hb_direct_barrier", "barrier-user").await;
        let mut receiver = test.connect(1);
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        let player_id = crate::game::PlayerId(test.player.id);
        let pos = test
            .state
            .query_player(player_id, |ecs, entity| {
                ecs.get::<PlayerPosition>(entity)
                    .map(|position| (position.x, position.y))
            })
            .flatten()
            .expect("connected player position");
        let (cx, cy) = crate::world::World::chunk_pos(pos.0, pos.1);
        let session_id = crate::game::SessionId::new(1);

        crate::net::presentation::deliver_world_effects_for_test(
            &test.state,
            vec![
                crate::game::BroadcastEffect::Nearby {
                    cx,
                    cy,
                    data: crate::net::session::wire::make_b_packet_bytes("HB", b"F"),
                    exclude: None,
                },
                crate::game::BroadcastEffect::Direct {
                    session_id,
                    data: crate::net::session::wire::make_b_packet_bytes("BI", &[1]),
                },
                crate::game::BroadcastEffect::Nearby {
                    cx,
                    cy,
                    data: crate::net::session::wire::make_b_packet_bytes("HB", b"Z"),
                    exclude: None,
                },
            ],
        );

        let events = crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        assert_eq!(event_names(&events), ["HB", "BI", "HB"]);
        assert_eq!(hb_tags(&events), [b'F', b'Z']);
    }

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
            player_id,
            crate::game::SessionId::new(1),
            crate::game::PlayerCommand::InventoryUse,
            &mut due_actions,
        );
        crate::net::presentation::deliver_world_effects_for_test(&test.state, admitted.broadcasts);
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
        let (_, completions) = tokio::sync::mpsc::channel(1);
        let mut pending_work = TickPendingWork::new(completions);
        adapt_due_effects(&test.state, &mut pending_work, due.effects, &mut broadcasts);
        crate::net::presentation::deliver_world_effects_for_test(&test.state, broadcasts);

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

    #[tokio::test]
    async fn protector_flows_from_admission_through_exact_deadline_to_ordered_wire_effects() {
        let test =
            crate::test_support::ServerTestHarness::new("protector_e2e", "protector-e2e-user")
                .await;
        let mut receiver = test.connect(1);
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        let player_id = crate::game::PlayerId(test.player.id);
        let center = prepare_delayed_consumable(&test.state, player_id, 6, 100);

        let mut due_actions = crate::game::logic::due::DueActionQueue::new(1);
        let admitted_before = Instant::now();
        let admitted = crate::game::logic::commands::apply_player_command_with_due(
            &test.state,
            player_id,
            crate::game::SessionId::new(1),
            crate::game::PlayerCommand::InventoryUse,
            &mut due_actions,
        );
        let admitted_after = Instant::now();
        crate::net::presentation::deliver_world_effects_for_test(&test.state, admitted.broadcasts);
        let admission_events = crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        assert_eq!(event_names(&admission_events), ["HB", "IN"]);
        assert_eq!(hb_tags(&admission_events), [b'O']);
        assert_eq!(
            test.state.consumable_packs.get(&center).map(|pack| *pack),
            Some((b'B', 1))
        );

        let deadline = due_actions
            .next_due_at()
            .expect("admitted Protector deadline");
        assert!(deadline >= admitted_before + Duration::from_secs(2));
        assert!(deadline <= admitted_after + Duration::from_secs(2));
        let early = super::super::due::run_due_action_phase_at(
            &test.state,
            &mut due_actions,
            deadline.checked_sub(Duration::from_nanos(1)).unwrap(),
        );
        assert_eq!(early.executed, 0);
        assert!(early.effects.is_empty());
        assert_eq!(due_actions.len(), 1);
        assert_eq!(
            test.state.query_player_opt(player_id, |ecs, entity| {
                Some(ecs.get::<PlayerStats>(entity)?.health)
            }),
            Some(100)
        );
        assert!(crate::test_support::ServerTestHarness::drain_events(&mut receiver).is_empty());

        let due =
            super::super::due::run_due_action_phase_at(&test.state, &mut due_actions, deadline);
        assert_eq!(due.executed, 1);
        assert_eq!(due_actions.len(), 0);
        assert!(matches!(
            due.effects.as_slice(),
            [DueEffect::Protector(effect)]
                if effect.deaths.is_empty() && effect.cleared_pack == center
        ));
        let mut broadcasts = Vec::new();
        let (_, completions) = tokio::sync::mpsc::channel(1);
        let mut pending_work = TickPendingWork::new(completions);
        adapt_due_effects(&test.state, &mut pending_work, due.effects, &mut broadcasts);
        crate::net::presentation::deliver_world_effects_for_test(&test.state, broadcasts);

        let detonation_events = crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        assert_eq!(event_names(&detonation_events), ["@S", "@L", "HB", "HB"]);
        assert_eq!(hb_tags(&detonation_events), [b'D', b'O']);
        assert!(!test.state.consumable_packs.contains_key(&center));
    }

    #[tokio::test]
    async fn lethal_raz_flows_from_admission_through_exact_deadline_to_death_queue() {
        let test = crate::test_support::ServerTestHarness::new("raz_e2e", "raz-e2e-user").await;
        let mut receiver = test.connect(1);
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        let player_id = crate::game::PlayerId(test.player.id);
        let center = prepare_delayed_consumable(&test.state, player_id, 7, 500);

        let mut due_actions = crate::game::logic::due::DueActionQueue::new(1);
        let admitted_before = Instant::now();
        let admitted = crate::game::logic::commands::apply_player_command_with_due(
            &test.state,
            player_id,
            crate::game::SessionId::new(1),
            crate::game::PlayerCommand::InventoryUse,
            &mut due_actions,
        );
        let admitted_after = Instant::now();
        crate::net::presentation::deliver_world_effects_for_test(&test.state, admitted.broadcasts);
        let admission_events = crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        assert_eq!(event_names(&admission_events), ["HB", "IN"]);
        assert_eq!(hb_tags(&admission_events), [b'O']);
        assert_eq!(
            test.state.consumable_packs.get(&center).map(|pack| *pack),
            Some((b'B', 2))
        );

        let deadline = due_actions.next_due_at().expect("admitted Raz deadline");
        assert!(deadline >= admitted_before + Duration::from_secs(5));
        assert!(deadline <= admitted_after + Duration::from_secs(5));
        let early = super::super::due::run_due_action_phase_at(
            &test.state,
            &mut due_actions,
            deadline.checked_sub(Duration::from_nanos(1)).unwrap(),
        );
        assert_eq!(early.executed, 0);
        assert!(early.effects.is_empty());
        assert_eq!(due_actions.len(), 1);
        assert_eq!(
            test.state.query_player_opt(player_id, |ecs, entity| {
                Some(ecs.get::<PlayerStats>(entity)?.health)
            }),
            Some(500)
        );
        assert!(crate::test_support::ServerTestHarness::drain_events(&mut receiver).is_empty());

        let due =
            super::super::due::run_due_action_phase_at(&test.state, &mut due_actions, deadline);
        assert_eq!(due.executed, 1);
        assert_eq!(due_actions.len(), 0);
        assert!(matches!(
            due.effects.as_slice(),
            [DueEffect::Raz(effect)]
                if effect.deaths == [player_id] && effect.cleared_pack == center
        ));
        assert!(test.state.drain_player_deaths().is_empty());
        let mut broadcasts = Vec::new();
        let (_, completions) = tokio::sync::mpsc::channel(1);
        let mut pending_work = TickPendingWork::new(completions);
        adapt_due_effects(&test.state, &mut pending_work, due.effects, &mut broadcasts);
        assert_eq!(test.state.drain_player_deaths(), [player_id]);
        crate::net::presentation::deliver_world_effects_for_test(&test.state, broadcasts);

        let detonation_events = crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        assert_eq!(event_names(&detonation_events), ["@S", "@L", "HB", "HB"]);
        assert_eq!(hb_tags(&detonation_events), [b'D', b'O']);
        assert!(!test.state.consumable_packs.contains_key(&center));
    }
}
