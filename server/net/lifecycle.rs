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
                            tracing::error!(player_id = pid_c, error = ?e, "Periodic save failed for player");
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
pub fn spawn_game_tick_loop(state: Arc<GameState>, shutdown: broadcast::Sender<()>) {
    tokio::spawn(async move {
        loop {
            match tokio::spawn(run_game_tick(state.clone(), shutdown.subscribe())).await {
                Err(je) if je.is_panic() => {
                    tracing::error!(
                        target: "tickprof",
                        "GAME TICK PANICKED — рестарт через 200ms (ECS мог остаться mid-mutation)"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                }
                // Чистый shutdown (`Ok`) или отмена задачи (`Err`) — выходим из supervisor.
                _ => break,
            }
        }
    });
}

// Perf-critical 1:1-ref tick loop (C# Step/Update, ServerTime.cs). Тело —
// единый горячий цикл со связанным win_*-инструментарием диагностики фриза;
// механическое дробление ради лимита строк рискует регрессиями фриза
// (см. историю tickprof). Точечный allow в конвенции db/mod.rs / skills.rs.
#[allow(clippy::too_many_lines)]
async fn run_game_tick(state: Arc<GameState>, mut shutdown: broadcast::Receiver<()>) {
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

        // 0. Команды жизненного цикла (Connect/Disconnect) — ДО действий,
        // чтобы entity игрока спавнился раньше своих TY. ВНЕ `ecs.write()`
        // блока ниже: хендлеры берут `ecs` сами (tick — единственный писатель,
        // контеншна нет). Убирает ecs-доступ из conn-тасков (C-4 фриз).
        for cmd in state.drain_life() {
            match cmd {
                crate::game::LifeCmd::Connect { row, tx, token } => {
                    crate::net::session::player::init::connect_in_tick(&state, &tx, &row, token);
                }
                crate::game::LifeCmd::Disconnect { pid, token } => {
                    crate::net::session::player::init::disconnect_in_tick(&state, pid, token);
                }
            }
        }

        // 1. Сначала обрабатываем все входящие пакеты от игроков (Action Queue 1:1 с C#).
        let actions = state.incoming_actions.drain();
        let n_actions = actions.len();
        let d0 = Instant::now();
        for (pid, tx, ty) in actions {
            let state_clone = state.clone();
            let tx_clone = tx.clone();
            let ty_owned = ty.clone();
            let handle = tokio::spawn(async move {
                crate::net::session::dispatch::dispatch_ty_packet(
                    &state_clone,
                    &tx_clone,
                    pid,
                    &ty_owned,
                )
                .await
            });
            match handle.await {
                Ok(res) => {
                    if let Err(e) = res {
                        tracing::error!(
                            player_id = pid,
                            packet = ?ty,
                            error = ?e,
                            "Error processing TY packet"
                        );
                    }
                }
                Err(je) if je.is_panic() => {
                    tracing::error!(
                        player_id = pid,
                        packet = ?ty,
                        "PANIC processing TY packet!"
                    );
                }
                Err(je) => {
                    tracing::error!(
                        player_id = pid,
                        packet = ?ty,
                        "TY packet processing task cancelled or failed: {:?}",
                        je
                    );
                }
            }
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
            box_ops,
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
            let pa = std::mem::take(&mut ecs.resource_mut::<crate::game::ProgrammatorQueue>().0);
            let bp =
                std::mem::take(&mut *ecs.resource_mut::<crate::game::BoxPersistQueue>().0.lock());
            // Отложенные конвертации клеток (StupidAction 1:1 с C#).
            let convs =
                std::mem::take(&mut ecs.resource_mut::<crate::game::PendingCellConversions>().0);
            let pr = std::mem::take(&mut ecs.resource_mut::<crate::game::PackResendQueue>().0);
            drop(ecs);
            drop(schedule);
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

        // Персистенция боксов (BoxPersistQueue уже дренирован внутри ecs.write).
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
            tokio::spawn(async move {
                let _guard = DbTaskGuard { state: state_clone };
                for ((bx, by), op) in box_ops {
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

        // StupidAction 1:1 с C# `World.W.StupidAction(10, x, y, action)` — отложенная конвертация клеток.
        let mut remaining_conversions: Vec<crate::game::PendingConversion> = Vec::new();
        let mut converted_owners: Vec<crate::game::player::PlayerId> = Vec::new();
        for mut conv in cell_conversions {
            if conv.ticks_left > 1 {
                conv.ticks_left -= 1;
                remaining_conversions.push(conv);
            } else if state.world.valid_coord(conv.x, conv.y)
                && state.world.get_cell(conv.x, conv.y) == conv.required_cell
            {
                // ticks_left == 1: выполняем действие, если guard cell совпадает.
                state.world.set_cell(conv.x, conv.y, conv.target_cell);
                state.world.set_durability(conv.x, conv.y, conv.durability);
                crate::game::broadcast_cell_update(&state, conv.x, conv.y);
                converted_owners.push(conv.owner_pid);
            }
        }
        // Возвращаем оставшиеся + начисляем 2-й BuildWar-exp за конвертацию
        // frame→block (1:1 C# Player.Build("V"): AddExp на frame И в колбэке).
        let mut buildwar_pkts: Vec<(crate::game::player::PlayerId, Vec<(String, i32)>)> =
            Vec::new();
        {
            let mut ecs = state.ecs.write();
            ecs.resource_mut::<crate::game::PendingCellConversions>().0 = remaining_conversions;
            for owner in converted_owners {
                let Some(entity) = state.get_player_entity(owner) else {
                    continue;
                };
                if let Some(mut skills) = ecs.get_mut::<crate::game::player::PlayerSkills>(entity)
                    && crate::game::skills::add_skill_exp(
                        &mut skills.states,
                        crate::game::skills::SkillType::BuildWar.code(),
                        1.0,
                    )
                {
                    buildwar_pkts.push((
                        owner,
                        crate::game::skills::skill_progress_payload(&skills.states),
                    ));
                }
            }
        }
        for (owner, payload) in buildwar_pkts {
            if let Some(tx) = state.player_sessions.get(&owner).map(|t| t.clone()) {
                let sk = crate::protocol::packets::skills_packet(&payload);
                let _ = tx.send(crate::net::session::wire::make_u_packet_bytes(sk.0, &sk.1));
            }
        }

        // Отложенные команды программатора.
        for action in prog_actions {
            match action {
                crate::game::ProgrammatorAction::Move { pid, tx, x, y, dir } => {
                    // Тот же handle_move, что и ручной ход (no-DRY): валидация
                    // коллизии/ворот/дистанции — нельзя пройти сквозь блоки.
                    // programmatic=true пропускает guard «программа бежит». Тайминг
                    // (delay per operator) — в programmator_system, отдельный цикл.
                    crate::net::session::play::movement::handle_move(
                        &state, &tx, pid, 0, x, y, dir, true,
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
                        block_type: &block_type,
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
                crate::game::ProgrammatorAction::FillGun { pid, tx, x, y } => {
                    crate::net::session::play::packs::handle_gun_fill_prog(&state, &tx, pid, x, y);
                }
            }
        }
        for (pid, rx, ry, mh, bcast) in pending {
            crate::net::session::play::death::run_death_broadcasts(&state, &bcast, pid);
            let tx = state.query_player_opt(pid, |ecs, entity| {
                ecs.get::<crate::game::player::PlayerConnection>(entity)
                    .map(|c| c.tx.clone())
            });
            if let Some(tx) = tx {
                crate::net::session::play::death::send_respawn_after_death(
                    &tx, pid, rx, ry, mh, &bcast,
                );
                crate::net::session::play::chunks::check_chunk_changed(&state, &tx, pid);
            }
        }

        // Периодический BotsRender (1:1 C# `Player.BotsRender`, каждые 4с):
        // заново шлёт `X` всех видимых ботов каждому игроку. Без этого
        // клиентский `RobotsGarbageCollector` (6с без пинга) удаляет
        // простаивающих ботов — они мигают при ходьбе и исчезают в покое.
        // Таймер per-player в `ActivePlayer`; due-список собираем заранее и
        // отпускаем шард `active_players` ДО рендера (он берёт `ecs.read()`).
        {
            let now_render = Instant::now();
            let mut due: Vec<crate::game::player::PlayerId> = Vec::new();
            for mut e in state.active_players.iter_mut() {
                if now_render.duration_since(e.value().last_bots_render)
                    >= std::time::Duration::from_secs(4)
                {
                    e.value_mut().last_bots_render = now_render;
                    due.push(*e.key());
                }
            }
            for pid in due {
                if let Some(tx) = state.player_sessions.get(&pid).map(|t| t.clone()) {
                    crate::net::session::play::chunks::bots_render(&state, &tx, pid);
                }
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
        if dt_total > TICK_BUDGET && last_warn.elapsed() >= std::time::Duration::from_millis(500) {
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
}
