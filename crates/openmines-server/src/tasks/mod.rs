pub mod auction;
pub mod cron;
pub mod lifecycle;
pub mod simulation;

use crate::game::GameState;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Запуск всех фоновых задач, воркеров периодического сохранения и планировщика.
pub struct BackgroundTasks {
    game_tick: std::thread::JoinHandle<()>,
    persistence: crate::persistence::PersistenceRuntime,
}

impl BackgroundTasks {
    pub async fn shutdown(self) {
        let Self {
            game_tick,
            persistence,
        } = self;
        match tokio::task::spawn_blocking(move || game_tick.join()).await {
            Ok(Ok(())) => {}
            Ok(Err(panic)) => std::panic::resume_unwind(panic),
            Err(error) => panic!("failed to join game tick thread: {error}"),
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
    let mut persistence = crate::persistence::PersistenceRuntime::start(state.db.clone());
    let persistence_completions = persistence.take_completion_receiver();
    let game_tick = simulation::spawn_game_tick_loop(
        Arc::clone(state),
        shutdown,
        persistence.handle(),
        persistence_completions,
    );

    // 3. Обработка завершения аукционов
    auction::spawn_auction_finalize_loop(Arc::clone(state), shutdown.subscribe());

    BackgroundTasks {
        game_tick,
        persistence,
    }
}
