//! Фоновые задачи: сброс мира, периодическое сохранение игроков, сохранение при остановке.
//! Отделено от `run()` в `mod.rs`, чтобы тот отвечал только за accept TCP (SRP).

use crate::game::GameState;
use crate::world::WorldProvider;
use bevy_ecs::prelude::Entity;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Периодический flush mmap-слоёв мира.
pub fn spawn_world_flush_loop(state: Arc<GameState>, mut shutdown: broadcast::Receiver<()>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = shutdown.recv() => break,
            }
            let t0 = std::time::Instant::now();
            if let Err(e) = state.world.flush() {
                tracing::error!("World flush error: {e}");
            }
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

            let mut dirty_rows = Vec::new();
            for pid in pids {
                let row = state.modify_player(pid, |ecs, entity| {
                    let mut flags = ecs.get_mut::<crate::game::PlayerFlags>(entity)?;
                    if flags.dirty {
                        flags.dirty = false;
                        crate::game::player::extract_player_row(ecs, entity)
                    } else { None }
                }).flatten();
                if let Some(r) = row { dirty_rows.push(r); }
            }

            let mut saved = 0usize;
            for player_data in &dirty_rows {
                if let Err(e) = state.db.save_player(player_data) {
                    tracing::error!("Periodic save failed for player {}: {e}", player_data.id);
                } else {
                    saved += 1;
                    crate::metrics::PLAYER_SAVE_TOTAL.inc();
                }
            }
            if saved > 0 {
                tracing::debug!("Periodic save: flushed {saved} players");
            }
        }
    });
}

/// Сохранение «грязных» зданий в БД.
pub fn spawn_building_dirty_flush_loop(state: Arc<GameState>, mut shutdown: broadcast::Receiver<()>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(45));
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
                    if flags.dirty { dirty_entities.push(entity); }
                }
            }

            let mut saved = 0usize;
            for entity in dirty_entities {
                let row = state.modify_building(entity, |ecs, ent| {
                    let mut flags = ecs.get_mut::<crate::game::BuildingFlags>(ent)?;
                    if flags.dirty {
                        flags.dirty = false;
                        crate::game::buildings::extract_building_row(ecs, ent)
                    } else { None }
                }).flatten();

                if let Some(r) = row {
                    if let Err(e) = state.db.save_building(&r) {
                        tracing::error!("Periodic save failed for building {}: {e}", r.id);
                    } else {
                        saved += 1;
                    }
                }
            }
            if saved > 0 {
                tracing::debug!("Periodic save: flushed {saved} buildings");
            }
        }
    });
}

/// Основной игровой цикл ECS (системы).
pub fn spawn_game_tick_loop(state: Arc<GameState>, mut shutdown: broadcast::Receiver<()>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = shutdown.recv() => break,
            }
            // ECS + очереди side-effects.
            // Системы НЕ ре-лочат `ecs` — вместо этого пушат в BroadcastQueue/ProgrammatorQueue.
            // Обрабатываем очереди ПОСЛЕ `schedule.run()`, когда `ecs.write()` уже отпущен.
            let (pending, broadcasts, prog_actions) = {
                let mut ecs = state.ecs.write();
                let mut schedule = state.schedule.write();
                schedule.run(&mut ecs);
                let p = crate::net::session::social::misc::flush_player_death_queue_after_tick(&state, &mut ecs);
                let bc = std::mem::take(&mut ecs.resource_mut::<crate::game::BroadcastQueue>().0);
                let pa = std::mem::take(&mut ecs.resource_mut::<crate::game::ProgrammatorQueue>().0);
                drop(schedule);
                (p, bc, pa)
            };

            // Отложенные broadcast'ы из ECS-систем (sand, combat).
            for effect in broadcasts {
                match effect {
                    crate::game::BroadcastEffect::CellUpdate(x, y) => {
                        crate::game::broadcast_cell_update(&state, x, y);
                    }
                    crate::game::BroadcastEffect::Nearby { cx, cy, data, exclude } => {
                        state.broadcast_to_nearby(cx, cy, &data, exclude);
                    }
                }
            }

            // Отложенные команды программатора.
            for action in prog_actions {
                match action {
                    crate::game::ProgrammatorAction::Move { pid, tx, x, y, dir } => {
                        crate::net::session::play::movement::handle_move(&state, &tx, pid, 0, x, y, dir);
                    }
                    crate::game::ProgrammatorAction::Dig { pid, tx, dir } => {
                        crate::net::session::play::dig_build::handle_dig(&state, &tx, pid, dir);
                    }
                }
            }

            for (pid, rx, ry, mh, bcast) in pending {
                crate::net::session::social::misc::run_death_broadcasts(&state, &bcast);
                let tx = state.query_player(pid, |ecs, entity| {
                    ecs.get::<crate::game::player::PlayerConnection>(entity).map(|c| c.tx.clone())
                }).flatten();
                if let Some(tx) = tx {
                    crate::net::session::social::misc::send_respawn_after_death(&tx, pid, rx, ry, mh);
                    crate::net::session::play::chunks::check_chunk_changed(&state, &tx, pid);
                }
            }
        }
    });
}
