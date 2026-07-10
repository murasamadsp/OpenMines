//! Фоновые задачи: сброс мира, периодическое сохранение игроков, сохранение при остановке.
//! Отделено от `run()` в `mod.rs`, чтобы тот отвечал только за accept TCP (SRP).

use crate::game::{GameState, ScheduleActivity};
use crate::world::WorldProvider;
use bevy_ecs::prelude::Entity;
use crossbeam_utils::CachePadded;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

struct PendingSaveGuard {
    state: Arc<GameState>,
}

impl Drop for PendingSaveGuard {
    fn drop(&mut self) {
        self.state
            .db_pending_tasks
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Периодический flush mmap-слоёв мира.
pub fn spawn_world_flush_loop(state: Arc<GameState>, mut shutdown: broadcast::Receiver<()>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_mins(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = shutdown.recv() => break,
            }
            state.prune_auth_failures_by_addr(Instant::now());
            // C# World.Update: ежечасный пересчёт цен кристаллов (self-throttled на 1ч).
            crate::game::market::tick_crystal_prices(&state);
            let t0 = std::time::Instant::now();
            tracing::debug!(target: "tickprof", "WORLD FLUSH start");
            let state_c = state.clone();
            match tokio::task::spawn_blocking(move || state_c.world.flush()).await {
                Ok(Ok(flush_stats)) => {
                    crate::metrics::WORLD_FLUSH_DURABILITY_CHUNKS_TOTAL.inc_by(
                        u64::try_from(flush_stats.durability.dirty_chunks).unwrap_or(u64::MAX),
                    );
                    crate::metrics::WORLD_FLUSH_DURABILITY_RANGES_TOTAL
                        .inc_by(u64::try_from(flush_stats.durability.ranges).unwrap_or(u64::MAX));
                    crate::metrics::WORLD_FLUSH_DURABILITY_BYTES_TOTAL
                        .inc_by(u64::try_from(flush_stats.durability.bytes).unwrap_or(u64::MAX));
                }
                Ok(Err(e)) => tracing::error!(error = ?e, "World flush error"),
                Err(e) => tracing::error!(error = ?e, "World flush task failed"),
            }
            tracing::debug!(target: "tickprof", elapsed = ?t0.elapsed(), "WORLD FLUSH end");
            crate::metrics::WORLD_FLUSH_TOTAL.inc();
            crate::metrics::WORLD_FLUSH_SECONDS.observe(t0.elapsed().as_secs_f64());
        }
    });
}

/// C# `World.Update`: раз в минуту шлёт всем активным игрокам `ON online:0`.
pub fn spawn_online_count_loop(state: Arc<GameState>, mut shutdown: broadcast::Receiver<()>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_mins(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = shutdown.recv() => break,
            }
            broadcast_online_count(&state);
        }
    });
}

fn broadcast_online_count(state: &GameState) {
    let pids: Vec<crate::game::PlayerId> = state.active_player_ids();
    let online_count = i32::try_from(pids.len()).unwrap_or(i32::MAX);
    let packet = crate::protocol::packets::online(online_count, 0);
    let wire = crate::net::session::wire::make_u_packet_bytes(packet.0, &packet.1);
    for pid in pids {
        state.send_to_player(pid, wire.clone());
    }
}

/// Сохранение «грязных» игроков в БД.
pub fn spawn_player_dirty_flush_loop(state: Arc<GameState>, mut shutdown: broadcast::Receiver<()>) {
    tokio::spawn(async move {
        // 1:1 ref: `Player.Sync()` runs about every 10 seconds.
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = shutdown.recv() => break,
            }

            // Сначала снимаем список pid без вложенного `modify_player` под guard'ом итератора:
            // иначе держим ref `active_players` + `ecs.write()` — легко словить взаимную блокировку
            // с сессией (`query_player` / `broadcast_to_nearby`) и «зависание» всего сервера ~10 с.
            let pids: Vec<crate::game::PlayerId> = state.player_entity_ids();

            // Extract dirty rows WITHOUT clearing dirty yet — clearing happens only after
            // a successful save so that a concurrent disconnect save or a save failure
            // cannot silently lose the dirty flag (BUG 1 / BUG 3 fix).
            let mut dirty_rows = Vec::new();
            for pid in pids {
                let row = state
                    .modify_player(pid, |ecs, entity| {
                        let flags = ecs.get::<crate::game::PlayerFlags>(entity)?;
                        if flags.dirty {
                            crate::game::player::extract_player_row(ecs, entity)
                        } else {
                            None
                        }
                    })
                    .flatten();
                if let Some(r) = row {
                    dirty_rows.push((pid, r));
                }
            }

            if !dirty_rows.is_empty() {
                tracing::debug!(dirty_count = dirty_rows.len(), "Periodic save started");
            }

            let mut saved = 0;
            for (pid, player_data) in dirty_rows {
                let db = state.db.clone();
                let state_c = state.clone();
                let pid_c = pid;
                tokio::spawn(async move {
                    let res = db.save_player(&player_data).await;
                    match res {
                        Ok(()) => {
                            state_c.modify_player(pid_c, |ecs, entity| {
                                if let Some(mut flags) =
                                    ecs.get_mut::<crate::game::PlayerFlags>(entity)
                                {
                                    flags.dirty = false;
                                }
                            });
                            crate::metrics::PLAYER_SAVE_TOTAL.inc();
                        }
                        Err(e) => {
                            tracing::error!(player_id = %pid_c, error = ?e, "Periodic save failed for player");
                        }
                    }
                });
                saved += 1;
            }
            if saved > 0 {
                tracing::debug!(saved_count = saved, "Periodic save complete");
            }
        }
    });
}

/// Сохранение «грязных» зданий в БД.
#[allow(clippy::significant_drop_tightening)]
pub fn spawn_building_dirty_flush_loop(
    state: Arc<GameState>,
    mut shutdown: broadcast::Receiver<()>,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(45));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = shutdown.recv() => break,
            }

            let mut dirty_entities = Vec::new();
            {
                let mut ecs = state.ecs_write_profiled("building_dirty_flush.scan");
                let mut query = ecs.query::<(Entity, &crate::game::BuildingFlags)>();
                for (entity, flags) in query.iter(&ecs) {
                    if flags.dirty {
                        dirty_entities.push(entity);
                    }
                }
            }

            let mut saved = 0usize;
            for entity in dirty_entities {
                // Извлекаем row БЕЗ снятия dirty — чистим флаг только после
                // успешного save (как в player-loop, см. :58-60). Иначе ошибка БД
                // теряла изменения здания навсегда (флаг уже снят → не ретраится).
                let row = state.modify_building(entity, |ecs, ent| {
                    let flags = ecs.get::<crate::game::BuildingFlags>(ent)?;
                    if flags.dirty {
                        crate::game::buildings::extract_building_row(ecs, ent)
                    } else {
                        None
                    }
                });

                if let Some(r) = row {
                    let db = state.db.clone();
                    match db.save_building(&r).await {
                        Ok(()) => {
                            state.modify_building(entity, |ecs, ent| {
                                if let Some(mut flags) =
                                    ecs.get_mut::<crate::game::BuildingFlags>(ent)
                                {
                                    flags.dirty = false;
                                }
                            });
                            saved += 1;
                        }
                        Err(e) => tracing::error!(error = ?e, "Periodic save failed for building"),
                    }
                }
            }
            if saved > 0 {
                tracing::debug!(count = saved, "Periodic save: flushed buildings");
            }
        }
    });
}

/// Supervisor game-tick'а. Паника внутри tick/schedule означает неизвестное
/// состояние ECS, поэтому процесс должен падать целиком, а не продолжать игру
/// после возможной mid-mutation.
struct TickServices {
    heartbeat: Arc<TickHeartbeat>,
    tick_log_tx: std::sync::mpsc::SyncSender<TickLogEvent>,
    presentation: crate::net::presentation::PresentationRuntime,
}

pub fn spawn_game_tick_loop(state: Arc<GameState>, shutdown: &broadcast::Sender<()>) {
    let mut rx = state
        .commands_rx
        .lock()
        .take()
        .expect("commands_rx already taken");

    let mut shutdown_rx = shutdown.subscribe();
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
    spawn_tick_log_worker(tick_log_rx);
    let presentation = crate::net::presentation::PresentationRuntime::start(state.clone());
    let services = TickServices {
        heartbeat,
        tick_log_tx,
        presentation,
    };

    std::thread::Builder::new()
        .name("openmines-game-tick".to_owned())
        .spawn(move || {
            tracing::info!(tick_rate_ms = tick_rate_ms, "ECS Game Thread started");

            let mut tick_window = TickWindowProfile::new(Instant::now());
            let mut last_warn = Instant::now()
                .checked_sub(std::time::Duration::from_secs(1))
                .unwrap_or_else(Instant::now);
            let mut schedule_clock = ScheduleClock::new(state.schedules.len(), Instant::now());
            let mut sim_tick = crate::game::SimTick::default();

            let tick_duration = std::time::Duration::from_millis(tick_rate_ms);
            let mut previous_tick_started_at = Instant::now();
            let mut next_tick_at = previous_tick_started_at;

            loop {
                let start = Instant::now();
                crate::metrics::TICK_START_INTERVAL_SECONDS
                    .observe(start.duration_since(previous_tick_started_at).as_secs_f64());
                crate::metrics::TICK_WAKE_LATENESS_SECONDS
                    .observe(start.saturating_duration_since(next_tick_at).as_secs_f64());
                previous_tick_started_at = start;
                next_tick_at = start + tick_duration;
                sim_tick = sim_tick.next();
                crate::metrics::SIMULATION_TICK
                    .set(i64::try_from(sim_tick.get()).unwrap_or(i64::MAX));

                if shutdown_rx.try_recv().is_ok() {
                    tracing::info!("ECS Game Thread shutting down");
                    break;
                }

                let state_clone = state.clone();
                let run_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_game_tick_sync(
                        &state_clone,
                        &mut rx,
                        &mut tick_window,
                        &mut last_warn,
                        &mut schedule_clock,
                        &services,
                    )
                }));

                let inner_tick_elapsed = match run_res {
                    Ok(elapsed) => elapsed,
                    Err(panic_err) => {
                        tracing::error!(
                            target: "tickprof",
                            panic = ?panic_err,
                            "GAME TICK PANICKED — aborting process because ECS state may be corrupt"
                        );
                        std::process::exit(101);
                    }
                };

                let elapsed = start.elapsed();
                let outer_overhead = elapsed.saturating_sub(inner_tick_elapsed);
                if outer_overhead > std::time::Duration::from_millis(50) {
                    tracing::warn!(
                        target: "tickprof",
                        total_wall = ?elapsed,
                        inner_tick = ?inner_tick_elapsed,
                        outer_overhead = ?outer_overhead,
                        "SLOW game tick outer overhead"
                    );
                }
                if let Some(remaining) = next_tick_at.checked_duration_since(Instant::now()) {
                    std::thread::sleep(remaining);
                }
            }
        })
        .expect("spawn game tick thread");
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
    SideBoxPersist = 8,
    SideCellConversions = 9,
    SideCellConversionsEcsLockWait = 10,
    SideProgrammatorActions = 11,
    SideDeath = 12,
    SideBotsRender = 13,
    Summary = 14,
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
        8 => "side_box_persist",
        9 => "side_cell_conversions",
        10 => "side_cell_conversions_ecs_lock_wait",
        11 => "side_programmator_actions",
        12 => "side_death",
        13 => "side_bots_render",
        14 => "summary",
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

// Perf-critical 1:1-ref tick loop (C# Step/Update, ServerTime.cs). Тело —
// единый горячий цикл со связанным win_*-инструментарием диагностики фриза;
// механическое дробление ради лимита строк рискует регрессиями фриза
// (см. историю tickprof). Точечный allow в конвенции db/mod.rs / skills.rs.
#[derive(Clone, Copy, Default)]
struct SideProfile {
    broadcasts: std::time::Duration,
    pack_resends: std::time::Duration,
    box_persist: std::time::Duration,
    cell_conversions: std::time::Duration,
    programmator_actions: std::time::Duration,
    death: std::time::Duration,
    bots_render: std::time::Duration,
}

impl SideProfile {
    fn update_max(&mut self, other: Self) {
        self.broadcasts = self.broadcasts.max(other.broadcasts);
        self.pack_resends = self.pack_resends.max(other.pack_resends);
        self.box_persist = self.box_persist.max(other.box_persist);
        self.cell_conversions = self.cell_conversions.max(other.cell_conversions);
        self.programmator_actions = self.programmator_actions.max(other.programmator_actions);
        self.death = self.death.max(other.death);
        self.bots_render = self.bots_render.max(other.bots_render);
    }

    fn dominant(self) -> (&'static str, Duration) {
        [
            ("broadcasts", self.broadcasts),
            ("pack_resends", self.pack_resends),
            ("box_persist", self.box_persist),
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

struct TickWindowProfile {
    start: Instant,
    ticks: u64,
    over_budget: u64,
    max_total: Duration,
    max_dispatch: Duration,
    max_schedule: Duration,
    max_side: Duration,
    max_unprofiled: Duration,
    max_unprofiled_profile: TickUnprofiledProfile,
    max_side_profile: SideProfile,
    max_schedule_tail_profile: ScheduleTailProfile,
    max_actions: usize,
    max_top_schedule: Duration,
    max_top_schedule_name: String,
}

impl TickWindowProfile {
    fn new(start: Instant) -> Self {
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

    fn record(
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

    fn reset(&mut self, start: Instant) {
        *self = Self::new(start);
    }
}

#[derive(Clone, Copy)]
struct TickDurations {
    total: Duration,
    dispatch: Duration,
    schedule: Duration,
    side: Duration,
    unprofiled: Duration,
    unprofiled_profile: TickUnprofiledProfile,
}

#[derive(Clone, Copy, Debug, Default)]
struct TickUnprofiledProfile {
    setup: Duration,
    dispatch_to_schedule: Duration,
    schedule_to_side: Duration,
    side_accounting_gap: Duration,
    remainder: Duration,
}

impl TickUnprofiledProfile {
    fn update_max(&mut self, other: Self) {
        self.setup = self.setup.max(other.setup);
        self.dispatch_to_schedule = self.dispatch_to_schedule.max(other.dispatch_to_schedule);
        self.schedule_to_side = self.schedule_to_side.max(other.schedule_to_side);
        self.side_accounting_gap = self.side_accounting_gap.max(other.side_accounting_gap);
        self.remainder = self.remainder.max(other.remainder);
    }

    const fn total(self) -> Duration {
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
struct ScheduleRunProfile {
    name: String,
    lock_wait: Duration,
    run: Duration,
}

impl ScheduleRunProfile {
    fn total(&self) -> Duration {
        self.lock_wait + self.run
    }
}

type PendingDeathEffect = (
    crate::game::PlayerId,
    i32,
    i32,
    i32,
    crate::net::session::play::death::DeathBroadcasts,
);

type BoxPersistOp = (crate::game::WorldPos, Option<[i64; 6]>);

struct ScheduleTickResult {
    pending_deaths: Vec<PendingDeathEffect>,
    broadcasts: Vec<crate::game::BroadcastEffect>,
    programmator_actions: Vec<crate::game::ProgrammatorAction>,
    cell_conversions: Vec<crate::game::PendingConversion>,
    pack_resends: Vec<(i32, i32)>,
    box_ops: Vec<BoxPersistOp>,
    sched_select: Duration,
    sched_lock_wait: Duration,
    sched_run: Duration,
    schedule_tail_profile: ScheduleTailProfile,
    schedule_runs: Vec<ScheduleRunProfile>,
    sim_profile: SimProfile,
}

struct ScheduleTailOutput {
    pending_deaths: Vec<PendingDeathEffect>,
    broadcasts: Vec<crate::game::BroadcastEffect>,
    programmator_actions: Vec<crate::game::ProgrammatorAction>,
    cell_conversions: Vec<crate::game::PendingConversion>,
    pack_resends: Vec<(i32, i32)>,
    box_ops: Vec<BoxPersistOp>,
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
            pending_deaths: Vec::new(),
            broadcasts: Vec::new(),
            programmator_actions: Vec::new(),
            cell_conversions: Vec::new(),
            pack_resends: Vec::new(),
            box_ops: Vec::new(),
            sched_select,
            sched_lock_wait: Duration::ZERO,
            sched_run: Duration::ZERO,
            schedule_tail_profile: ScheduleTailProfile::default(),
            schedule_runs,
            sim_profile: empty_sim_profile(player_entity_count, online_count),
        }
    }
}

struct TickLogEvent {
    full: bool,
    total: Duration,
    thread_cpu: Duration,
    off_cpu: Duration,
    dispatch: Duration,
    schedule: Duration,
    side: Duration,
    unprofiled: Duration,
    actions: usize,
    deferred_commands: usize,
    tick_budget: Duration,
    schedule_warn_threshold: Duration,
    dominant_section: &'static str,
    top_command_name: &'static str,
    top_command_elapsed: Duration,
    top_schedule_name: String,
    top_schedule_elapsed: Duration,
    sched_select: Duration,
    sched_lock_wait: Duration,
    sched_run: Duration,
    sched_flush: Duration,
    side_profile: SideProfile,
    schedule_tail_profile: ScheduleTailProfile,
    unprofiled_profile: TickUnprofiledProfile,
    sim_profile: SimProfile,
    queue_profile: QueueProfile,
    programmator_action_profile: ProgrammatorActionProfile,
    schedule_runs: Vec<ScheduleRunProfile>,
}

struct ScheduleClock {
    last_runs: Vec<Instant>,
}

#[derive(Clone, Copy)]
struct ScheduleWorkload {
    online_count: usize,
    player_entity_count: usize,
    crafting_due: bool,
}

#[derive(Clone, Copy)]
struct ScheduleCandidate {
    activity: ScheduleActivity,
    interval: Duration,
}

impl ScheduleClock {
    fn new(len: usize, now: Instant) -> Self {
        Self {
            last_runs: vec![now; len],
        }
    }

    fn sync_len(&mut self, len: usize, now: Instant) {
        self.last_runs.resize(len, now);
    }

    fn last_run_mut(&mut self, idx: usize, now: Instant) -> &mut Instant {
        if idx >= self.last_runs.len() {
            self.last_runs.resize(idx + 1, now);
        }
        &mut self.last_runs[idx]
    }

    fn select_due<F>(
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
    state: &Arc<GameState>,
    heartbeat: &TickHeartbeat,
    ecs: &mut bevy_ecs::prelude::World,
    player_entity_count: usize,
    online_count: usize,
) -> ScheduleTailOutput {
    let mut profile = ScheduleTailProfile::default();
    heartbeat.mark(TickStage::FlushQueues);
    let started = Instant::now();
    let pending_deaths =
        crate::net::session::play::death::flush_player_death_queue_after_tick(state, ecs);
    profile.death_queue = started.elapsed();

    let started = Instant::now();
    let broadcasts = std::mem::take(&mut ecs.resource_mut::<crate::game::BroadcastQueue>().0);
    profile.broadcast_queue = started.elapsed();

    let started = Instant::now();
    let programmator_actions =
        std::mem::take(&mut ecs.resource_mut::<crate::game::ProgrammatorQueue>().0);
    profile.programmator_queue = started.elapsed();

    let started = Instant::now();
    let box_ops = std::mem::take(&mut *ecs.resource_mut::<crate::game::BoxPersistQueue>().0.lock());
    profile.box_queue = started.elapsed();

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
        pending_deaths,
        broadcasts,
        programmator_actions,
        cell_conversions,
        pack_resends,
        box_ops,
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

fn run_schedule_phase(
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

    let tail = drain_schedule_tail(
        state,
        heartbeat,
        &mut ecs,
        player_entity_count,
        online_count,
    );

    let tail_t0 = Instant::now();
    drop(ecs);
    let mut tail_profile = tail.profile;
    tail_profile.drop_ecs_lock = tail_t0.elapsed();
    ScheduleTickResult {
        pending_deaths: tail.pending_deaths,
        broadcasts: tail.broadcasts,
        programmator_actions: tail.programmator_actions,
        cell_conversions: tail.cell_conversions,
        pack_resends: tail.pack_resends,
        box_ops: tail.box_ops,
        sched_select,
        sched_lock_wait: lw,
        sched_run: schedule_run_total,
        schedule_tail_profile: tail_profile,
        schedule_runs,
        sim_profile: tail.sim_profile,
    }
}

fn spawn_tick_log_worker(rx: std::sync::mpsc::Receiver<TickLogEvent>) {
    std::thread::Builder::new()
        .name("openmines-tickprof-log".to_owned())
        .spawn(move || {
            while let Ok(event) = rx.recv() {
                log_tick_event(&event);
            }
        })
        .expect("spawn tickprof log worker");
}

#[allow(clippy::too_many_lines)]
fn log_tick_event(event: &TickLogEvent) {
    let (dominant_schedule_tail, dominant_schedule_tail_elapsed) =
        event.schedule_tail_profile.dominant();
    let (dominant_side, dominant_side_elapsed) = event.side_profile.dominant();
    let (dominant_unprofiled, dominant_unprofiled_elapsed) = event.unprofiled_profile.dominant();
    let execution_class = if event.thread_cpu > event.tick_budget {
        "cpu_bound"
    } else if event.off_cpu > event.thread_cpu {
        "preempted"
    } else {
        "mixed"
    };
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
        queue_box_ops = event.queue_profile.box_ops,
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
        sched_tail_death_queue = ?event.schedule_tail_profile.death_queue,
        sched_tail_broadcast_queue = ?event.schedule_tail_profile.broadcast_queue,
        sched_tail_programmator_queue = ?event.schedule_tail_profile.programmator_queue,
        sched_tail_box_queue = ?event.schedule_tail_profile.box_queue,
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
         actions={} side_broadcasts={:?} side_pack_resends={:?} side_box_persist={:?} \
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
        event.side_profile.box_persist,
        event.side_profile.cell_conversions,
        event.side_profile.programmator_actions,
        event.side_profile.death,
        event.side_profile.bots_render,
    );
}

#[derive(Clone, Copy, Debug, Default)]
struct ScheduleTailProfile {
    death_queue: Duration,
    broadcast_queue: Duration,
    programmator_queue: Duration,
    box_queue: Duration,
    cell_conversion_queue: Duration,
    pack_resend_queue: Duration,
    sim_profile: Duration,
    drop_ecs_lock: Duration,
}

impl ScheduleTailProfile {
    fn update_max(&mut self, other: Self) {
        self.death_queue = self.death_queue.max(other.death_queue);
        self.broadcast_queue = self.broadcast_queue.max(other.broadcast_queue);
        self.programmator_queue = self.programmator_queue.max(other.programmator_queue);
        self.box_queue = self.box_queue.max(other.box_queue);
        self.cell_conversion_queue = self.cell_conversion_queue.max(other.cell_conversion_queue);
        self.pack_resend_queue = self.pack_resend_queue.max(other.pack_resend_queue);
        self.sim_profile = self.sim_profile.max(other.sim_profile);
        self.drop_ecs_lock = self.drop_ecs_lock.max(other.drop_ecs_lock);
    }

    fn total(self) -> Duration {
        self.death_queue
            + self.broadcast_queue
            + self.programmator_queue
            + self.box_queue
            + self.cell_conversion_queue
            + self.pack_resend_queue
            + self.sim_profile
            + self.drop_ecs_lock
    }

    fn dominant(self) -> (&'static str, Duration) {
        [
            ("death_queue", self.death_queue),
            ("broadcast_queue", self.broadcast_queue),
            ("programmator_queue", self.programmator_queue),
            ("box_queue", self.box_queue),
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

fn dominant_tick_section(durations: TickDurations) -> &'static str {
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

fn top_schedule_run(schedule_runs: &[ScheduleRunProfile]) -> Option<(&str, Duration)> {
    schedule_runs
        .iter()
        .max_by_key(|profile| profile.total())
        .map(|profile| (profile.name.as_str(), profile.total()))
}

#[derive(Clone, Copy, Debug, Default)]
struct SimProfile {
    player_entities: usize,
    online_players: usize,
    offline_player_entities: usize,
    running_programmators: usize,
    online_running_programmators: usize,
    offline_running_programmators: usize,
}

#[derive(Clone, Copy, Debug, Default)]
struct QueueProfile {
    broadcasts: usize,
    pack_resends: usize,
    box_ops: usize,
    cell_conversions_in: usize,
    cell_conversions_remaining: usize,
    cell_conversions_applied: usize,
    programmator_actions: usize,
    deaths: usize,
}

#[derive(Clone, Copy, Debug, Default)]
struct ProgrammatorActionProfile {
    moves: usize,
    digs: usize,
    builds: usize,
    geo: usize,
    heal: usize,
    set_auto_dig: usize,
    set_aggression: usize,
    set_hand_mode: usize,
    fill_gun: usize,
    set_status: usize,
}

impl ProgrammatorActionProfile {
    const fn count(&mut self, action: &crate::game::ProgrammatorAction) {
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

const fn empty_sim_profile(player_entities: usize, online_players: usize) -> SimProfile {
    SimProfile {
        player_entities,
        online_players,
        offline_player_entities: player_entities.saturating_sub(online_players),
        running_programmators: 0,
        online_running_programmators: 0,
        offline_running_programmators: 0,
    }
}

#[allow(clippy::too_many_lines)]
fn run_game_tick_sync(
    state: &Arc<GameState>,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::game::QueuedPlayerCommand>,
    tick_window: &mut TickWindowProfile,
    last_warn: &mut Instant,
    schedule_clock: &mut ScheduleClock,
    services: &TickServices,
) -> Duration {
    let thread_cpu_started = cpu_time::ThreadTime::now();
    let tick_budget =
        std::time::Duration::from_millis(state.config.gameplay.schedules.game_loop_tick_rate_ms);
    let schedule_warn_threshold = std::time::Duration::from_millis(
        state.config.gameplay.schedules.schedule_warn_threshold_ms,
    );
    let tick_t0 = Instant::now();
    let setup_t0 = Instant::now();
    services.heartbeat.begin_tick();
    let unprofiled_setup = setup_t0.elapsed();

    // 1. Сначала обрабатываем все входящие команды от игроков
    let mut n_actions = 0;
    let mut deferred_commands = 0;
    let mut top_command_name = "-";
    let mut top_command_elapsed = Duration::ZERO;
    let mut command_effects = crate::game::CommandEffects::default();
    let d0 = Instant::now();
    services.heartbeat.mark(TickStage::Dispatch);
    while let Ok(queued) = rx.try_recv() {
        state.record_command_dequeued();
        n_actions += 1;
        let apply_started_at = Instant::now();
        let sequence = queued.sequence;
        let cmd = queued.command;
        let cmd_name = cmd.name();
        crate::metrics::COMMAND_QUEUE_RESIDENCE_SECONDS
            .with_label_values(&[cmd_name])
            .observe(
                apply_started_at
                    .saturating_duration_since(queued.enqueued_at)
                    .as_secs_f64(),
            );
        crate::metrics::COMMAND_RECEIVE_TO_APPLY_SECONDS
            .with_label_values(&[cmd_name])
            .observe(
                apply_started_at
                    .saturating_duration_since(queued.received_at)
                    .as_secs_f64(),
            );
        command_effects.append(crate::game::logic::commands::apply_player_command(
            state, cmd,
        ));
        let cmd_elapsed = apply_started_at.elapsed();
        crate::metrics::COMMAND_APPLY_SECONDS
            .with_label_values(&[cmd_name])
            .observe(cmd_elapsed.as_secs_f64());
        crate::metrics::COMMANDS_TOTAL
            .with_label_values(&[cmd_name, "applied"])
            .inc();
        crate::metrics::COMMAND_SEQUENCE.set(i64::try_from(sequence.get()).unwrap_or(i64::MAX));
        if cmd_elapsed > top_command_elapsed {
            top_command_name = cmd_name;
            top_command_elapsed = cmd_elapsed;
        }
        if d0.elapsed() >= tick_budget && !rx.is_empty() {
            deferred_commands = rx.len();
            break;
        }
    }
    let dt_dispatch = d0.elapsed();

    // 2. ECS + очереди side-effects.
    let sched_t0 = Instant::now();
    let unprofiled_dispatch_to_schedule = sched_t0
        .saturating_duration_since(d0)
        .saturating_sub(dt_dispatch);
    let online_count = state.online_count();
    let player_entity_count = state.player_entity_count();
    let ScheduleTickResult {
        pending_deaths: pending,
        broadcasts,
        programmator_actions: prog_actions,
        cell_conversions,
        pack_resends,
        box_ops,
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
        player_entity_count,
        schedule_warn_threshold,
    );
    let dt_schedule = sched_t0.elapsed();
    let side_t0 = Instant::now();
    let unprofiled_schedule_to_side = side_t0
        .saturating_duration_since(sched_t0)
        .saturating_sub(dt_schedule);
    let sched_flush = dt_schedule
        .saturating_sub(sched_select)
        .saturating_sub(sched_lock_wait)
        .saturating_sub(sched_run);

    // 3. Side-effects: broadcasts + конвертации + программатор + смерти.
    let mut side_profile = SideProfile::default();
    let mut programmator_action_profile = ProgrammatorActionProfile::default();
    for action in &prog_actions {
        programmator_action_profile.count(action);
    }
    let mut queue_profile = QueueProfile {
        broadcasts: broadcasts.len(),
        pack_resends: pack_resends.len(),
        box_ops: box_ops.len(),
        cell_conversions_in: cell_conversions.len(),
        programmator_actions: prog_actions.len(),
        deaths: pending.len(),
        ..QueueProfile::default()
    };
    let due_bots_render = if online_count > 0 {
        state.take_due_bots_render(
            Instant::now(),
            crate::game::GameState::BOTS_RENDER_OBSERVER_BUDGET,
        )
    } else {
        Vec::new()
    };

    let side_has_work = !broadcasts.is_empty()
        || !command_effects.events.is_empty()
        || !command_effects.saves.is_empty()
        || !pack_resends.is_empty()
        || !box_ops.is_empty()
        || !cell_conversions.is_empty()
        || !prog_actions.is_empty()
        || !pending.is_empty()
        || !due_bots_render.is_empty();

    if !side_has_work {
        services.heartbeat.mark(TickStage::Summary);
        let side_end = Instant::now();
        let dt_side = side_end.saturating_duration_since(side_t0);
        let dt_total = tick_t0.elapsed();
        let thread_cpu = thread_cpu_started.elapsed();
        let off_cpu = dt_total.saturating_sub(thread_cpu);
        let unprofiled_side_accounting_gap =
            dt_side.saturating_sub(side_end.saturating_duration_since(side_t0));
        let dt_unprofiled = dt_total
            .saturating_sub(dt_dispatch)
            .saturating_sub(dt_schedule)
            .saturating_sub(dt_side);
        let mut unprofiled_profile = TickUnprofiledProfile {
            setup: unprofiled_setup,
            dispatch_to_schedule: unprofiled_dispatch_to_schedule,
            schedule_to_side: unprofiled_schedule_to_side,
            side_accounting_gap: unprofiled_side_accounting_gap,
            remainder: Duration::ZERO,
        };
        unprofiled_profile.remainder = dt_unprofiled.saturating_sub(unprofiled_profile.total());
        let durations = TickDurations {
            total: dt_total,
            dispatch: dt_dispatch,
            schedule: dt_schedule,
            side: dt_side,
            unprofiled: dt_unprofiled,
            unprofiled_profile,
        };
        let top_schedule = top_schedule_run(&schedule_runs);
        let dominant_section = dominant_tick_section(durations);
        tick_window.record(
            durations,
            side_profile,
            schedule_tail_profile,
            n_actions,
            top_schedule,
            tick_budget,
        );

        if dt_total > tick_budget && last_warn.elapsed() >= std::time::Duration::from_millis(500) {
            *last_warn = Instant::now();
            let top_schedule_name = top_schedule.map_or("-", |(name, _)| name).to_owned();
            let top_schedule_elapsed = top_schedule.map_or(Duration::ZERO, |(_, elapsed)| elapsed);
            let event = TickLogEvent {
                full: dt_total > schedule_warn_threshold,
                total: dt_total,
                thread_cpu,
                off_cpu,
                dispatch: dt_dispatch,
                schedule: dt_schedule,
                side: dt_side,
                unprofiled: dt_unprofiled,
                actions: n_actions,
                deferred_commands,
                tick_budget,
                schedule_warn_threshold,
                dominant_section,
                top_command_name,
                top_command_elapsed,
                top_schedule_name,
                top_schedule_elapsed,
                sched_select,
                sched_lock_wait,
                sched_run,
                sched_flush,
                side_profile,
                schedule_tail_profile,
                unprofiled_profile,
                sim_profile,
                queue_profile,
                programmator_action_profile,
                schedule_runs,
            };
            let _ = services.tick_log_tx.try_send(event);
        }

        return dt_total;
    }

    let section_t0 = Instant::now();
    services.heartbeat.mark(TickStage::SideBroadcasts);
    for event in command_effects.events {
        services.presentation.publish(event);
    }
    for save in command_effects.saves {
        dispatch_save_command(state, save);
    }
    side_profile.broadcasts = section_t0.elapsed();

    let section_t0 = Instant::now();
    services.heartbeat.mark(TickStage::SideBroadcasts);
    for effect in broadcasts {
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
            } => {
                state.broadcast_to_nearby(cx, cy, &data, exclude);
            }
        }
    }
    side_profile.broadcasts += section_t0.elapsed();

    let section_t0 = Instant::now();
    services.heartbeat.mark(TickStage::SidePackResends);
    for (px, py) in pack_resends {
        if let Some(view) = state.get_pack_at(px, py) {
            crate::net::session::social::buildings::broadcast_pack_update(state, &view);
        }
    }
    side_profile.pack_resends = section_t0.elapsed();

    let section_t0 = Instant::now();
    services.heartbeat.mark(TickStage::SideBoxPersist);
    if !box_ops.is_empty() {
        struct DbTaskGuard {
            state: Arc<GameState>,
        }
        impl Drop for DbTaskGuard {
            fn drop(&mut self) {
                self.state
                    .db_pending_tasks
                    .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            }
        }

        let db = state.db.clone();
        let state_clone = state.clone();
        state
            .db_pending_tasks
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        state.tokio_handle.spawn(async move {
            let _guard = DbTaskGuard { state: state_clone };
            for (pos, op) in box_ops {
                let (bx, by): (i32, i32) = pos.into();
                let r = match op {
                    None => db.delete_box_at(bx, by).await,
                    Some(crystals) => db.upsert_box(bx, by, &crystals).await,
                };
                if let Err(e) = r {
                    tracing::error!(x = bx, y = by, error = ?e, "box persist failed");
                }
            }
        });
    }
    side_profile.box_persist = section_t0.elapsed();

    let section_t0 = Instant::now();
    services.heartbeat.mark(TickStage::SideCellConversions);
    let mut remaining_conversions: Vec<crate::game::PendingConversion> = Vec::new();
    let mut converted_owners: Vec<crate::game::player::PlayerId> = Vec::new();
    for mut conv in cell_conversions {
        if conv.ticks_left > 1 {
            conv.ticks_left -= 1;
            remaining_conversions.push(conv);
        } else {
            let (x, y): (i32, i32) = conv.pos.into();
            let should_convert = state.world.valid_coord(x, y)
                && state.world.get_cell_typed(x, y) == conv.required_cell;
            if should_convert {
                state.world.write_world_cell(
                    x,
                    y,
                    crate::world::WorldCell {
                        cell_type: conv.target_cell,
                        durability: conv.durability,
                    },
                );
                crate::game::broadcast_cell_update(state, x, y);
                queue_profile.cell_conversions_applied += 1;
                converted_owners.push(conv.owner_pid);
            }
        }
    }
    queue_profile.cell_conversions_remaining = remaining_conversions.len();
    let mut buildwar_pkts: Vec<(crate::game::player::PlayerId, (&'static str, Vec<u8>))> =
        Vec::new();
    if !remaining_conversions.is_empty() || !converted_owners.is_empty() {
        let ctx = if converted_owners.is_empty() {
            None
        } else {
            Some(crate::game::ExpContext::from_state(state))
        };
        services
            .heartbeat
            .mark(TickStage::SideCellConversionsEcsLockWait);
        let mut ecs = state.ecs_write_profiled("tick.side_cell_conversions");
        services.heartbeat.mark(TickStage::SideCellConversions);
        ecs.resource_mut::<crate::game::PendingCellConversions>().0 = remaining_conversions;
        for owner in converted_owners {
            let Some(entity) = state.get_player_entity(owner) else {
                continue;
            };
            if let Some(mut skills) = ecs.get_mut::<crate::game::player::PlayerSkillsComp>(entity)
                && let Some(ctx) = ctx
                && let Some(sk) = ctx.add_skill_exp(
                    &mut skills.states,
                    crate::game::skills::SkillType::BuildWar.code(),
                    1.0,
                )
            {
                buildwar_pkts.push((owner, sk));
            }
        }
    }
    for (owner, sk_pkt) in buildwar_pkts {
        if let Some(tx) = state.player_sender(owner) {
            let _ = tx.send(crate::net::session::wire::make_u_packet_bytes(
                sk_pkt.0, &sk_pkt.1,
            ));
        }
    }
    side_profile.cell_conversions = section_t0.elapsed();

    let section_t0 = Instant::now();
    services.heartbeat.mark(TickStage::SideProgrammatorActions);
    for action in prog_actions {
        match action {
            crate::game::ProgrammatorAction::Move {
                pid,
                session_id,
                x,
                y,
                dir,
            } => {
                let (tx, _rx) = programmator_action_tx(state, session_id);
                crate::net::session::play::movement::handle_move(
                    state, &tx, pid, 0, x, y, dir, true,
                );
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
                let bld = crate::protocol::packets::XbldClient {
                    direction: dir,
                    block_type: &block_type,
                };
                crate::net::session::play::dig_build::handle_build(state, &tx, pid, &bld, true);
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
                crate::game::logic::commands::apply_programmator_auto_dig_set(
                    state, &tx, pid, enabled,
                );
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
    side_profile.programmator_actions = section_t0.elapsed();

    let section_t0 = Instant::now();
    services.heartbeat.mark(TickStage::SideDeath);
    for (pid, rx, ry, mh, bcast) in pending {
        crate::net::session::play::death::run_death_broadcasts(state, &bcast, pid);
        let tx = state.player_sender(pid);
        if let Some(tx) = tx {
            crate::net::session::play::death::send_respawn_after_death(
                &tx, pid, rx, ry, mh, &bcast,
            );
            crate::net::session::play::chunks::check_chunk_changed(state, &tx, pid);
        }
    }
    side_profile.death = section_t0.elapsed();

    let section_t0 = Instant::now();
    services.heartbeat.mark(TickStage::SideBotsRender);
    let bots_render_result = crate::net::session::play::chunks::bots_render_batch(
        state,
        due_bots_render,
        crate::game::GameState::BOTS_RENDER_BYTE_BUDGET,
    );
    crate::metrics::BOTS_RENDER_OBSERVERS_TOTAL
        .with_label_values(&["completed"])
        .inc_by(u64::try_from(bots_render_result.completed.len()).unwrap_or(u64::MAX));
    crate::metrics::BOTS_RENDER_OBSERVERS_TOTAL
        .with_label_values(&["sent"])
        .inc_by(u64::try_from(bots_render_result.observers_sent).unwrap_or(u64::MAX));
    crate::metrics::BOTS_RENDER_OBSERVERS_TOTAL
        .with_label_values(&["deferred"])
        .inc_by(u64::try_from(bots_render_result.deferred.len()).unwrap_or(u64::MAX));
    crate::metrics::BOTS_RENDER_BYTES_TOTAL
        .inc_by(u64::try_from(bots_render_result.bytes_enqueued).unwrap_or(u64::MAX));
    crate::metrics::BOTS_RENDER_SNAPSHOT_CHUNKS
        .set(i64::try_from(bots_render_result.snapshot_chunks).unwrap_or(i64::MAX));
    let bots_render_now = Instant::now();
    for due in bots_render_result.completed {
        state.reschedule_bots_render(
            due,
            bots_render_now + crate::game::GameState::BOTS_RENDER_INTERVAL,
        );
    }
    for due in bots_render_result.deferred {
        state.reschedule_bots_render(due, bots_render_now + tick_budget);
    }
    side_profile.bots_render = section_t0.elapsed();

    // ── Stage 0: агрегация и throttled-вывод (target=tickprof) ──
    let side_end = Instant::now();
    services.heartbeat.mark(TickStage::Summary);
    let dt_side = side_t0.elapsed();
    let dt_total = tick_t0.elapsed();
    let thread_cpu = thread_cpu_started.elapsed();
    let off_cpu = dt_total.saturating_sub(thread_cpu);
    let unprofiled_side_accounting_gap =
        dt_side.saturating_sub(side_end.saturating_duration_since(side_t0));
    let dt_unprofiled = dt_total
        .saturating_sub(dt_dispatch)
        .saturating_sub(dt_schedule)
        .saturating_sub(dt_side);
    let mut unprofiled_profile = TickUnprofiledProfile {
        setup: unprofiled_setup,
        dispatch_to_schedule: unprofiled_dispatch_to_schedule,
        schedule_to_side: unprofiled_schedule_to_side,
        side_accounting_gap: unprofiled_side_accounting_gap,
        remainder: Duration::ZERO,
    };
    unprofiled_profile.remainder = dt_unprofiled.saturating_sub(unprofiled_profile.total());
    let durations = TickDurations {
        total: dt_total,
        dispatch: dt_dispatch,
        schedule: dt_schedule,
        side: dt_side,
        unprofiled: dt_unprofiled,
        unprofiled_profile,
    };
    let top_schedule = top_schedule_run(&schedule_runs);
    let dominant_section = dominant_tick_section(durations);
    tick_window.record(
        durations,
        side_profile,
        schedule_tail_profile,
        n_actions,
        top_schedule,
        tick_budget,
    );

    if dt_total > tick_budget && last_warn.elapsed() >= std::time::Duration::from_millis(500) {
        *last_warn = Instant::now();
        let top_schedule_name = top_schedule.map_or("-", |(name, _)| name).to_owned();
        let top_schedule_elapsed = top_schedule.map_or(Duration::ZERO, |(_, elapsed)| elapsed);
        let event = TickLogEvent {
            full: dt_total > schedule_warn_threshold,
            total: dt_total,
            thread_cpu,
            off_cpu,
            dispatch: dt_dispatch,
            schedule: dt_schedule,
            side: dt_side,
            unprofiled: dt_unprofiled,
            actions: n_actions,
            deferred_commands,
            tick_budget,
            schedule_warn_threshold,
            dominant_section,
            top_command_name,
            top_command_elapsed,
            top_schedule_name,
            top_schedule_elapsed,
            sched_select,
            sched_lock_wait,
            sched_run,
            sched_flush,
            side_profile,
            schedule_tail_profile,
            unprofiled_profile,
            sim_profile,
            queue_profile,
            programmator_action_profile,
            schedule_runs,
        };
        let _ = services.tick_log_tx.try_send(event);
    }

    if tick_window.start.elapsed() >= std::time::Duration::from_secs(5) {
        tracing::debug!(
            target: "tickprof",
            "5s summary: ticks={} over_budget={} \
             max_total={:?} max_dispatch={:?} \
             max_schedule={:?} max_side={:?} \
             max_unprofiled={:?} max_actions={} max_top_schedule={} max_top_schedule_elapsed={:?} max_side_broadcasts={:?} \
             max_side_pack_resends={:?} max_side_box_persist={:?} \
             max_side_cell_conversions={:?} max_side_programmator_actions={:?} \
             max_side_death={:?} max_side_bots_render={:?} \
             max_sched_tail_death_queue={:?} max_sched_tail_broadcast_queue={:?} \
             max_sched_tail_programmator_queue={:?} max_sched_tail_box_queue={:?} \
             max_sched_tail_cell_conversion_queue={:?} max_sched_tail_pack_resend_queue={:?} \
             max_sched_tail_sim_profile={:?} max_sched_tail_drop_ecs_lock={:?} \
             max_unprofiled_setup={:?} max_unprofiled_dispatch_to_schedule={:?} \
             max_unprofiled_schedule_to_side={:?} max_unprofiled_side_accounting_gap={:?} \
             max_unprofiled_remainder={:?}",
            tick_window.ticks,
            tick_window.over_budget,
            tick_window.max_total,
            tick_window.max_dispatch,
            tick_window.max_schedule,
            tick_window.max_side,
            tick_window.max_unprofiled,
            tick_window.max_actions,
            tick_window.max_top_schedule_name,
            tick_window.max_top_schedule,
            tick_window.max_side_profile.broadcasts,
            tick_window.max_side_profile.pack_resends,
            tick_window.max_side_profile.box_persist,
            tick_window.max_side_profile.cell_conversions,
            tick_window.max_side_profile.programmator_actions,
            tick_window.max_side_profile.death,
            tick_window.max_side_profile.bots_render,
            tick_window.max_schedule_tail_profile.death_queue,
            tick_window.max_schedule_tail_profile.broadcast_queue,
            tick_window.max_schedule_tail_profile.programmator_queue,
            tick_window.max_schedule_tail_profile.box_queue,
            tick_window.max_schedule_tail_profile.cell_conversion_queue,
            tick_window.max_schedule_tail_profile.pack_resend_queue,
            tick_window.max_schedule_tail_profile.sim_profile,
            tick_window.max_schedule_tail_profile.drop_ecs_lock,
            tick_window.max_unprofiled_profile.setup,
            tick_window.max_unprofiled_profile.dispatch_to_schedule,
            tick_window.max_unprofiled_profile.schedule_to_side,
            tick_window.max_unprofiled_profile.side_accounting_gap,
            tick_window.max_unprofiled_profile.remainder,
        );
        tick_window.reset(Instant::now());
    }
    services.heartbeat.mark(TickStage::Idle);
    dt_total
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

fn dispatch_save_command(state: &Arc<GameState>, command: crate::game::SaveCommand) {
    let crate::game::SaveCommand::SavePlayer { row } = command;

    let db = state.db.clone();
    let state_clone = state.clone();
    state
        .db_pending_tasks
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    state.tokio_handle.spawn(async move {
        let _guard = PendingSaveGuard { state: state_clone };
        let mut attempts = 0;
        let mut backoff = Duration::from_millis(100);
        loop {
            match db.save_player(&row).await {
                Ok(()) => break,
                Err(error) => {
                    attempts += 1;
                    if attempts >= 3 {
                        tracing::error!(
                            player_id = row.id,
                            error = ?error,
                            "Failed to save player after 3 attempts"
                        );
                        break;
                    }
                    tracing::warn!(
                        player_id = row.id,
                        error = ?error,
                        attempt = attempts,
                        ?backoff,
                        "Failed to save player; retrying"
                    );
                    tokio::time::sleep(backoff).await;
                    backoff *= 2;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
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
            box_persist: std::time::Duration::from_millis(2),
            cell_conversions: std::time::Duration::from_millis(4),
            programmator_actions: std::time::Duration::from_millis(3),
            death: std::time::Duration::from_millis(7),
            bots_render: std::time::Duration::from_millis(6),
        };

        profile.update_max(SideProfile {
            broadcasts: std::time::Duration::from_millis(9),
            pack_resends: std::time::Duration::from_millis(1),
            box_persist: std::time::Duration::from_millis(8),
            cell_conversions: std::time::Duration::from_millis(2),
            programmator_actions: std::time::Duration::from_millis(10),
            death: std::time::Duration::from_millis(1),
            bots_render: std::time::Duration::from_millis(11),
        });

        assert_eq!(profile.broadcasts, std::time::Duration::from_millis(9));
        assert_eq!(profile.pack_resends, std::time::Duration::from_millis(5));
        assert_eq!(profile.box_persist, std::time::Duration::from_millis(8));
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
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();
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

        broadcast_online_count(&state);

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
}
