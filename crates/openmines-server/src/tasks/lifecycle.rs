//! Фоновые задачи: сброс мира, периодическое сохранение игроков, сохранение при остановке.
//! Отделено от `run()` в `mod.rs`, чтобы тот отвечал только за accept TCP (SRP).

use crate::game::GameState;
use crate::world::WorldProvider;
use bevy_ecs::prelude::Entity;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

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
            let _ = tokio::task::spawn_blocking(move || {
                if let Err(e) = state_c.world.flush() {
                    tracing::error!(error = ?e, "World flush error");
                }
            })
            .await;
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

/// Supervisor game-tick'а: спавнит `run_game_tick` и РЕСПАВНИТ его при панике,
/// чтобы паника в одном TY-хендлере / ECS-системе / side-effect не превращала
/// сервер в «зомби» (accept-loop жив, игровая логика мертва навсегда). Backoff
/// 200ms между рестартами — не спинить CPU при устойчивой панике. `EcsWorld`
/// живёт в `GameState` (не пересоздаётся): после паники под `ecs.write()` guard
/// снимается (`parking_lot` без poison), следующий тик берёт лок штатно.
pub fn spawn_game_tick_loop(state: Arc<GameState>, shutdown: &broadcast::Sender<()>) {
    let mut rx = state
        .commands_rx
        .lock()
        .take()
        .expect("commands_rx already taken");

    let mut shutdown_rx = shutdown.subscribe();
    let tick_rate_ms = state.config.gameplay.schedules.game_loop_tick_rate_ms;
    let panic_backoff_ms = state.config.gameplay.schedules.game_loop_panic_backoff_ms;
    let heartbeat = Arc::new(TickHeartbeat::new(Instant::now()));
    spawn_game_tick_watchdog(
        state.clone(),
        heartbeat.clone(),
        shutdown.subscribe(),
        tick_rate_ms,
    );
    spawn_parking_lot_deadlock_detector(shutdown.subscribe());

    std::thread::Builder::new()
        .name("openmines-game-tick".to_owned())
        .spawn(move || {
            tracing::info!(
                tick_rate_ms = tick_rate_ms,
                panic_backoff_ms = panic_backoff_ms,
                "ECS Game Thread started"
            );

            let mut tick_window = TickWindowProfile::new(Instant::now());
            let mut last_warn = Instant::now()
                .checked_sub(std::time::Duration::from_secs(1))
                .unwrap_or_else(Instant::now);

            let tick_duration = std::time::Duration::from_millis(tick_rate_ms);
            let backoff_duration = std::time::Duration::from_millis(panic_backoff_ms);

            loop {
                let start = Instant::now();

                if shutdown_rx.try_recv().is_ok() {
                    tracing::info!("ECS Game Thread shutting down");
                    break;
                }

                let state_clone = state.clone();
                let run_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_game_tick_sync(
                        &state_clone,
                        &mut rx,
                        &heartbeat,
                        &mut tick_window,
                        &mut last_warn,
                    );
                }));

                if let Err(panic_err) = run_res {
                    tracing::error!(
                        target: "tickprof",
                        panic = ?panic_err,
                        "GAME TICK PANICKED — thread loop continues (ECS could be mid-mutation)"
                    );
                    std::thread::sleep(backoff_duration);
                }

                let elapsed = start.elapsed();
                if let Some(remaining) = tick_duration.checked_sub(elapsed) {
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
    last_mark_ms: AtomicU64,
    tick_seq: AtomicU64,
    stage: AtomicU8,
    schedule_index: AtomicU64,
}

impl TickHeartbeat {
    const fn new(started_at: Instant) -> Self {
        Self {
            started_at,
            last_mark_ms: AtomicU64::new(0),
            tick_seq: AtomicU64::new(0),
            stage: AtomicU8::new(TickStage::Idle as u8),
            schedule_index: AtomicU64::new(u64::MAX),
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
    max_side_profile: SideProfile,
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
            max_side_profile: SideProfile::default(),
            max_actions: 0,
            max_top_schedule: Duration::ZERO,
            max_top_schedule_name: "-".to_string(),
        }
    }

    fn record(
        &mut self,
        durations: TickDurations,
        side_profile: SideProfile,
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
        self.max_side_profile.update_max(side_profile);
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

fn top_schedule_run(schedule_runs: &[(String, Duration)]) -> Option<(&str, Duration)> {
    schedule_runs
        .iter()
        .max_by_key(|(_, elapsed)| *elapsed)
        .map(|(name, elapsed)| (name.as_str(), *elapsed))
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
        }
    }
}

fn collect_sim_profile(ecs: &mut bevy_ecs::world::World, online_players: usize) -> SimProfile {
    let player_entities = ecs
        .query::<&crate::game::player::PlayerPosition>()
        .iter(ecs)
        .count();
    let (running_programmators, online_running_programmators) = ecs
        .query::<(
            Option<&crate::game::player::PlayerConnection>,
            &crate::game::programmator::ProgrammatorState,
        )>()
        .iter(ecs)
        .fold(
            (0usize, 0usize),
            |(running, online_running), (conn, prog)| {
                if !prog.running {
                    return (running, online_running);
                }
                (running + 1, online_running + usize::from(conn.is_some()))
            },
        );

    SimProfile {
        player_entities,
        online_players,
        offline_player_entities: player_entities.saturating_sub(online_players),
        running_programmators,
        online_running_programmators,
        offline_running_programmators: running_programmators
            .saturating_sub(online_running_programmators),
    }
}

#[allow(clippy::too_many_lines)]
fn run_game_tick_sync(
    state: &Arc<GameState>,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::game::PlayerCommand>,
    heartbeat: &TickHeartbeat,
    tick_window: &mut TickWindowProfile,
    last_warn: &mut Instant,
) {
    let tick_budget =
        std::time::Duration::from_millis(state.config.gameplay.schedules.game_loop_tick_rate_ms);
    let schedule_warn_threshold = std::time::Duration::from_millis(
        state.config.gameplay.schedules.schedule_warn_threshold_ms,
    );
    let tick_t0 = Instant::now();
    heartbeat.begin_tick();

    // 1. Сначала обрабатываем все входящие команды от игроков
    let mut n_actions = 0;
    let d0 = Instant::now();
    heartbeat.mark(TickStage::Dispatch);
    while let Ok(cmd) = rx.try_recv() {
        n_actions += 1;
        match cmd {
            crate::game::PlayerCommand::Connect { row, tx, token } => {
                crate::net::session::player::init::connect_in_tick(state, &tx, &row, token);
            }
            crate::game::PlayerCommand::Disconnect { player_id, token } => {
                crate::net::session::player::init::disconnect_in_tick(state, player_id, token);
            }
            crate::game::PlayerCommand::Ty {
                player_id,
                tx,
                packet,
            } => {
                let state_clone = state.clone();
                let tx_clone = tx;
                state.tokio_handle.spawn(async move {
                    if let Err(e) = crate::net::session::dispatch::dispatch_ty_packet(
                        &state_clone,
                        &tx_clone,
                        player_id,
                        &packet,
                    )
                    .await
                    {
                        tracing::error!(
                            player_id = %player_id,
                            error = ?e,
                            "Failed to dispatch TY packet command"
                        );
                    }
                });
            }
            _ => {}
        }
    }
    let dt_dispatch = d0.elapsed();

    // 2. ECS + очереди side-effects.
    let sched_t0 = Instant::now();
    let (
        pending,
        broadcasts,
        prog_actions,
        cell_conversions,
        pack_resends,
        box_ops,
        sched_lock_wait,
        sched_run,
        schedule_runs,
        sim_profile,
    ) = {
        let building_entities = state.building_entities_snapshot();
        heartbeat.mark(TickStage::EcsLockWait);
        let mut ecs = state.ecs_write_profiled("tick.schedule");
        let lw = sched_t0.elapsed();
        let run_t0 = Instant::now();
        let mut schedule_runs: Vec<(String, std::time::Duration)> = Vec::new();

        let now = Instant::now();
        for (idx, gs) in state.schedules.iter().enumerate() {
            let interval_ms = gs.interval_ms.load(std::sync::atomic::Ordering::Relaxed);
            if interval_ms == 0 {
                continue;
            }
            let interval = std::time::Duration::from_millis(interval_ms);
            let mut last_run = gs.last_run.lock();
            if now.duration_since(*last_run) >= interval {
                heartbeat.mark_schedule(TickStage::ScheduleRun, idx.try_into().unwrap_or(u64::MAX));
                let schedule_t0 = Instant::now();
                let run_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    gs.schedule.write().run(&mut ecs);
                }));
                let elapsed = schedule_t0.elapsed();
                schedule_runs.push((gs.name.clone(), elapsed));
                if elapsed > schedule_warn_threshold {
                    tracing::warn!(
                        target: "scheduler",
                        schedule = %gs.name,
                        duration = ?elapsed,
                        threshold = ?schedule_warn_threshold,
                        "System schedule execution exceeded warning threshold"
                    );
                }
                if let Err(panic_err) = run_res {
                    tracing::error!(
                        target: "scheduler",
                        schedule = %gs.name,
                        panic = ?panic_err,
                        "PANIC occurred in system schedule execution"
                    );
                }
                *last_run = now;
            }
        }

        let rn = run_t0.elapsed();

        heartbeat.mark(TickStage::FlushQueues);
        let p = crate::net::session::play::death::flush_player_death_queue_after_tick(
            state,
            &mut ecs,
            &building_entities,
        );
        let bc = std::mem::take(&mut ecs.resource_mut::<crate::game::BroadcastQueue>().0);
        let pa = std::mem::take(&mut ecs.resource_mut::<crate::game::ProgrammatorQueue>().0);
        let bp = std::mem::take(&mut *ecs.resource_mut::<crate::game::BoxPersistQueue>().0.lock());
        let convs =
            std::mem::take(&mut ecs.resource_mut::<crate::game::PendingCellConversions>().0);
        let pr = std::mem::take(&mut ecs.resource_mut::<crate::game::PackResendQueue>().0);
        let sim_profile = collect_sim_profile(&mut ecs, state.online_count());
        drop(ecs);
        (p, bc, pa, convs, pr, bp, lw, rn, schedule_runs, sim_profile)
    };
    let dt_schedule = sched_t0.elapsed();
    let sched_flush = dt_schedule
        .saturating_sub(sched_lock_wait)
        .saturating_sub(sched_run);

    // 3. Side-effects: broadcasts + конвертации + программатор + смерти.
    let side_t0 = Instant::now();
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

    let section_t0 = Instant::now();
    heartbeat.mark(TickStage::SideBroadcasts);
    for effect in broadcasts {
        match effect {
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
    side_profile.broadcasts = section_t0.elapsed();

    let section_t0 = Instant::now();
    heartbeat.mark(TickStage::SidePackResends);
    for (px, py) in pack_resends {
        if let Some(view) = state.get_pack_at(px, py) {
            crate::net::session::social::buildings::broadcast_pack_update(state, &view);
        }
    }
    side_profile.pack_resends = section_t0.elapsed();

    let section_t0 = Instant::now();
    heartbeat.mark(TickStage::SideBoxPersist);
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
    heartbeat.mark(TickStage::SideCellConversions);
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
    let ctx = crate::game::ExpContext::from_state(state);
    let mut buildwar_pkts: Vec<(crate::game::player::PlayerId, (&'static str, Vec<u8>))> =
        Vec::new();
    {
        heartbeat.mark(TickStage::SideCellConversionsEcsLockWait);
        let mut ecs = state.ecs_write_profiled("tick.side_cell_conversions");
        heartbeat.mark(TickStage::SideCellConversions);
        ecs.resource_mut::<crate::game::PendingCellConversions>().0 = remaining_conversions;
        for owner in converted_owners {
            let Some(entity) = state.get_player_entity(owner) else {
                continue;
            };
            if let Some(mut skills) = ecs.get_mut::<crate::game::player::PlayerSkillsComp>(entity)
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
    heartbeat.mark(TickStage::SideProgrammatorActions);
    for action in prog_actions {
        match action {
            crate::game::ProgrammatorAction::Move { pid, tx, x, y, dir } => {
                let (tx, _rx) = programmator_action_tx(tx);
                crate::net::session::play::movement::handle_move(
                    state, &tx, pid, 0, x, y, dir, true,
                );
            }
            crate::game::ProgrammatorAction::Dig { pid, tx, dir } => {
                let (tx, _rx) = programmator_action_tx(tx);
                crate::net::session::play::dig_build::handle_dig(state, &tx, pid, dir, true);
            }
            crate::game::ProgrammatorAction::Build {
                pid,
                tx,
                dir,
                block_type,
            } => {
                let (tx, _rx) = programmator_action_tx(tx);
                let bld = crate::protocol::packets::XbldClient {
                    direction: dir,
                    block_type: &block_type,
                };
                crate::net::session::play::dig_build::handle_build(state, &tx, pid, &bld, true);
            }
            crate::game::ProgrammatorAction::Geo { pid, tx } => {
                let (tx, _rx) = programmator_action_tx(tx);
                crate::net::session::play::geo::handle_geo(state, &tx, pid, true);
            }
            crate::game::ProgrammatorAction::Heal { pid, tx } => {
                let (tx, _rx) = programmator_action_tx(tx);
                crate::net::session::ui::heal_inventory::handle_heal(state, &tx, pid, true);
            }
            crate::game::ProgrammatorAction::SetAutoDig { pid, tx, enabled } => {
                let (tx, _rx) = programmator_action_tx(tx);
                crate::net::session::social::misc::handle_auto_dig_set(state, &tx, pid, enabled);
            }
            crate::game::ProgrammatorAction::SetAggression { pid, tx, enabled } => {
                let (tx, _rx) = programmator_action_tx(tx);
                crate::net::session::social::misc::handle_aggression_set(state, &tx, pid, enabled);
            }
            crate::game::ProgrammatorAction::SetHandMode { tx, enabled } => {
                if let Some(tx) = tx {
                    let packet = crate::protocol::packets::hand_mode(enabled);
                    let _ = tx.send(crate::net::session::wire::make_u_packet_bytes(
                        packet.0, &packet.1,
                    ));
                }
            }
            crate::game::ProgrammatorAction::FillGun { pid, tx, x, y } => {
                let (tx, _rx) = programmator_action_tx(tx);
                crate::net::session::play::packs::handle_gun_fill_prog(state, &tx, pid, x, y);
            }
            crate::game::ProgrammatorAction::SetProgrammatorStatus { tx, running } => {
                if let Some(tx) = tx {
                    let _ = tx.send(crate::net::session::wire::make_u_packet_bytes(
                        "@P",
                        &crate::protocol::packets::programmator_status(running).1,
                    ));
                }
            }
        }
    }
    side_profile.programmator_actions = section_t0.elapsed();

    let section_t0 = Instant::now();
    heartbeat.mark(TickStage::SideDeath);
    for (pid, rx, ry, mh, bcast) in pending {
        crate::net::session::play::death::run_death_broadcasts(state, &bcast, pid);
        let tx = state.query_player_opt(pid, |ecs, entity| {
            ecs.get::<crate::game::player::PlayerConnection>(entity)
                .map(|c| c.tx.clone())
        });
        if let Some(tx) = tx {
            crate::net::session::play::death::send_respawn_after_death(
                &tx, pid, rx, ry, mh, &bcast,
            );
            crate::net::session::play::chunks::check_chunk_changed(state, &tx, pid);
        }
    }
    side_profile.death = section_t0.elapsed();

    let section_t0 = Instant::now();
    heartbeat.mark(TickStage::SideBotsRender);
    {
        let now_render = Instant::now();
        let due = state.take_due_bots_render(now_render, std::time::Duration::from_secs(4));
        for pid in due {
            if let Some(tx) = state.player_sender(pid) {
                crate::net::session::play::chunks::bots_render(state, &tx, pid);
            }
        }
    }
    side_profile.bots_render = section_t0.elapsed();

    // ── Stage 0: агрегация и throttled-вывод (target=tickprof) ──
    heartbeat.mark(TickStage::Summary);
    let dt_side = side_t0.elapsed();
    let dt_total = tick_t0.elapsed();
    let dt_unprofiled = dt_total
        .saturating_sub(dt_dispatch)
        .saturating_sub(dt_schedule)
        .saturating_sub(dt_side);
    let durations = TickDurations {
        total: dt_total,
        dispatch: dt_dispatch,
        schedule: dt_schedule,
        side: dt_side,
        unprofiled: dt_unprofiled,
    };
    let top_schedule = top_schedule_run(&schedule_runs);
    let dominant_section = dominant_tick_section(durations);
    tick_window.record(
        durations,
        side_profile,
        n_actions,
        top_schedule,
        tick_budget,
    );

    if dt_total > schedule_warn_threshold
        && last_warn.elapsed() >= std::time::Duration::from_millis(500)
    {
        *last_warn = Instant::now();
        tracing::warn!(
            target: "tickprof",
            sim_player_entities = sim_profile.player_entities,
            sim_online_players = sim_profile.online_players,
            sim_offline_player_entities = sim_profile.offline_player_entities,
            sim_running_programmators = sim_profile.running_programmators,
            sim_online_running_programmators = sim_profile.online_running_programmators,
            sim_offline_running_programmators = sim_profile.offline_running_programmators,
            queue_broadcasts = queue_profile.broadcasts,
            queue_pack_resends = queue_profile.pack_resends,
            queue_box_ops = queue_profile.box_ops,
            queue_cell_conversions_in = queue_profile.cell_conversions_in,
            queue_cell_conversions_remaining = queue_profile.cell_conversions_remaining,
            queue_cell_conversions_applied = queue_profile.cell_conversions_applied,
            queue_programmator_actions = queue_profile.programmator_actions,
            queue_deaths = queue_profile.deaths,
            prog_moves = programmator_action_profile.moves,
            prog_digs = programmator_action_profile.digs,
            prog_builds = programmator_action_profile.builds,
            prog_geo = programmator_action_profile.geo,
            prog_heal = programmator_action_profile.heal,
            prog_set_auto_dig = programmator_action_profile.set_auto_dig,
            prog_set_aggression = programmator_action_profile.set_aggression,
            prog_set_hand_mode = programmator_action_profile.set_hand_mode,
            prog_fill_gun = programmator_action_profile.fill_gun,
            prog_set_status = programmator_action_profile.set_status,
            dominant_section,
            top_schedule = top_schedule.map_or("-", |(name, _)| name),
            top_schedule_elapsed = ?top_schedule.map_or(Duration::ZERO, |(_, elapsed)| elapsed),
            sched_lock_wait = ?sched_lock_wait,
            sched_run = ?sched_run,
            sched_flush = ?sched_flush,
            schedule_runs = ?schedule_runs,
            "OVER-BUDGET tick: total={dt_total:?} dispatch={dt_dispatch:?} \
             schedule={dt_schedule:?} side={dt_side:?} unprofiled={dt_unprofiled:?} \
             actions={n_actions} side_broadcasts={:?} side_pack_resends={:?} \
             side_box_persist={:?} side_cell_conversions={:?} \
             side_programmator_actions={:?} side_death={:?} side_bots_render={:?}",
            side_profile.broadcasts,
            side_profile.pack_resends,
            side_profile.box_persist,
            side_profile.cell_conversions,
            side_profile.programmator_actions,
            side_profile.death,
            side_profile.bots_render,
        );
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
             max_side_death={:?} max_side_bots_render={:?}",
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
        );
        tick_window.reset(Instant::now());
    }
    heartbeat.mark(TickStage::Idle);
}

fn programmator_action_tx(
    tx: Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>,
) -> (
    tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    Option<tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>>,
) {
    tx.map_or_else(
        || {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            (tx, Some(rx))
        },
        |tx| (tx, None),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use std::sync::Arc;
    use tokio::sync::mpsc;

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

        let (tx1, mut rx1) = mpsc::unbounded_channel();
        let (tx2, mut rx2) = mpsc::unbounded_channel();
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

    fn drain_queued_packets(rx: &mut mpsc::UnboundedReceiver<Vec<u8>>) {
        while rx.try_recv().is_ok() {}
    }

    fn assert_online_packet(rx: &mut mpsc::UnboundedReceiver<Vec<u8>>, expected_payload: &[u8]) {
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
