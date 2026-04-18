use crate::game::GameState;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::time::{self, Duration, Interval, MissedTickBehavior};
use tracing::info;

/// Описание одной задачи
pub struct CronJob {
    pub name: &'static str,
    pub interval: Duration,
    pub run: Box<dyn Fn(Arc<GameState>) + Send + Sync>,
}

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

        // Реестр задач
        let mut jobs: Vec<CronJob> = Vec::new();

        // 1. Добавляем нашу "заглушку" из конфига
        if config.cron.hourly_log_enabled {
            jobs.push(CronJob {
                name: "HourlyHeartbeat",
                // Для теста можно поставить 10 секунд, но по ТЗ - час.
                // Оставим час (3600с), чтобы не спамить.
                interval: Duration::from_secs(3600),
                run: Box::new(|state| {
                    let online = state.active_players.len();
                    info!("[Cron] Hourly Heartbeat. Online players: {}", online);
                }),
            });
        }

        tokio::spawn(async move {
            info!("Cron system started ({} jobs registered)", jobs.len());

            // Подготавливаем интервалы (в реальной MMORPG их может быть много)
            // Используем MissedTickBehavior::Skip, чтобы если сервер "залагал",
            // задачи не запускались пачкой друг за другом.
            let mut min_interval = Self::create_interval(60);
            let mut hour_interval = Self::create_interval(3600);

            loop {
                tokio::select! {
                    _ = min_interval.tick() => {
                        Self::run_tick(&state, &jobs, Duration::from_secs(60));
                    }
                    _ = hour_interval.tick() => {
                        Self::run_tick(&state, &jobs, Duration::from_secs(3600));
                    }
                    _ = shutdown_rx.recv() => {
                        info!("Cron system shutting down...");
                        break;
                    }
                }
            }
        });
    }

    fn create_interval(secs: u64) -> Interval {
        let mut interval = time::interval(Duration::from_secs(secs));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        interval
    }

    fn run_tick(state: &Arc<GameState>, jobs: &[CronJob], current_interval: Duration) {
        for job in jobs {
            if job.interval == current_interval {
                let state_clone = Arc::clone(state);
                info!("Cron executing job: {}", job.name);

                // В MMORPG инфраструктуре задачи обычно выполняются в пуле потоков
                // или как отдельные таски, чтобы одна тяжелая задача (например, бэкап)
                // не вешала весь цикл планировщика.
                let name = job.name;
                (job.run)(state_clone);
                info!("Cron job finished: {}", name);
            }
        }
    }
}
