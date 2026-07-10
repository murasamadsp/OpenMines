pub mod auction;
pub mod cron;
pub mod lifecycle;

use crate::game::GameState;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Запуск всех фоновых задач, воркеров периодического сохранения и планировщика.
pub struct BackgroundTasks {
    game_tick: std::thread::JoinHandle<()>,
    player_dirty_flush: tokio::task::JoinHandle<()>,
    building_dirty_flush: tokio::task::JoinHandle<()>,
    persistence: crate::persistence::PersistenceRuntime,
}

impl BackgroundTasks {
    pub async fn shutdown(self) {
        let Self {
            game_tick,
            player_dirty_flush,
            building_dirty_flush,
            persistence,
        } = self;
        match tokio::task::spawn_blocking(move || game_tick.join()).await {
            Ok(Ok(())) => {}
            Ok(Err(panic)) => std::panic::resume_unwind(panic),
            Err(error) => panic!("failed to join game tick thread: {error}"),
        }
        if let Err(error) = player_dirty_flush.await {
            panic!("player dirty flush task failed: {error}");
        }
        if let Err(error) = building_dirty_flush.await {
            panic!("building dirty flush task failed: {error}");
        }
        persistence.shutdown().await;
    }
}

pub fn spawn_background_tasks(
    state: &Arc<GameState>,
    shutdown: &broadcast::Sender<()>,
) -> BackgroundTasks {
    // 1. Запуск планировщика задач (cron)
    cron::CronManager::new(Arc::clone(state), shutdown.clone()).spawn();

    // 2. Воркеры периодического сохранения и тиков
    lifecycle::spawn_world_flush_loop(Arc::clone(state), shutdown.subscribe());
    lifecycle::spawn_online_count_loop(Arc::clone(state), shutdown.subscribe());
    let persistence = crate::persistence::PersistenceRuntime::start(state.db.clone());
    let player_dirty_flush = lifecycle::spawn_player_dirty_flush_loop(
        Arc::clone(state),
        shutdown.subscribe(),
        persistence.handle(),
    );
    let building_dirty_flush = lifecycle::spawn_building_dirty_flush_loop(
        Arc::clone(state),
        shutdown.subscribe(),
        persistence.handle(),
    );
    let game_tick =
        lifecycle::spawn_game_tick_loop(Arc::clone(state), shutdown, persistence.handle());

    // 3. Обработка завершения аукционов
    auction::spawn_auction_finalize_loop(Arc::clone(state), shutdown.subscribe());

    BackgroundTasks {
        game_tick,
        player_dirty_flush,
        building_dirty_flush,
        persistence,
    }
}
