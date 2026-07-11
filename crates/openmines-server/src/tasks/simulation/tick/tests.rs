use super::*;
use crate::game::ScheduleActivity;
use crate::tasks::simulation::commands::{
    AdmittedCommand, publish_command_saves, take_admitted_command,
};
use crate::tasks::simulation::profiler::{
    SideProfile, TickExecutionClass, classify_tick_execution,
};
use crate::tasks::simulation::scheduler::{ScheduleCandidate, ScheduleWorkload};
use crate::tasks::simulation::snapshots::{
    PLAYER_DIRTY_FLUSH_INTERVAL, flush_dirty_buildings_once, flush_due_dirty_snapshots,
};
use crate::tasks::simulation::{
    BoxPickupBacklog, DeathBacklog, apply_pending_box_pickups, apply_pending_deaths,
};
use crate::world::WorldProvider as _;
use bytes::BytesMut;
use std::sync::Arc;
use tokio::sync::mpsc;

fn candidate(
    _name: &'static str,
    activity: ScheduleActivity,
    interval_ms: u64,
) -> ScheduleCandidate {
    ScheduleCandidate {
        activity,
        interval: Duration::from_millis(interval_ms),
    }
}

#[test]
fn schedule_clock_skips_idle_world_without_catchup() {
    let base = Instant::now();
    let mut clock = ScheduleClock::new(2, base);
    let schedules = [
        candidate("hazards", ScheduleActivity::OnlinePlayers, 10),
        candidate("physics", ScheduleActivity::PlayerEntities, 10),
    ];

    let due = clock.select_due(
        schedules.len(),
        base + Duration::from_millis(11),
        ScheduleWorkload {
            online_count: 0,
            player_entity_count: 0,
            crafting_due: false,
        },
        |idx| schedules.get(idx).copied(),
    );
    assert!(due.is_empty());

    let due = clock.select_due(
        schedules.len(),
        base + Duration::from_millis(12),
        ScheduleWorkload {
            online_count: 1,
            player_entity_count: 1,
            crafting_due: false,
        },
        |idx| schedules.get(idx).copied(),
    );
    assert!(
        due.is_empty(),
        "idle skip must reset last_run instead of catching up immediately"
    );

    let due = clock.select_due(
        schedules.len(),
        base + Duration::from_millis(21),
        ScheduleWorkload {
            online_count: 1,
            player_entity_count: 1,
            crafting_due: false,
        },
        |idx| schedules.get(idx).copied(),
    );
    assert_eq!(due, vec![0, 1]);
}

#[test]
fn schedule_activity_defines_idle_behavior_without_name_matching() {
    let base = Instant::now();
    let schedules = [
        candidate("renamed_online_work", ScheduleActivity::OnlinePlayers, 10),
        candidate("renamed_entity_work", ScheduleActivity::PlayerEntities, 10),
        candidate("renamed_durable_work", ScheduleActivity::Always, 10),
        candidate("renamed_crafting_work", ScheduleActivity::DueCrafting, 10),
    ];

    let mut clock = ScheduleClock::new(schedules.len(), base);
    let due = clock.select_due(
        schedules.len(),
        base + Duration::from_millis(11),
        ScheduleWorkload {
            online_count: 0,
            player_entity_count: 1,
            crafting_due: false,
        },
        |idx| schedules.get(idx).copied(),
    );
    assert_eq!(due, vec![1, 2]);

    let mut clock = ScheduleClock::new(schedules.len(), base);
    let due = clock.select_due(
        schedules.len(),
        base + Duration::from_millis(11),
        ScheduleWorkload {
            online_count: 0,
            player_entity_count: 0,
            crafting_due: false,
        },
        |idx| schedules.get(idx).copied(),
    );
    assert_eq!(due, vec![2]);

    let mut clock = ScheduleClock::new(schedules.len(), base);
    let due = clock.select_due(
        schedules.len(),
        base + Duration::from_millis(11),
        ScheduleWorkload {
            online_count: 0,
            player_entity_count: 0,
            crafting_due: true,
        },
        |idx| schedules.get(idx).copied(),
    );
    assert_eq!(due, vec![2, 3]);
}

#[test]
fn schedule_clock_preserves_disabled_schedule_slots() {
    let base = Instant::now();
    let mut clock = ScheduleClock::new(3, base);
    let schedules = [
        Some(candidate("hazards", ScheduleActivity::OnlinePlayers, 10)),
        None,
        Some(candidate(
            "building_crafting",
            ScheduleActivity::DueCrafting,
            10,
        )),
    ];

    let due = clock.select_due(
        schedules.len(),
        base + Duration::from_millis(11),
        ScheduleWorkload {
            online_count: 0,
            player_entity_count: 0,
            crafting_due: true,
        },
        |idx| schedules.get(idx).copied().flatten(),
    );
    assert_eq!(due, vec![2]);
    assert_eq!(clock.last_runs.len(), schedules.len());
}

#[test]
fn schedule_clock_runs_from_completion_time_not_original_deadline() {
    let base = Instant::now();
    let mut clock = ScheduleClock::new(1, base);
    let schedules = [candidate("building_crafting", ScheduleActivity::Always, 10)];
    let first_due_at = base + Duration::from_millis(25);

    let due = clock.select_due(
        schedules.len(),
        first_due_at,
        ScheduleWorkload {
            online_count: 0,
            player_entity_count: 0,
            crafting_due: false,
        },
        |idx| schedules.get(idx).copied(),
    );
    assert_eq!(due, vec![0]);
    *clock.last_run_mut(0, first_due_at) = first_due_at;

    let due = clock.select_due(
        schedules.len(),
        first_due_at + Duration::from_millis(9),
        ScheduleWorkload {
            online_count: 0,
            player_entity_count: 0,
            crafting_due: false,
        },
        |idx| schedules.get(idx).copied(),
    );
    assert!(due.is_empty());

    let due = clock.select_due(
        schedules.len(),
        first_due_at + Duration::from_millis(10),
        ScheduleWorkload {
            online_count: 0,
            player_entity_count: 0,
            crafting_due: false,
        },
        |idx| schedules.get(idx).copied(),
    );
    assert_eq!(due, vec![0]);
}

#[test]
fn side_profile_update_max_keeps_per_section_maximums() {
    let mut profile = SideProfile {
        broadcasts: std::time::Duration::from_millis(1),
        pack_resends: std::time::Duration::from_millis(5),
        box_pickups: std::time::Duration::from_millis(6),
        persistence_flush: std::time::Duration::from_millis(8),
        cell_conversions: std::time::Duration::from_millis(4),
        programmator_actions: std::time::Duration::from_millis(3),
        death: std::time::Duration::from_millis(7),
        bots_render: std::time::Duration::from_millis(6),
    };

    profile.update_max(SideProfile {
        broadcasts: std::time::Duration::from_millis(9),
        pack_resends: std::time::Duration::from_millis(1),
        box_pickups: std::time::Duration::from_millis(12),
        persistence_flush: std::time::Duration::from_millis(2),
        cell_conversions: std::time::Duration::from_millis(2),
        programmator_actions: std::time::Duration::from_millis(10),
        death: std::time::Duration::from_millis(1),
        bots_render: std::time::Duration::from_millis(11),
    });

    assert_eq!(profile.broadcasts, std::time::Duration::from_millis(9));
    assert_eq!(profile.pack_resends, std::time::Duration::from_millis(5));
    assert_eq!(profile.box_pickups, std::time::Duration::from_millis(12));
    assert_eq!(
        profile.persistence_flush,
        std::time::Duration::from_millis(8)
    );
    assert_eq!(
        profile.cell_conversions,
        std::time::Duration::from_millis(4)
    );
    assert_eq!(
        profile.programmator_actions,
        std::time::Duration::from_millis(10)
    );
    assert_eq!(profile.death, std::time::Duration::from_millis(7));
    assert_eq!(profile.bots_render, std::time::Duration::from_millis(11));
}

#[test]
fn tick_execution_class_separates_host_preemption_from_server_stalls() {
    let budget = Duration::from_millis(10);
    assert_eq!(
        classify_tick_execution(
            Duration::from_micros(174),
            Duration::from_micros(17_680),
            budget,
        ),
        TickExecutionClass::Preempted
    );
    assert_eq!(
        classify_tick_execution(
            Duration::from_micros(5_151),
            Duration::from_micros(13_594),
            budget,
        ),
        TickExecutionClass::Mixed
    );
    assert_eq!(
        classify_tick_execution(Duration::from_millis(11), Duration::from_millis(1), budget),
        TickExecutionClass::CpuBound
    );
}

#[tokio::test]
async fn disconnect_waits_for_persistence_capacity_before_mutation() {
    let (state, player, db_path, dir, world_name) =
        make_persistence_test_state("disconnect_admission").await;
    let (outbox, _rx) = crate::net::session::outbox::channel();
    crate::net::session::player::init::connect_in_tick(&state, &outbox, &player, 41);
    let pid = crate::game::PlayerId(player.id);

    let (persistence, mut persisted) = crate::persistence::PersistenceHandle::test_channel(1);
    persistence
        .try_reserve(crate::game::SaveKind::Player)
        .expect("filler capacity")
        .publish(crate::game::SaveCommand::Player {
            row: Box::new(test_player_row(99)),
        });

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let now = Instant::now();
    tx.send(crate::game::QueuedPlayerCommand {
        sequence: crate::game::CommandSeq::new(1),
        received_at: now,
        enqueued_at: now,
        command: crate::game::PlayerCommand::Disconnect {
            player_id: pid,
            session_id: crate::game::SessionId::new(41),
        },
    })
    .expect("queue disconnect");
    drop(tx);
    let mut pending = None;

    assert!(matches!(
        take_admitted_command(&mut rx, &mut pending, &persistence),
        Err("disconnect")
    ));
    assert!(pending.is_some());
    assert!(state.is_player_active(pid));

    let filler = persisted.try_recv().expect("filler command");
    assert!(matches!(
        filler,
        crate::game::SaveCommand::Player { row } if row.id == 99
    ));
    let Ok(Some(AdmittedCommand { queued, permit })) =
        take_admitted_command(&mut rx, &mut pending, &persistence)
    else {
        panic!("disconnect must be admitted after capacity is released");
    };
    let command_name = queued.command.name();
    let mut effects = crate::game::logic::commands::apply_player_command(&state, queued.command);
    publish_command_saves(permit, &mut effects.saves, command_name);

    assert!(!state.is_player_active(pid));
    assert!(pending.is_none());
    assert!(matches!(
        take_admitted_command(&mut rx, &mut pending, &persistence),
        Ok(None)
    ));
    assert!(matches!(
        persisted.try_recv(),
        Some(crate::game::SaveCommand::Player { row }) if row.id == player.id
    ));
    assert!(persisted.try_recv().is_none());

    cleanup_persistence_test(&db_path, &dir, &world_name);
}

#[test]
fn building_removal_waits_for_box_persistence_capacity() {
    let (persistence, mut persisted) = crate::persistence::PersistenceHandle::test_channel(1);
    persistence
        .try_reserve(crate::game::SaveKind::Box)
        .expect("filler capacity")
        .publish(crate::game::SaveCommand::Box {
            write: crate::db::BoxWrite {
                x: 1,
                y: 1,
                crystals: None,
            },
        });
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let now = Instant::now();
    tx.send(crate::game::QueuedPlayerCommand {
        sequence: crate::game::CommandSeq::new(1),
        received_at: now,
        enqueued_at: now,
        command: crate::game::PlayerCommand::ApplyRemovedBuilding {
            removal: crate::game::logic::contracts::BuildingRemoval {
                view: crate::game::PackView {
                    id: 1,
                    pack_type: crate::game::PackType::Teleport,
                    x: 10,
                    y: 10,
                    owner_id: crate::game::PlayerId(1),
                    clan_id: 0,
                    charge: 7,
                    max_charge: 100,
                    hp: 0,
                    max_hp: 100,
                },
                trigger_pid: None,
                storage_crystals: None,
            },
        },
    })
    .expect("queue building removal");
    drop(tx);
    let mut pending = None;

    assert!(matches!(
        take_admitted_command(&mut rx, &mut pending, &persistence),
        Err("apply_removed_building")
    ));
    assert!(pending.is_some());
    assert!(persisted.try_recv().is_some());
    assert!(matches!(
        take_admitted_command(&mut rx, &mut pending, &persistence),
        Ok(Some(AdmittedCommand {
            permit: Some(_),
            ..
        }))
    ));
    assert!(pending.is_none());
}

#[test]
fn hazard_box_pickup_backlog_coalesces_by_player() {
    let player_id = crate::game::PlayerId(7);
    let mut backlog = BoxPickupBacklog::default();
    backlog.extend(vec![
        crate::game::BoxPickupIntent {
            player_id,
            player_pos: (5, 5).into(),
            box_pos: (5, 5).into(),
            source: crate::game::BoxPickupSource::Standing,
        },
        crate::game::BoxPickupIntent {
            player_id,
            player_pos: (6, 5).into(),
            box_pos: (6, 5).into(),
            source: crate::game::BoxPickupSource::Standing,
        },
    ]);

    assert_eq!(backlog.queue.len(), 1);
    assert_eq!(backlog.players.len(), 1);
    assert_eq!(
        backlog.pop_front().expect("coalesced intent").box_pos,
        (5, 5).into()
    );
    assert!(backlog.queue.is_empty());
    assert!(backlog.players.is_empty());
}

#[tokio::test]
async fn hazard_box_pickup_waits_for_capacity_then_applies_once() {
    let (state, player, db_path, dir, world_name) =
        make_persistence_test_state("hazard_box_admission").await;
    let (outbox, _rx) = crate::net::session::outbox::channel();
    crate::net::session::player::init::connect_in_tick(&state, &outbox, &player, 43);
    let player_id = crate::game::PlayerId(player.id);
    state.modify_player(player_id, |ecs, entity| {
        ecs.get_mut::<crate::game::PlayerFlags>(entity)?.dirty = false;
        Some(())
    });
    state.put_box_cell_authoritative(5, 5, [3, 2, 1, 0, 0, 0]);

    let (persistence, mut persisted) = crate::persistence::PersistenceHandle::test_channel(1);
    persistence
        .try_reserve(crate::game::SaveKind::Box)
        .expect("filler capacity")
        .publish(crate::game::SaveCommand::Box {
            write: crate::db::BoxWrite {
                x: 1,
                y: 1,
                crystals: None,
            },
        });
    let intent = crate::game::BoxPickupIntent {
        player_id,
        player_pos: (5, 5).into(),
        box_pos: (5, 5).into(),
        source: crate::game::BoxPickupSource::Standing,
    };
    let mut backlog = BoxPickupBacklog::default();
    backlog.extend(vec![intent, intent]);
    let mut broadcasts = Vec::new();

    apply_pending_box_pickups(&state, &persistence, &mut backlog, &mut broadcasts);

    assert_eq!(backlog.queue.len(), 1);
    assert!(broadcasts.is_empty());
    assert_eq!(
        state.world.get_cell(5, 5),
        crate::world::cells::cell_type::BOX
    );
    let (crystals, dirty) = state
        .query_player_opt(player_id, |ecs, entity| {
            Some((
                ecs.get::<crate::game::PlayerStats>(entity)?.crystals,
                ecs.get::<crate::game::PlayerFlags>(entity)?.dirty,
            ))
        })
        .expect("connected player state");
    assert_eq!(crystals, [0; 6]);
    assert!(!dirty);

    assert!(persisted.try_recv().is_some());
    apply_pending_box_pickups(&state, &persistence, &mut backlog, &mut broadcasts);

    assert!(backlog.queue.is_empty());
    assert_eq!(
        state.world.get_cell(5, 5),
        crate::world::cells::cell_type::EMPTY
    );
    let crystals = state
        .query_player_opt(player_id, |ecs, entity| {
            Some(ecs.get::<crate::game::PlayerStats>(entity)?.crystals)
        })
        .expect("connected player crystals");
    assert_eq!(crystals, [3, 2, 1, 0, 0, 0]);
    assert_eq!(broadcasts.len(), 2);
    assert!(matches!(
        persisted.try_recv(),
        Some(crate::game::SaveCommand::Box { write })
            if write.x == 5 && write.y == 5 && write.crystals.is_none()
    ));
    assert!(persisted.try_recv().is_none());

    backlog.extend(vec![intent]);
    apply_pending_box_pickups(&state, &persistence, &mut backlog, &mut broadcasts);
    assert!(backlog.queue.is_empty());
    assert!(persisted.try_recv().is_none());
    let crystals_after_stale = state
        .query_player_opt(player_id, |ecs, entity| {
            Some(ecs.get::<crate::game::PlayerStats>(entity)?.crystals)
        })
        .expect("connected player crystals after stale intent");
    assert_eq!(crystals_after_stale, [3, 2, 1, 0, 0, 0]);

    cleanup_persistence_test(&db_path, &dir, &world_name);
}

#[tokio::test]
async fn dig_box_pickup_persists_and_returns_ordered_effects() {
    let (state, player, db_path, dir, world_name) =
        make_persistence_test_state("dig_box_admission").await;
    let (outbox, _rx) = crate::net::session::outbox::channel();
    crate::net::session::player::init::connect_in_tick(&state, &outbox, &player, 45);
    let player_id = crate::game::PlayerId(player.id);
    state.put_box_cell_authoritative(5, 6, [4, 0, 0, 0, 0, 0]);
    state.request_box_pickup(crate::game::BoxPickupIntent {
        player_id,
        player_pos: (5, 5).into(),
        box_pos: (5, 6).into(),
        source: crate::game::BoxPickupSource::Dig {
            session_id: Some(crate::game::SessionId::new(45)),
            direction: 0,
            skin: 0,
            clan_id: 0,
            tail: 0,
            exclude_self: true,
        },
    });
    let (persistence, mut persisted) = crate::persistence::PersistenceHandle::test_channel(1);
    let mut backlog = BoxPickupBacklog::default();
    backlog.extend(state.drain_box_pickups());
    let mut broadcasts = Vec::new();

    apply_pending_box_pickups(&state, &persistence, &mut backlog, &mut broadcasts);

    assert!(backlog.queue.is_empty());
    assert_eq!(broadcasts.len(), 4);
    assert!(matches!(
        broadcasts[0],
        crate::game::BroadcastEffect::Direct { .. }
    ));
    assert!(matches!(
        broadcasts[1],
        crate::game::BroadcastEffect::Direct { .. }
    ));
    assert!(matches!(
        broadcasts[2],
        crate::game::BroadcastEffect::Nearby { .. }
    ));
    assert!(matches!(
        broadcasts[3],
        crate::game::BroadcastEffect::CellUpdate(_)
    ));
    assert!(matches!(
        persisted.try_recv(),
        Some(crate::game::SaveCommand::Box { write })
            if write.x == 5 && write.y == 6 && write.crystals.is_none()
    ));
    assert_eq!(
        state.world.get_cell(5, 6),
        crate::world::cells::cell_type::EMPTY
    );

    cleanup_persistence_test(&db_path, &dir, &world_name);
}

#[tokio::test]
async fn death_box_drop_waits_for_capacity_then_persists_once() {
    let (state, player, db_path, dir, world_name) =
        make_persistence_test_state("death_box_admission").await;
    let (outbox, _rx) = crate::net::session::outbox::channel();
    crate::net::session::player::init::connect_in_tick(&state, &outbox, &player, 44);
    let player_id = crate::game::PlayerId(player.id);
    state.modify_player(player_id, |ecs, entity| {
        ecs.get_mut::<crate::game::PlayerStats>(entity)?.crystals = [3, 2, 1, 0, 0, 0];
        ecs.get_mut::<crate::game::PlayerFlags>(entity)?.dirty = false;
        Some(())
    });

    let (persistence, mut persisted) = crate::persistence::PersistenceHandle::test_channel(1);
    persistence
        .try_reserve(crate::game::SaveKind::Box)
        .expect("filler capacity")
        .publish(crate::game::SaveCommand::Box {
            write: crate::db::BoxWrite {
                x: 1,
                y: 1,
                crystals: None,
            },
        });
    state.request_player_death(player_id);
    state.request_player_death(player_id);
    let mut backlog = DeathBacklog::default();
    backlog.extend(state.drain_player_deaths());
    assert_eq!(backlog.queue.len(), 1);

    let effects = apply_pending_deaths(&state, &persistence, &mut backlog);

    assert!(effects.is_empty());
    assert_eq!(backlog.queue.len(), 1);
    let (position, crystals, dirty) = state
        .query_player_opt(player_id, |ecs, entity| {
            let position = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
            Some((
                (position.x, position.y),
                ecs.get::<crate::game::PlayerStats>(entity)?.crystals,
                ecs.get::<crate::game::PlayerFlags>(entity)?.dirty,
            ))
        })
        .expect("connected player state");
    assert_eq!(position, (5, 5));
    assert_eq!(crystals, [3, 2, 1, 0, 0, 0]);
    assert!(!dirty);

    assert!(persisted.try_recv().is_some());
    let effects = apply_pending_deaths(&state, &persistence, &mut backlog);

    assert_eq!(effects.len(), 1);
    assert!(backlog.queue.is_empty());
    let crystals = state
        .query_player_opt(player_id, |ecs, entity| {
            Some(ecs.get::<crate::game::PlayerStats>(entity)?.crystals)
        })
        .expect("connected player crystals");
    assert_eq!(crystals, [0; 6]);
    let save = persisted.try_recv().expect("death box save");
    let crate::game::SaveCommand::Box { write } = save else {
        panic!("death must publish a box save");
    };
    assert_eq!(write.crystals, Some([3, 2, 1, 0, 0, 0]));
    assert_eq!(
        state.world.get_cell(write.x, write.y),
        crate::world::cells::cell_type::BOX
    );
    assert!(persisted.try_recv().is_none());

    state.request_player_death(player_id);
    backlog.extend(state.drain_player_deaths());
    let second_effects = apply_pending_deaths(&state, &persistence, &mut backlog);
    assert_eq!(second_effects.len(), 1);
    assert!(persisted.try_recv().is_none());

    cleanup_persistence_test(&db_path, &dir, &world_name);
}

#[tokio::test]
async fn periodic_player_snapshot_preserves_dirty_on_saturation_and_new_mutation() {
    let (state, player, db_path, dir, world_name) =
        make_persistence_test_state("periodic_admission").await;
    let (outbox, _rx) = crate::net::session::outbox::channel();
    crate::net::session::player::init::connect_in_tick(&state, &outbox, &player, 42);
    let pid = crate::game::PlayerId(player.id);
    state.modify_player(pid, |ecs, entity| {
        ecs.get_mut::<crate::game::PlayerFlags>(entity)?.dirty = true;
        Some(())
    });

    let (persistence, mut persisted) = crate::persistence::PersistenceHandle::test_channel(1);
    persistence
        .try_reserve(crate::game::SaveKind::Player)
        .expect("filler capacity")
        .publish(crate::game::SaveCommand::Player {
            row: Box::new(test_player_row(99)),
        });

    let (_completion_tx, completion_rx) = tokio::sync::mpsc::channel(1);
    let mut pending = TickPendingWork::new(completion_rx);
    let first_due = pending.next_player_flush;
    pending.next_building_flush = first_due + Duration::from_hours(1);

    assert!(flush_due_dirty_snapshots(
        &state,
        &persistence,
        &mut pending,
        first_due
    ));
    assert!(player_is_dirty(&state, pid));
    assert!(persisted.try_recv().is_some());

    let second_due = pending.next_player_flush;
    assert_eq!(second_due, first_due + PLAYER_DIRTY_FLUSH_INTERVAL);
    assert!(!flush_due_dirty_snapshots(
        &state,
        &persistence,
        &mut pending,
        second_due.checked_sub(Duration::from_millis(1)).unwrap()
    ));
    assert!(player_is_dirty(&state, pid));
    assert!(flush_due_dirty_snapshots(
        &state,
        &persistence,
        &mut pending,
        second_due
    ));
    assert!(!player_is_dirty(&state, pid));
    let snapshot = persisted.try_recv().expect("periodic snapshot");
    assert!(matches!(
        snapshot,
        crate::game::SaveCommand::Player { row } if row.id == player.id
    ));

    state.modify_player(pid, |ecs, entity| {
        ecs.get_mut::<crate::game::PlayerStats>(entity)?.money = 123;
        ecs.get_mut::<crate::game::PlayerFlags>(entity)?.dirty = true;
        Some(())
    });
    assert!(player_is_dirty(&state, pid));
    let third_due = pending.next_player_flush;
    assert!(flush_due_dirty_snapshots(
        &state,
        &persistence,
        &mut pending,
        third_due
    ));
    assert!(!player_is_dirty(&state, pid));
    let latest = persisted.try_recv().expect("latest periodic snapshot");
    assert!(matches!(
        latest,
        crate::game::SaveCommand::Player { row }
            if row.id == player.id && row.money == 123
    ));
    assert!(persisted.try_recv().is_none());

    cleanup_persistence_test(&db_path, &dir, &world_name);
}

#[tokio::test]
async fn periodic_building_snapshot_preserves_dirty_on_saturation_and_persists_once() {
    let (state, _player, db_path, dir, world_name) =
        make_persistence_test_state("periodic_building_admission").await;
    let extra = crate::db::BuildingExtra {
        charge: 7,
        max_charge: 100,
        cost: 12,
        hp: 80,
        max_hp: 100,
        money_inside: 0,
        crystals_inside: [0; 6],
        items_inside: std::collections::HashMap::new(),
        craft_recipe_id: None,
        craft_num: 0,
        craft_end_ts: 0,
        craft_ready: false,
        clanzone: 0,
    };
    let entity = {
        let mut ecs = state.ecs_write_profiled("test.periodic_building_spawn");
        crate::game::buildings::spawn_building_from_extra(
            &mut ecs,
            &crate::game::BuildingSpawnSpec {
                id: 7,
                pack_type: crate::game::PackType::Gun,
                x: 10,
                y: 10,
                owner_id: crate::game::PlayerId(1),
                clan_id: 0,
                extra: &extra,
            },
        )
    };
    state.modify_building(entity, |ecs, entity| {
        ecs.get_mut::<crate::game::BuildingFlags>(entity)?.dirty = true;
        Some(())
    });

    let (persistence, mut persisted) = crate::persistence::PersistenceHandle::test_channel(1);
    persistence
        .try_reserve(crate::game::SaveKind::Box)
        .expect("filler capacity")
        .publish(crate::game::SaveCommand::Box {
            write: crate::db::BoxWrite {
                x: 1,
                y: 1,
                crystals: None,
            },
        });

    assert_eq!(flush_dirty_buildings_once(&state, &persistence), 0);
    assert!(building_is_dirty(&state, entity));
    assert!(persisted.try_recv().is_some());

    state.modify_building(entity, |ecs, entity| {
        ecs.get_mut::<crate::game::buildings::BuildingStorage>(entity)?
            .money = 123;
        Some(())
    });
    assert_eq!(flush_dirty_buildings_once(&state, &persistence), 1);
    assert!(!building_is_dirty(&state, entity));
    let snapshot = persisted.try_recv().expect("periodic building snapshot");
    assert!(matches!(
        snapshot,
        crate::game::SaveCommand::Building { row }
            if row.id == 7 && row.money_inside == 123
    ));
    assert_eq!(flush_dirty_buildings_once(&state, &persistence), 0);
    assert!(persisted.try_recv().is_none());

    cleanup_persistence_test(&db_path, &dir, &world_name);
}

#[tokio::test]
async fn online_count_broadcast_sends_on_to_active_players() {
    let dir = std::env::temp_dir();
    let nonce = format!("{}_{}", std::process::id(), unique_test_nonce());
    let db_path = dir.join(format!("online_count_{nonce}.db"));
    let _ = std::fs::remove_file(&db_path);

    let database = crate::db::Database::open(&db_path).await.unwrap();
    let mut p1 = database.create_player("online-a", "p", "h1").await.unwrap();
    let mut p2 = database.create_player("online-b", "p", "h2").await.unwrap();
    p1.x = 5;
    p1.y = 5;
    p2.x = 6;
    p2.y = 5;

    let cell_defs =
        crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json")).unwrap();
    let world_name = format!("online_count_world_{nonce}");
    let world = crate::world::World::new(&world_name, 2, 2, cell_defs, &dir).unwrap();
    let config = crate::config::Config {
        world_name: world_name.clone(),
        port: 8090,
        world_chunks_w: 2,
        world_chunks_h: 2,
        data_dir: dir.to_string_lossy().to_string(),
        logging: crate::config::LoggingConfig::runtime_baseline(),
        cron: crate::config::CronConfig::runtime_baseline(),
        gameplay: crate::config::GameplayConfig::runtime_baseline(),
    };
    let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config)
        .await
        .unwrap();

    let (tx1, mut rx1) = crate::net::session::outbox::channel();
    let (tx2, mut rx2) = crate::net::session::outbox::channel();
    crate::net::session::player::init::connect_in_tick(&state, &tx1, &p1, 1);
    crate::net::session::player::init::connect_in_tick(&state, &tx2, &p2, 2);
    drain_queued_packets(&mut rx1);
    drain_queued_packets(&mut rx2);

    crate::tasks::lifecycle::broadcast_online_count(&state);

    assert_online_packet(&mut rx1, b"2:0");
    assert_online_packet(&mut rx2, b"2:0");
    assert!(rx1.try_recv().is_err());
    assert!(rx2.try_recv().is_err());

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(dir.join(format!("{world_name}_v2.map")));
    let _ = std::fs::remove_file(dir.join(format!("{world_name}_durability.map")));
}

fn drain_queued_packets(rx: &mut mpsc::Receiver<Vec<u8>>) {
    while rx.try_recv().is_ok() {}
}

fn assert_online_packet(rx: &mut mpsc::Receiver<Vec<u8>>, expected_payload: &[u8]) {
    let frame = rx.try_recv().expect("ON frame");
    let mut buf = BytesMut::from(&frame[..]);
    let packet = crate::protocol::Packet::try_decode(&mut buf)
        .expect("valid packet")
        .expect("decoded packet");
    assert_eq!(packet.event_str(), "ON");
    assert_eq!(packet.payload.as_ref(), expected_payload);
}

fn unique_test_nonce() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

async fn make_persistence_test_state(
    label: &str,
) -> (
    Arc<GameState>,
    crate::db::PlayerRow,
    std::path::PathBuf,
    std::path::PathBuf,
    String,
) {
    let dir = std::env::temp_dir();
    let nonce = unique_test_nonce();
    let db_path = dir.join(format!("{label}_{nonce}.db"));
    let database = crate::db::Database::open(&db_path).await.unwrap();
    let mut player = database
        .create_player("persistence-player", "password", "hash")
        .await
        .unwrap();
    player.x = 5;
    player.y = 5;
    let cell_defs =
        crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json")).unwrap();
    let world_name = format!("{label}_{nonce}_world");
    let world = crate::world::World::new(&world_name, 2, 2, cell_defs, &dir).unwrap();
    let config = crate::config::Config {
        world_name: world_name.clone(),
        port: 8090,
        world_chunks_w: 2,
        world_chunks_h: 2,
        data_dir: dir.to_string_lossy().to_string(),
        logging: crate::config::LoggingConfig::runtime_baseline(),
        cron: crate::config::CronConfig::runtime_baseline(),
        gameplay: crate::config::GameplayConfig::runtime_baseline(),
    };
    let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config)
        .await
        .unwrap();
    (state, player, db_path, dir, world_name)
}

fn test_player_row(id: i32) -> crate::db::PlayerRow {
    crate::db::PlayerRow {
        id,
        name: format!("player-{id}"),
        passwd: String::new(),
        hash: String::new(),
        x: 5,
        y: 5,
        dir: 0,
        health: 100,
        max_health: 100,
        money: 0,
        creds: 0,
        skin: 0,
        auto_dig: false,
        aggression: false,
        crystals: [0; 6],
        clan_id: None,
        resp_x: None,
        resp_y: None,
        inventory: std::collections::HashMap::new(),
        skills: crate::db::SkillSlots {
            skills: std::collections::HashMap::new(),
            total_slots: 20,
        },
        role: 0,
        selected_program_id: None,
        selected_program: None,
        programmator_running: false,
        programmator_snapshot: None,
        clan_rank: 0,
        last_bonus_at: 0,
    }
}

fn player_is_dirty(state: &Arc<GameState>, pid: crate::game::PlayerId) -> bool {
    state
        .query_player_opt(pid, |ecs, entity| {
            Some(
                ecs.get::<crate::game::PlayerFlags>(entity)
                    .is_some_and(|flags| flags.dirty),
            )
        })
        .unwrap_or(false)
}

fn building_is_dirty(state: &Arc<GameState>, entity: bevy_ecs::prelude::Entity) -> bool {
    state
        .ecs
        .read()
        .get::<crate::game::BuildingFlags>(entity)
        .is_some_and(|flags| flags.dirty)
}

fn cleanup_persistence_test(db_path: &std::path::Path, dir: &std::path::Path, world_name: &str) {
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(dir.join(format!("{world_name}_v2.map")));
    let _ = std::fs::remove_file(dir.join(format!("{world_name}_road_v2.map")));
    let _ = std::fs::remove_file(dir.join(format!("{world_name}_durability.map")));
    let _ = std::fs::remove_file(dir.join(format!("{world_name}_world.journal")));
}
