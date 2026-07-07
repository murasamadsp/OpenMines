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
                    error!(error = ?e, "Failed to create JobScheduler");
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
                        let online = st.online_count();
                        info!(online_players = online, "[Cron] Hourly Heartbeat");
                    })
                });

                match job {
                    Ok(j) => {
                        if let Err(e) = sched.add(j).await {
                            error!(error = ?e, "Failed to add HourlyHeartbeat job to scheduler");
                        } else {
                            info!("HourlyHeartbeat job registered in cron");
                        }
                    }
                    Err(e) => {
                        error!(error = ?e, "Failed to create HourlyHeartbeat job");
                    }
                }
            }

            if let Err(e) = sched.start().await {
                error!(error = ?e, "Failed to start JobScheduler");
                return;
            }
            info!("Cron system started (tokio-cron-scheduler)");

            // Ожидаем сигнала выключения
            let _ = shutdown_rx.recv().await;
            info!("Cron system shutting down...");
            if let Err(e) = sched.shutdown().await {
                error!(error = ?e, "Error shutting down cron scheduler");
            }
        });
    }
}
