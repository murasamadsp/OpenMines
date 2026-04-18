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
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = shutdown.recv() => break,
            }
            
            let mut dirty_rows = Vec::new();
            for entry in &state.active_players {
                let pid = *entry.key();
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
            // Execute systems
            let mut ecs = state.ecs.write();
            let mut schedule = state.schedule.write();
            schedule.run(&mut ecs);
        }
    });
}
