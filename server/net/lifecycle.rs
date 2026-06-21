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
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
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
                    tracing::error!("World flush error: {e}");
                }
            })
            .await;
            tracing::warn!(target: "tickprof", "WORLD FLUSH end took {:?}", t0.elapsed());
            crate::metrics::WORLD_FLUSH_TOTAL.inc();
            crate::metrics::WORLD_FLUSH_SECONDS.observe(t0.elapsed().as_secs_f64());
        }
    });
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
            let pids: Vec<crate::game::PlayerId> =
                state.active_players.iter().map(|e| *e.key()).collect();

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
                tracing::info!(
                    "Periodic save: {} dirty players, inv sizes: {:?}",
                    dirty_rows.len(),
                    dirty_rows
                        .iter()
                        .map(|(pid, r)| (*pid, r.inventory.len()))
                        .collect::<Vec<_>>()
                );
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
                            tracing::error!("Periodic save failed for player {}: {e}", pid_c);
                        }
                    }
                });
                saved += 1;
            }
            if saved > 0 {
                tracing::debug!("Periodic save: flushed {saved} players");
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
                let row = state.modify_building(entity, |ecs, ent| {
                    let mut flags = ecs.get_mut::<crate::game::BuildingFlags>(ent)?;
                    if flags.dirty {
                        flags.dirty = false;
                        crate::game::buildings::extract_building_row(ecs, ent)
                    } else {
                        None
                    }
                });

                if let Some(r) = row {
                    let db = state.db.clone();
                    let res = db.save_building(&r).await;
                    match res {
                        Ok(()) => saved += 1,
                        Err(e) => tracing::error!("Periodic save failed for building: {e}"),
                    }
                }
            }
            if saved > 0 {
                tracing::debug!("Periodic save: flushed {saved} buildings");
            }
        }
    });
}

// Perf-critical 1:1-ref tick loop (C# Step/Update, ServerTime.cs). Тело —
// единый горячий цикл со связанным win_*-инструментарием диагностики фриза;
// механическое дробление ради лимита строк рискует регрессиями фриза
// (см. историю tickprof). Точечный allow в конвенции db/mod.rs / skills.rs.
#[allow(clippy::too_many_lines)]
pub fn spawn_game_tick_loop(state: Arc<GameState>, mut shutdown: broadcast::Receiver<()>) {
    tokio::spawn(async move {
        // ── Stage 0 instrumentation (self-throttled; диагностика фриза, target=tickprof) ──
        const TICK_BUDGET: std::time::Duration = std::time::Duration::from_millis(10);

        // Game tick loop: systems + queue draining.
        // 1:1 ref: C# Step/Update loops (ServerTime.cs) run with 1ms or 10ms sleeps.
        // 10ms (100Hz) is the standard for player actions and systems in the legacy server.
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(10));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut win_start = Instant::now();
        let mut win_ticks: u64 = 0;
        let mut win_over: u64 = 0;
        let mut win_max_total = std::time::Duration::ZERO;
        let mut win_max_dispatch = std::time::Duration::ZERO;
        let mut win_max_schedule = std::time::Duration::ZERO;
        let mut win_max_side = std::time::Duration::ZERO;
        let mut win_max_actions: usize = 0;
        let mut last_warn = Instant::now()
            .checked_sub(std::time::Duration::from_secs(1))
            .unwrap_or_else(Instant::now);

        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = shutdown.recv() => break,
            }
            let tick_t0 = Instant::now();

            // 1. Сначала обрабатываем все входящие пакеты от игроков (Action Queue 1:1 с C#).
            let actions = state.incoming_actions.drain();
            let n_actions = actions.len();
            let d0 = Instant::now();
            for (pid, tx, ty) in actions {
                let _ =
                    crate::net::session::dispatch::dispatch_ty_packet(&state, &tx, pid, &ty).await;
            }
            let dt_dispatch = d0.elapsed();

            // 2. ECS + очереди side-effects.
            // Системы НЕ ре-лочат `ecs` — вместо этого пушат в BroadcastQueue/ProgrammatorQueue.
            // Обрабатываем очереди ПОСЛЕ `schedule.run()`, когда `ecs.write()` уже отпущен.
            let sched_t0 = Instant::now();
            let (
                pending,
                broadcasts,
                prog_actions,
                cell_conversions,
                pack_resends,
                sched_lock_wait,
                sched_run,
            ) = {
                let mut ecs = state.ecs.write();
                let mut schedule = state.schedule.write();
                let lw = sched_t0.elapsed();
                let run_t0 = Instant::now();
                schedule.run(&mut ecs);
                let rn = run_t0.elapsed();

                let p = crate::net::session::play::death::flush_player_death_queue_after_tick(
                    &state, &mut ecs,
                );
                let bc = std::mem::take(&mut ecs.resource_mut::<crate::game::BroadcastQueue>().0);
                let pa =
                    std::mem::take(&mut ecs.resource_mut::<crate::game::ProgrammatorQueue>().0);
                // Отложенные конвертации клеток (StupidAction 1:1 с C#).
                let convs = std::mem::take(
                    &mut ecs.resource_mut::<crate::game::PendingCellConversions>().0,
                );
                let pr = std::mem::take(&mut ecs.resource_mut::<crate::game::PackResendQueue>().0);
                drop(ecs);
                drop(schedule);
                (p, bc, pa, convs, pr, lw, rn)
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

            // Отложенные broadcast'ы из ECS-систем (sand, combat).
            for effect in broadcasts {
                match effect {
                    crate::game::BroadcastEffect::CellUpdate(x, y) => {
                        crate::game::broadcast_cell_update(&state, x, y);
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

            // Перерасылка HB O для зданий у которых обнулился charge (C# `ResendPack`).
            for (px, py) in pack_resends {
                if let Some(view) = state.get_pack_at(px, py) {
                    crate::net::session::social::buildings::broadcast_pack_update(&state, &view);
                }
            }

            // Персистенция боксов ВНЕ `ecs.write()` (фикс C-1/C-2/H-1: SQLite
            // по боксам больше не на hot-path). DB-запись — в spawn_blocking.
            let box_ops = state.drain_box_persist();
            if !box_ops.is_empty() {
                let db = state.db.clone();
                tokio::spawn(async move {
                    for ((bx, by), op) in box_ops {
                        let r = match op {
                            None => db.delete_box_at(bx, by).await,
                            Some(crystals) => db.upsert_box(bx, by, &crystals).await,
                        };
                        if let Err(e) = r {
                            tracing::error!("box persist ({bx},{by}) failed: {e}");
                        }
                    }
                });
            }

            // StupidAction 1:1 с C# `World.W.StupidAction(10, x, y, action)` — отложенная конвертация клеток.
            let mut remaining_conversions: Vec<crate::game::PendingConversion> = Vec::new();
            for mut conv in cell_conversions {
                if conv.ticks_left > 1 {
                    conv.ticks_left -= 1;
                    remaining_conversions.push(conv);
                } else {
                    // ticks_left == 1: выполняем действие, если guard cell совпадает.
                    if state.world.valid_coord(conv.x, conv.y)
                        && state.world.get_cell(conv.x, conv.y) == conv.required_cell
                    {
                        state.world.set_cell(conv.x, conv.y, conv.target_cell);
                        state.world.set_durability(conv.x, conv.y, conv.durability);
                        crate::game::broadcast_cell_update(&state, conv.x, conv.y);
                    }
                }
            }
            // Возвращаем оставшиеся конверсии обратно в ECS Resource.
            {
                let mut ecs = state.ecs.write();
                ecs.resource_mut::<crate::game::PendingCellConversions>().0 = remaining_conversions;
            }

            // Отложенные команды программатора.
            for action in prog_actions {
                match action {
                    crate::game::ProgrammatorAction::Move { pid, tx, x, y, dir } => {
                        crate::net::session::play::movement::handle_move_pure(
                            &state, &tx, pid, x, y, dir,
                        );
                    }
                    crate::game::ProgrammatorAction::Dig { pid, tx, dir } => {
                        crate::net::session::play::dig_build::handle_dig(&state, &tx, pid, dir);
                    }
                    crate::game::ProgrammatorAction::Build {
                        pid,
                        tx,
                        dir,
                        block_type,
                    } => {
                        let bld = crate::protocol::packets::XbldClient {
                            direction: dir,
                            block_type,
                        };
                        crate::net::session::play::dig_build::handle_build(&state, &tx, pid, &bld);
                    }
                    crate::game::ProgrammatorAction::Geo { pid, tx } => {
                        crate::net::session::play::geo::handle_geo(&state, &tx, pid);
                    }
                    crate::game::ProgrammatorAction::Heal { pid, tx } => {
                        crate::net::session::ui::heal_inventory::handle_heal(&state, &tx, pid);
                    }
                    crate::game::ProgrammatorAction::SetAutoDig { pid, tx, enabled } => {
                        crate::net::session::social::misc::handle_auto_dig_set(
                            &state, &tx, pid, enabled,
                        );
                    }
                }
            }
            for (pid, rx, ry, mh, bcast) in pending {
                crate::net::session::play::death::run_death_broadcasts(&state, &bcast, pid);
                let tx = state
                    .query_player(pid, |ecs, entity| {
                        ecs.get::<crate::game::player::PlayerConnection>(entity)
                            .map(|c| c.tx.clone())
                    })
                    .flatten();
                if let Some(tx) = tx {
                    crate::net::session::play::death::send_respawn_after_death(
                        &tx, pid, rx, ry, mh, &bcast,
                    );
                    crate::net::session::play::chunks::check_chunk_changed(&state, &tx, pid);
                }
            }

            // ── Stage 0: агрегация и throttled-вывод (target=tickprof) ──
            let dt_side = side_t0.elapsed();
            let dt_total = tick_t0.elapsed();
            win_ticks += 1;
            if dt_total > TICK_BUDGET {
                win_over += 1;
            }
            win_max_total = win_max_total.max(dt_total);
            win_max_dispatch = win_max_dispatch.max(dt_dispatch);
            win_max_schedule = win_max_schedule.max(dt_schedule);
            win_max_side = win_max_side.max(dt_side);
            win_max_actions = win_max_actions.max(n_actions);

            // Индивидуальный over-budget тик — не чаще 1 раза/500мс (сам не флудит).
            if dt_total > TICK_BUDGET
                && last_warn.elapsed() >= std::time::Duration::from_millis(500)
            {
                last_warn = Instant::now();
                tracing::warn!(
                    target: "tickprof",
                    "OVER-BUDGET tick: total={dt_total:?} dispatch={dt_dispatch:?} \
                     schedule={dt_schedule:?} side={dt_side:?} actions={n_actions}"
                );
            }
            // Сводка раз в ~5с.
            if win_start.elapsed() >= std::time::Duration::from_secs(5) {
                tracing::info!(
                    target: "tickprof",
                    "5s summary: ticks={win_ticks} over_budget={win_over} \
                     max_total={win_max_total:?} max_dispatch={win_max_dispatch:?} \
                     max_schedule={win_max_schedule:?} max_side={win_max_side:?} \
                     max_actions={win_max_actions}"
                );
                win_start = Instant::now();
                win_ticks = 0;
                win_over = 0;
                win_max_total = std::time::Duration::ZERO;
                win_max_dispatch = std::time::Duration::ZERO;
                win_max_schedule = std::time::Duration::ZERO;
                win_max_side = std::time::Duration::ZERO;
                win_max_actions = 0;
            }
        }
    });
}
