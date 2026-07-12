//! Authoritative simulation thread and its tick-local state.

mod commands;
mod due;
mod effects;
mod profiler;
mod scheduler;
mod snapshots;
mod tick;

use crate::game::GameState;
use crossbeam_utils::CachePadded;
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

struct TickServices {
    heartbeat: Arc<TickHeartbeat>,
    tick_log_tx: std::sync::mpsc::SyncSender<profiler::TickLogEvent>,
    presentation: crate::net::presentation::PresentationRuntime,
    persistence: crate::persistence::PersistenceHandle,
}

struct TickProfileState {
    window: profiler::TickWindowProfile,
    last_warn: Instant,
}

impl TickProfileState {
    fn new(now: Instant) -> Self {
        Self {
            window: profiler::TickWindowProfile::new(now),
            last_warn: now.checked_sub(Duration::from_secs(1)).unwrap_or(now),
        }
    }
}

#[derive(Default)]
struct BoxPickupBacklog {
    queue: VecDeque<crate::game::BoxPickupIntent>,
    players: HashSet<crate::game::PlayerId>,
}

struct TickPendingWork {
    command: Option<crate::game::QueuedPlayerCommand>,
    box_pickups: BoxPickupBacklog,
    deaths: DeathBacklog,
    persistence_completions: tokio::sync::mpsc::Receiver<crate::game::PersistenceCompletion>,
    next_player_flush: Instant,
    next_building_flush: Instant,
}

impl TickPendingWork {
    fn new(
        persistence_completions: tokio::sync::mpsc::Receiver<crate::game::PersistenceCompletion>,
    ) -> Self {
        let now = Instant::now();
        Self {
            command: None,
            box_pickups: BoxPickupBacklog::default(),
            deaths: DeathBacklog::default(),
            persistence_completions,
            next_player_flush: now,
            next_building_flush: now,
        }
    }
}

#[derive(Default)]
struct DeathBacklog {
    queue: VecDeque<crate::game::PlayerId>,
    players: HashSet<crate::game::PlayerId>,
}

type PendingDeathEffect = (
    crate::game::PlayerId,
    i32,
    i32,
    i32,
    crate::net::session::play::death::DeathBroadcasts,
);

impl DeathBacklog {
    fn extend(&mut self, player_ids: Vec<crate::game::PlayerId>) {
        for player_id in player_ids {
            if self.players.insert(player_id) {
                self.queue.push_back(player_id);
            }
        }
    }

    fn pop_front(&mut self) -> Option<crate::game::PlayerId> {
        let player_id = self.queue.pop_front()?;
        self.players.remove(&player_id);
        Some(player_id)
    }
}

impl BoxPickupBacklog {
    fn extend(&mut self, intents: Vec<crate::game::BoxPickupIntent>) {
        for intent in intents {
            if self.players.insert(intent.player_id) {
                self.queue.push_back(intent);
            }
        }
    }

    fn pop_front(&mut self) -> Option<crate::game::BoxPickupIntent> {
        let intent = self.queue.pop_front()?;
        self.players.remove(&intent.player_id);
        Some(intent)
    }
}

fn apply_pending_box_pickups(
    state: &Arc<GameState>,
    persistence: &crate::persistence::PersistenceHandle,
    backlog: &mut BoxPickupBacklog,
    broadcasts: &mut Vec<crate::game::BroadcastEffect>,
) {
    while let Some(intent) = backlog.queue.front().copied() {
        let permit = match persistence.try_reserve(crate::game::SaveKind::Box) {
            Ok(permit) => permit,
            Err(crate::persistence::PersistenceAdmissionError::Full) => break,
            Err(crate::persistence::PersistenceAdmissionError::Closed) => {
                panic!("persistence worker closed before hazard box pickup admission")
            }
        };
        let popped = backlog
            .pop_front()
            .expect("hazard box pickup backlog front disappeared");
        debug_assert_eq!(popped, intent);
        match crate::game::logic::boxes::apply_box_pickup(state, intent) {
            crate::game::logic::boxes::BoxPickupApplyResult::Picked {
                save,
                broadcasts: mut pickup_broadcasts,
            } => {
                permit.publish(save);
                broadcasts.append(&mut pickup_broadcasts);
            }
            crate::game::logic::boxes::BoxPickupApplyResult::Stale => {}
        }
    }
}

fn apply_pending_deaths(
    state: &Arc<GameState>,
    persistence: &crate::persistence::PersistenceHandle,
    backlog: &mut DeathBacklog,
) -> Vec<PendingDeathEffect> {
    let mut admitted = Vec::new();
    while let Some(player_id) = backlog.queue.front().copied() {
        let permit = match persistence.try_reserve(crate::game::SaveKind::Box) {
            Ok(permit) => permit,
            Err(crate::persistence::PersistenceAdmissionError::Full) => break,
            Err(crate::persistence::PersistenceAdmissionError::Closed) => {
                panic!("persistence worker closed before player death admission")
            }
        };
        let popped = backlog
            .pop_front()
            .expect("death backlog front disappeared");
        debug_assert_eq!(popped, player_id);
        admitted.push((player_id, permit));
    }
    if admitted.is_empty() {
        return Vec::new();
    }

    let building_entities = state.building_entities_snapshot();
    let mut effects = Vec::with_capacity(admitted.len());
    let mut errors = Vec::new();
    {
        let mut ecs = state.ecs_write_profiled("death.apply_admitted_batch");
        for (player_id, permit) in admitted {
            match crate::net::session::play::death::apply_player_death_core(
                state,
                &mut ecs,
                &building_entities,
                player_id,
            ) {
                Ok(output) => {
                    if let Some(save) = output.save {
                        permit.publish(save);
                    }
                    effects.push((
                        player_id,
                        output.resp_x,
                        output.resp_y,
                        output.max_health,
                        output.broadcasts,
                    ));
                }
                Err(error) => errors.push((player_id, error)),
            }
        }
    }
    for (player_id, error) in errors {
        tracing::error!(player_id = %player_id, ?error, "Queued player death aborted");
        if let Some(tx) = state.player_sender(player_id) {
            crate::net::session::play::death::send_death_state_error(&tx);
        }
    }
    effects
}

pub fn spawn_game_tick_loop(
    state: Arc<GameState>,
    shutdown: &broadcast::Sender<()>,
    persistence: crate::persistence::PersistenceHandle,
    persistence_completions: tokio::sync::mpsc::Receiver<crate::game::PersistenceCompletion>,
) -> std::thread::JoinHandle<()> {
    let commands = state
        .commands_rx
        .lock()
        .take()
        .expect("commands_rx already taken");
    let tick_rate_ms = state.config.gameplay.schedules.game_loop_tick_rate_ms;
    let heartbeat = Arc::new(TickHeartbeat::new(Instant::now()));
    spawn_game_tick_watchdog(
        state.clone(),
        heartbeat.clone(),
        shutdown.subscribe(),
        tick_rate_ms,
    );
    spawn_parking_lot_deadlock_detector(shutdown.subscribe());

    let (tick_log_tx, tick_log_rx) = std::sync::mpsc::sync_channel(1024);
    profiler::spawn_tick_log_worker(tick_log_rx);
    let presentation = crate::net::presentation::PresentationRuntime::start(state.clone());
    let services = TickServices {
        heartbeat,
        tick_log_tx,
        presentation,
        persistence,
    };
    let runtime = SimulationRuntime::new(
        state,
        commands,
        shutdown.subscribe(),
        services,
        persistence_completions,
    );

    std::thread::Builder::new()
        .name("openmines-game-tick".to_owned())
        .spawn(move || runtime.run())
        .expect("spawn game tick thread")
}

struct SimulationRuntime {
    state: Arc<GameState>,
    commands: tokio::sync::mpsc::UnboundedReceiver<crate::game::QueuedPlayerCommand>,
    shutdown: broadcast::Receiver<()>,
    services: TickServices,
    tick_profile: TickProfileState,
    schedule_clock: scheduler::ScheduleClock,
    due_actions: crate::game::logic::due::DueActionQueue,
    sim_tick: crate::game::SimTick,
    pending_work: TickPendingWork,
    tick_duration: Duration,
    previous_tick_started_at: Instant,
    next_tick_at: Instant,
}

impl SimulationRuntime {
    fn new(
        state: Arc<GameState>,
        commands: tokio::sync::mpsc::UnboundedReceiver<crate::game::QueuedPlayerCommand>,
        shutdown: broadcast::Receiver<()>,
        services: TickServices,
        persistence_completions: tokio::sync::mpsc::Receiver<crate::game::PersistenceCompletion>,
    ) -> Self {
        let now = Instant::now();
        let tick_duration =
            Duration::from_millis(state.config.gameplay.schedules.game_loop_tick_rate_ms);
        let due_action_capacity = state.config.gameplay.simulation.due_action_capacity;
        Self {
            schedule_clock: scheduler::ScheduleClock::new(state.schedules.len(), now),
            state,
            commands,
            shutdown,
            services,
            tick_profile: TickProfileState::new(now),
            due_actions: crate::game::logic::due::DueActionQueue::new(due_action_capacity),
            sim_tick: crate::game::SimTick::default(),
            pending_work: TickPendingWork::new(persistence_completions),
            tick_duration,
            previous_tick_started_at: now,
            next_tick_at: now,
        }
    }

    fn run(mut self) {
        tracing::info!(
            tick_rate_ms = self.tick_duration.as_millis(),
            "ECS Game Thread started"
        );
        while self.run_once() {}
        self.finish_shutdown();
        tracing::info!("ECS Game Thread shutting down");
    }

    fn finish_shutdown(self) {
        let Self {
            state,
            mut commands,
            services,
            mut pending_work,
            ..
        } = self;
        commands.close();
        let TickServices {
            presentation,
            persistence,
            ..
        } = services;
        drop(persistence);

        let completions = drain_persistence_completions(
            &state,
            &presentation,
            &mut pending_work.persistence_completions,
        );
        tracing::info!(completions, "Simulation persistence completions drained");
    }

    fn run_once(&mut self) -> bool {
        let started_at = Instant::now();
        crate::metrics::TICK_START_INTERVAL_SECONDS.observe(
            started_at
                .duration_since(self.previous_tick_started_at)
                .as_secs_f64(),
        );
        crate::metrics::TICK_WAKE_LATENESS_SECONDS.observe(
            started_at
                .saturating_duration_since(self.next_tick_at)
                .as_secs_f64(),
        );
        self.previous_tick_started_at = started_at;
        self.next_tick_at = started_at + self.tick_duration;
        self.sim_tick = self.sim_tick.next();
        crate::metrics::SIMULATION_TICK.set(i64::try_from(self.sim_tick.get()).unwrap_or(i64::MAX));
        match self.shutdown.try_recv() {
            Ok(())
            | Err(
                tokio::sync::broadcast::error::TryRecvError::Closed
                | tokio::sync::broadcast::error::TryRecvError::Lagged(_),
            ) => return false,
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {}
        }

        let state = self.state.clone();
        let run_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tick::run_game_tick_sync(
                &state,
                &mut self.commands,
                &mut self.due_actions,
                &mut self.tick_profile,
                &mut self.schedule_clock,
                &mut self.pending_work,
                &self.services,
            )
        }));
        let inner_tick_elapsed = match run_result {
            Ok(elapsed) => elapsed,
            Err(panic) => {
                tracing::error!(
                    target: "tickprof",
                    ?panic,
                    "GAME TICK PANICKED - aborting process because ECS state may be corrupt"
                );
                std::process::exit(101);
            }
        };
        let elapsed = started_at.elapsed();
        let outer_overhead = elapsed.saturating_sub(inner_tick_elapsed);
        if outer_overhead > Duration::from_millis(50) {
            tracing::warn!(
                target: "tickprof",
                total_wall = ?elapsed,
                inner_tick = ?inner_tick_elapsed,
                ?outer_overhead,
                "SLOW game tick outer overhead"
            );
        }
        if let Some(remaining) = self.next_tick_at.checked_duration_since(Instant::now()) {
            std::thread::sleep(remaining);
        }
        true
    }
}

fn drain_persistence_completions(
    state: &Arc<GameState>,
    presentation: &crate::net::presentation::PresentationRuntime,
    completions: &mut tokio::sync::mpsc::Receiver<crate::game::PersistenceCompletion>,
) -> u64 {
    let mut applied = 0_u64;
    while let Some(completion) = completions.blocking_recv() {
        let effects = crate::game::logic::commands::apply_persistence_completion(state, completion);
        effects::apply_shutdown_command_effects(state, presentation, effects);
        applied = applied.saturating_add(1);
    }
    applied
}

const TICK_WATCHDOG_WARN_MULTIPLIER: u64 = 200;
const TICK_WATCHDOG_MIN_WARN_MS: u64 = 2_000;

struct TickHeartbeat {
    started_at: Instant,
    last_mark_ms: CachePadded<AtomicU64>,
    tick_seq: CachePadded<AtomicU64>,
    stage: CachePadded<AtomicU8>,
    schedule_index: CachePadded<AtomicU64>,
}

impl TickHeartbeat {
    const fn new(started_at: Instant) -> Self {
        Self {
            started_at,
            last_mark_ms: CachePadded::new(AtomicU64::new(0)),
            tick_seq: CachePadded::new(AtomicU64::new(0)),
            stage: CachePadded::new(AtomicU8::new(TickStage::Idle as u8)),
            schedule_index: CachePadded::new(AtomicU64::new(u64::MAX)),
        }
    }

    fn begin_tick(&self) {
        self.tick_seq.fetch_add(1, Ordering::Relaxed);
        self.mark(TickStage::TickStart);
    }

    fn mark(&self, stage: TickStage) {
        self.mark_schedule(stage, u64::MAX);
    }

    fn mark_schedule(&self, stage: TickStage, schedule_index: u64) {
        let elapsed_ms = self.started_at.elapsed().as_millis();
        let elapsed_ms = u64::try_from(elapsed_ms).unwrap_or(u64::MAX);
        self.stage.store(stage as u8, Ordering::Relaxed);
        self.schedule_index.store(schedule_index, Ordering::Relaxed);
        self.last_mark_ms.store(elapsed_ms, Ordering::Relaxed);
    }
}

#[derive(Clone, Copy)]
enum TickStage {
    Idle = 0,
    TickStart = 1,
    Dispatch = 2,
    EcsLockWait = 3,
    ScheduleRun = 4,
    FlushQueues = 5,
    SideBroadcasts = 6,
    SidePackResends = 7,
    SideCellConversions = 8,
    SideCellConversionsEcsLockWait = 9,
    SideProgrammatorActions = 10,
    SideDeath = 11,
    SideBotsRender = 12,
    Summary = 13,
}

const fn tick_stage_name(stage: u8) -> &'static str {
    match stage {
        0 => "idle",
        1 => "tick_start",
        2 => "dispatch",
        3 => "ecs_lock_wait",
        4 => "schedule_run",
        5 => "flush_queues",
        6 => "side_broadcasts",
        7 => "side_pack_resends",
        8 => "side_cell_conversions",
        9 => "side_cell_conversions_ecs_lock_wait",
        10 => "side_programmator_actions",
        11 => "side_death",
        12 => "side_bots_render",
        13 => "summary",
        _ => "unknown",
    }
}

fn spawn_game_tick_watchdog(
    state: Arc<GameState>,
    heartbeat: Arc<TickHeartbeat>,
    mut shutdown_rx: broadcast::Receiver<()>,
    tick_rate_ms: u64,
) {
    let warn_after_ms =
        TICK_WATCHDOG_MIN_WARN_MS.max(tick_rate_ms.saturating_mul(TICK_WATCHDOG_WARN_MULTIPLIER));
    std::thread::Builder::new()
        .name("openmines-tick-watchdog".to_owned())
        .spawn(move || {
            let sleep_ms = (warn_after_ms / 2).max(250);
            let sleep_duration = std::time::Duration::from_millis(sleep_ms);
            loop {
                std::thread::sleep(sleep_duration);
                if shutdown_rx.try_recv().is_ok() {
                    break;
                }

                let now_ms =
                    u64::try_from(heartbeat.started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
                let last_ms = heartbeat.last_mark_ms.load(Ordering::Relaxed);
                let age_ms = now_ms.saturating_sub(last_ms);
                if age_ms <= warn_after_ms {
                    continue;
                }

                let stage_id = heartbeat.stage.load(Ordering::Relaxed);
                let schedule_index = heartbeat.schedule_index.load(Ordering::Relaxed);
                let schedule = usize::try_from(schedule_index)
                    .ok()
                    .and_then(|i| state.schedules.get(i))
                    .map_or("-", |s| s.name.as_str());
                tracing::error!(
                    target: "tickprof",
                    age_ms,
                    warn_after_ms,
                    tick_seq = heartbeat.tick_seq.load(Ordering::Relaxed),
                    stage = tick_stage_name(stage_id),
                    schedule,
                    active_players = state.active_player_ids().len(),
                    pending_db_tasks = state.db_pending_tasks.load(Ordering::SeqCst),
                    "GAME TICK WATCHDOG: no progress heartbeat"
                );
            }
        })
        .expect("spawn game tick watchdog thread");
}

fn spawn_parking_lot_deadlock_detector(mut shutdown_rx: broadcast::Receiver<()>) {
    std::thread::Builder::new()
        .name("openmines-deadlock-detector".to_owned())
        .spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(10));
                if shutdown_rx.try_recv().is_ok() {
                    break;
                }

                let deadlocks = parking_lot::deadlock::check_deadlock();
                if deadlocks.is_empty() {
                    continue;
                }

                tracing::error!(
                    target: "tickprof",
                    deadlock_count = deadlocks.len(),
                    "PARKING_LOT DEADLOCK DETECTED"
                );
                for (deadlock_idx, threads) in deadlocks.iter().enumerate() {
                    tracing::error!(
                        target: "tickprof",
                        deadlock_index = deadlock_idx,
                        thread_count = threads.len(),
                        "parking_lot deadlock group"
                    );
                    for thread in threads {
                        tracing::error!(
                            target: "tickprof",
                            deadlock_index = deadlock_idx,
                            thread_id = ?thread.thread_id(),
                            backtrace = ?thread.backtrace(),
                            "parking_lot deadlocked thread"
                        );
                    }
                }
            }
        })
        .expect("spawn parking_lot deadlock detector thread");
}

#[cfg(test)]
mod shutdown_tests {
    use super::drain_persistence_completions;
    use crate::world::WorldProvider as _;

    async fn prepare_delete_completion() -> (
        crate::test_support::ServerTestHarness,
        crate::game::PersistenceCompletion,
    ) {
        let test = crate::test_support::ServerTestHarness::new(
            "building_delete_shutdown",
            "delete-shutdown",
        )
        .await;
        let extra = crate::db::BuildingExtra {
            charge: 100,
            max_charge: 100,
            cost: 10,
            hp: 100,
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
        let (building_id, _) = test
            .state
            .insert_building_runtime(&crate::game::BuildingInsertSpec {
                type_code: "R",
                pack_type: crate::game::PackType::Resp,
                x: 10,
                y: 10,
                owner_id: crate::game::PlayerId(test.player.id),
                clan_id: 0,
                extra: &extra,
            })
            .await
            .unwrap();
        test.state
            .db
            .update_player_resp(test.player.id, Some(10), Some(10))
            .await
            .unwrap();
        let player_id = crate::game::PlayerId(test.player.id);
        test.state.modify_player(player_id, |ecs, entity| {
            let mut metadata = ecs.get_mut::<crate::game::PlayerMetadata>(entity)?;
            metadata.resp_x = Some(10);
            metadata.resp_y = Some(10);
            Some(())
        });
        let request = crate::game::logic::building_delete::admit(
            &test.state,
            crate::game::RemovePack {
                x: 10,
                y: 10,
                cause: crate::game::BuildingDeleteCause::Damage {
                    trigger_player_id: None,
                    origin_session_id: None,
                },
            },
            crate::game::BuildingDeleteOperationId::new(31),
        )
        .unwrap();
        let outcome = test
            .state
            .db
            .apply_building_delete(&crate::db::BuildingDeleteWrite {
                building_id,
                x: 10,
                y: 10,
                clear_resp_bindings: true,
                box_write: None,
            })
            .await
            .unwrap();
        let crate::db::BuildingDeleteOutcome::Deleted {
            cleared_resp_bindings,
        } = outcome
        else {
            panic!("fixture delete must match persisted identity");
        };
        let completion = crate::game::PersistenceCompletion::BuildingDeleted {
            request,
            result: crate::game::BuildingDeleteResult::Deleted {
                cleared_resp_bindings,
            },
        };
        (test, completion)
    }

    #[tokio::test]
    async fn delete_completion_is_applied_before_final_shutdown_flush() {
        let (test, completion) = prepare_delete_completion().await;
        let (completion_tx, mut completion_rx) = tokio::sync::mpsc::channel(1);
        completion_tx.send(completion).await.unwrap();
        let state_for_drain = test.state.clone();
        let presentation = crate::net::presentation::PresentationRuntime::start(test.state.clone());
        let drain = tokio::task::spawn_blocking(move || {
            drain_persistence_completions(&state_for_drain, &presentation, &mut completion_rx)
        });

        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while test.state.building_entity_at(10, 10).is_some() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("shutdown completion was not applied");
        assert!(
            !drain.is_finished(),
            "drain must wait until the persistence completion channel closes"
        );
        drop(completion_tx);
        assert_eq!(drain.await.unwrap(), 1);

        crate::shutdown::shutdown_flush(&test.state).await;
        let player = test
            .state
            .db
            .get_player_by_id(test.player.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!((player.resp_x, player.resp_y), (None, None));
        assert!(test.state.db.load_all_buildings().await.unwrap().is_empty());
        assert_eq!(
            test.state.world.get_cell_typed(10, 10),
            crate::world::CellType(crate::world::cells::cell_type::EMPTY)
        );
    }
}
