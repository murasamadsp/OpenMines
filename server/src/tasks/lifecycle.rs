//! Фоновые задачи: сброс мира, периодическое сохранение игроков, сохранение при остановке.
//! Отделено от `run()` в `mod.rs`, чтобы тот отвечал только за accept TCP (SRP).

use crate::game::GameState;
use crate::world::WorldProvider;
use bevy_ecs::prelude::Entity;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::broadcast;

/// Периодический flush mmap-слоёв мира.
pub fn spawn_world_flush_loop(state: Arc<GameState>, mut shutdown: broadcast::Receiver<()>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_mins(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = shutdown.recv() => break,
            }
            state.prune_auth_failures_by_addr(Instant::now());
            // C# World.Update: ежечасный пересчёт цен кристаллов (self-throttled на 1ч).
            crate::game::market::tick_crystal_prices(&state);
            let t0 = std::time::Instant::now();
            tracing::warn!(target: "tickprof", "WORLD FLUSH start");
            let state_c = state.clone();
            let _ = tokio::task::spawn_blocking(move || {
                if let Err(e) = state_c.world.flush() {
                    tracing::error!(error = ?e, "World flush error");
                }
            })
            .await;
            tracing::warn!(target: "tickprof", elapsed = ?t0.elapsed(), "WORLD FLUSH end");
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
                let mut ecs = state.ecs.write();
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

    std::thread::spawn(move || {
        tracing::info!(
            tick_rate_ms = tick_rate_ms,
            panic_backoff_ms = panic_backoff_ms,
            "ECS Game Thread started"
        );

        let mut win_start = Instant::now();
        let mut win_ticks: u64 = 0;
        let mut win_over: u64 = 0;
        let mut win_max_total = std::time::Duration::ZERO;
        let mut win_max_dispatch = std::time::Duration::ZERO;
        let mut win_max_schedule = std::time::Duration::ZERO;
        let mut win_max_side = std::time::Duration::ZERO;
        let mut win_max_side_profile = SideProfile::default();
        let mut win_max_actions: usize = 0;
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
                    &mut win_ticks,
                    &mut win_over,
                    &mut win_max_total,
                    &mut win_max_dispatch,
                    &mut win_max_schedule,
                    &mut win_max_side,
                    &mut win_max_side_profile,
                    &mut win_max_actions,
                    &mut last_warn,
                    &mut win_start,
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
    });
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

#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
fn run_game_tick_sync(
    state: &Arc<GameState>,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::game::PlayerCommand>,
    win_ticks: &mut u64,
    win_over: &mut u64,
    win_max_total: &mut std::time::Duration,
    win_max_dispatch: &mut std::time::Duration,
    win_max_schedule: &mut std::time::Duration,
    win_max_side: &mut std::time::Duration,
    win_max_side_profile: &mut SideProfile,
    win_max_actions: &mut usize,
    last_warn: &mut Instant,
    win_start: &mut Instant,
) {
    let tick_budget =
        std::time::Duration::from_millis(state.config.gameplay.schedules.game_loop_tick_rate_ms);
    let tick_t0 = Instant::now();

    // 1. Сначала обрабатываем все входящие команды от игроков
    let mut n_actions = 0;
    let d0 = Instant::now();
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
    ) = {
        let mut ecs = state.ecs.write();
        let lw = sched_t0.elapsed();
        let run_t0 = Instant::now();

        let now = Instant::now();
        for gs in &state.schedules {
            let interval_ms = gs.interval_ms.load(std::sync::atomic::Ordering::Relaxed);
            if interval_ms == 0 {
                continue;
            }
            let interval = std::time::Duration::from_millis(interval_ms);
            let mut last_run = gs.last_run.lock();
            if now.duration_since(*last_run) >= interval {
                let schedule_t0 = Instant::now();
                let run_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    gs.schedule.write().run(&mut ecs);
                }));
                let elapsed = schedule_t0.elapsed();
                if elapsed > std::time::Duration::from_millis(5) {
                    tracing::warn!(
                        target: "scheduler",
                        schedule = %gs.name,
                        duration = ?elapsed,
                        "System schedule execution exceeded warning threshold (5ms)"
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

        let p =
            crate::net::session::play::death::flush_player_death_queue_after_tick(state, &mut ecs);
        let bc = std::mem::take(&mut ecs.resource_mut::<crate::game::BroadcastQueue>().0);
        let pa = std::mem::take(&mut ecs.resource_mut::<crate::game::ProgrammatorQueue>().0);
        let bp = std::mem::take(&mut *ecs.resource_mut::<crate::game::BoxPersistQueue>().0.lock());
        let convs =
            std::mem::take(&mut ecs.resource_mut::<crate::game::PendingCellConversions>().0);
        let pr = std::mem::take(&mut ecs.resource_mut::<crate::game::PackResendQueue>().0);
        drop(ecs);
        (p, bc, pa, convs, pr, bp, lw, rn)
    };
    let dt_schedule = sched_t0.elapsed();
    if dt_schedule > std::time::Duration::from_millis(50) {
        tracing::warn!(
            target: "tickprof",
            "SLOW schedule: total={dt_schedule:?} lock_wait={sched_lock_wait:?} \
             run={sched_run:?} flush={:?}",
            dt_schedule
                .saturating_sub(sched_lock_wait)
                .saturating_sub(sched_run)
        );
    }

    // 3. Side-effects: broadcasts + конвертации + программатор + смерти.
    let side_t0 = Instant::now();
    let mut side_profile = SideProfile::default();

    let section_t0 = Instant::now();
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
    for (px, py) in pack_resends {
        if let Some(view) = state.get_pack_at(px, py) {
            crate::net::session::social::buildings::broadcast_pack_update(state, &view);
        }
    }
    side_profile.pack_resends = section_t0.elapsed();

    let section_t0 = Instant::now();
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
                converted_owners.push(conv.owner_pid);
            }
        }
    }
    let ctx = crate::game::ExpContext::from_state(state);
    let mut buildwar_pkts: Vec<(crate::game::player::PlayerId, (&'static str, Vec<u8>))> =
        Vec::new();
    {
        let mut ecs = state.ecs.write();
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
    for action in prog_actions {
        match action {
            crate::game::ProgrammatorAction::Move { pid, tx, x, y, dir } => {
                crate::net::session::play::movement::handle_move(
                    state, &tx, pid, 0, x, y, dir, true,
                );
            }
            crate::game::ProgrammatorAction::Dig { pid, tx, dir } => {
                crate::net::session::play::dig_build::handle_dig(state, &tx, pid, dir, true);
            }
            crate::game::ProgrammatorAction::Build {
                pid,
                tx,
                dir,
                block_type,
            } => {
                let bld = crate::protocol::packets::XbldClient {
                    direction: dir,
                    block_type: &block_type,
                };
                crate::net::session::play::dig_build::handle_build(state, &tx, pid, &bld, true);
            }
            crate::game::ProgrammatorAction::Geo { pid, tx } => {
                crate::net::session::play::geo::handle_geo(state, &tx, pid, true);
            }
            crate::game::ProgrammatorAction::Heal { pid, tx } => {
                crate::net::session::ui::heal_inventory::handle_heal(state, &tx, pid, true);
            }
            crate::game::ProgrammatorAction::SetAutoDig { pid, tx, enabled } => {
                crate::net::session::social::misc::handle_auto_dig_set(state, &tx, pid, enabled);
            }
            crate::game::ProgrammatorAction::SetAggression { pid, tx, enabled } => {
                crate::net::session::social::misc::handle_aggression_set(state, &tx, pid, enabled);
            }
            crate::game::ProgrammatorAction::SetHandMode { tx, enabled } => {
                let packet = crate::protocol::packets::hand_mode(enabled);
                let _ = tx.send(crate::net::session::wire::make_u_packet_bytes(
                    packet.0, &packet.1,
                ));
            }
            crate::game::ProgrammatorAction::FillGun { pid, tx, x, y } => {
                crate::net::session::play::packs::handle_gun_fill_prog(state, &tx, pid, x, y);
            }
            crate::game::ProgrammatorAction::SetProgrammatorStatus { tx, running } => {
                let _ = tx.send(crate::net::session::wire::make_u_packet_bytes(
                    "@P",
                    &crate::protocol::packets::programmator_status(running).1,
                ));
            }
        }
    }
    side_profile.programmator_actions = section_t0.elapsed();

    let section_t0 = Instant::now();
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
    let dt_side = side_t0.elapsed();
    let dt_total = tick_t0.elapsed();
    *win_ticks += 1;
    if dt_total > tick_budget {
        *win_over += 1;
    }
    *win_max_total = (*win_max_total).max(dt_total);
    *win_max_dispatch = (*win_max_dispatch).max(dt_dispatch);
    *win_max_schedule = (*win_max_schedule).max(dt_schedule);
    *win_max_side = (*win_max_side).max(dt_side);
    win_max_side_profile.update_max(side_profile);
    *win_max_actions = (*win_max_actions).max(n_actions);

    if dt_total > tick_budget && last_warn.elapsed() >= std::time::Duration::from_millis(500) {
        *last_warn = Instant::now();
        tracing::warn!(
            target: "tickprof",
            "OVER-BUDGET tick: total={dt_total:?} dispatch={dt_dispatch:?} \
             schedule={dt_schedule:?} side={dt_side:?} actions={n_actions} \
             side_broadcasts={:?} side_pack_resends={:?} side_box_persist={:?} \
             side_cell_conversions={:?} side_programmator_actions={:?} \
             side_death={:?} side_bots_render={:?}",
            side_profile.broadcasts,
            side_profile.pack_resends,
            side_profile.box_persist,
            side_profile.cell_conversions,
            side_profile.programmator_actions,
            side_profile.death,
            side_profile.bots_render,
        );
    }

    if win_start.elapsed() >= std::time::Duration::from_secs(5) {
        tracing::info!(
            target: "tickprof",
            "5s summary: ticks={win_ticks} over_budget={win_over} \
             max_total={win_max_total:?} max_dispatch={win_max_dispatch:?} \
             max_schedule={win_max_schedule:?} max_side={win_max_side:?} \
             max_actions={win_max_actions} max_side_broadcasts={:?} \
             max_side_pack_resends={:?} max_side_box_persist={:?} \
             max_side_cell_conversions={:?} max_side_programmator_actions={:?} \
             max_side_death={:?} max_side_bots_render={:?}",
            win_max_side_profile.broadcasts,
            win_max_side_profile.pack_resends,
            win_max_side_profile.box_persist,
            win_max_side_profile.cell_conversions,
            win_max_side_profile.programmator_actions,
            win_max_side_profile.death,
            win_max_side_profile.bots_render,
        );
        *win_start = Instant::now();
        *win_ticks = 0;
        *win_over = 0;
        *win_max_total = std::time::Duration::ZERO;
        *win_max_dispatch = std::time::Duration::ZERO;
        *win_max_schedule = std::time::Duration::ZERO;
        *win_max_side = std::time::Duration::ZERO;
        *win_max_side_profile = SideProfile::default();
        *win_max_actions = 0;
    }
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
            logging: crate::config::LoggingConfig::default(),
            cron: crate::config::CronConfig::default(),
            gameplay: crate::config::GameplayConfig::default(),
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
