pub mod auction;
pub mod cron;
pub mod lifecycle;

use crate::game::GameState;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Запуск всех фоновых задач, воркеров периодического сохранения и планировщика.
pub fn spawn_background_tasks(state: &Arc<GameState>, shutdown: &broadcast::Sender<()>) {
    // 1. Запуск планировщика задач (cron)
    cron::CronManager::new(Arc::clone(state), shutdown.clone()).spawn();

    // 2. Воркеры периодического сохранения и тиков
    lifecycle::spawn_world_flush_loop(Arc::clone(state), shutdown.subscribe());
    lifecycle::spawn_online_count_loop(Arc::clone(state), shutdown.subscribe());
    lifecycle::spawn_player_dirty_flush_loop(Arc::clone(state), shutdown.subscribe());
    lifecycle::spawn_building_dirty_flush_loop(Arc::clone(state), shutdown.subscribe());
    lifecycle::spawn_game_tick_loop(Arc::clone(state), shutdown.clone());

    // 3. Обработка завершения аукционов
    auction::spawn_auction_finalize_loop(Arc::clone(state), shutdown.subscribe());
}
