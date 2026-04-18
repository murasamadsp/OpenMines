//! Фоновые задачи: сброс мира, периодическое сохранение игроков, сохранение при остановке.
//! Отделено от `run()` в `mod.rs`, чтобы тот отвечал только за accept TCP (SRP).

use crate::game::GameState;
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
            let dirty_players: Vec<crate::db::PlayerRow> = state
                .active_players
                .iter_mut()
                .filter(|entry| entry.value().dirty)
                .map(|mut entry| {
                    entry.value_mut().dirty = false;
                    entry.value().data.clone()
                })
                .collect();
            let mut saved = 0usize;
            for player_data in &dirty_players {
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

/// Основной игровой цикл ECS (системы).
pub fn spawn_game_tick_loop(state: Arc<GameState>, mut shutdown: broadcast::Receiver<()>) {
    tokio::spawn(async move {
        // Тик раз в секунду для начала.
        // В будущем можно увеличить частоту (например, 20 TPS -> 50ms).
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = shutdown.recv() => break,
            }
            state.tick();
        }
    });
}
