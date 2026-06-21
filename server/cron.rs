use crate::game::GameState;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::{error, info};

pub struct CronManager {
    game_state: Arc<GameState>,
    shutdown: broadcast::Sender<()>,
}

impl CronManager {
    pub const fn new(game_state: Arc<GameState>, shutdown: broadcast::Sender<()>) -> Self {
        Self {
            game_state,
            shutdown,
        }
    }

    /// Запуск движка планировщика
    pub fn spawn(self) {
        let state = self.game_state;
        let mut shutdown_rx = self.shutdown.subscribe();
        let config = state.config.clone();

        tokio::spawn(async move {
            let mut sched = match JobScheduler::new().await {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to create JobScheduler: {e}");
                    return;
                }
            };

            // Добавляем ежечасный лог игроков
            if config.cron.hourly_log_enabled {
                let state_clone = Arc::clone(&state);
                // "0 0 * * * *" - выполняется каждую минуту 00, секунду 00 каждого часа.
                let job = Job::new_async("0 0 * * * *", move |_uuid, _lock| {
                    let st = Arc::clone(&state_clone);
                    Box::pin(async move {
                        let online = st.active_players.len();
                        info!("[Cron] Hourly Heartbeat. Online players: {}", online);
                    })
                });

                match job {
                    Ok(j) => {
                        if let Err(e) = sched.add(j).await {
                            error!("Failed to add HourlyHeartbeat job to scheduler: {e}");
                        } else {
                            info!("HourlyHeartbeat job registered in cron");
                        }
                    }
                    Err(e) => {
                        error!("Failed to create HourlyHeartbeat job: {e}");
                    }
                }
            }

            if let Err(e) = sched.start().await {
                error!("Failed to start JobScheduler: {e}");
                return;
            }
            info!("Cron system started (tokio-cron-scheduler)");

            // Ожидаем сигнала выключения
            let _ = shutdown_rx.recv().await;
            info!("Cron system shutting down...");
            if let Err(e) = sched.shutdown().await {
                error!("Error shutting down cron scheduler: {e}");
            }
        });
    }
}
