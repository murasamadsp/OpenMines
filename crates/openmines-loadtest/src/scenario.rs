use crate::config::Config;
use crate::stats::Stats;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

/// Seed deterministic load-test players and return `(id, hash)` credentials.
pub async fn seed_players(
    database_path: &str,
    count: u32,
) -> Result<Vec<(i64, String)>, anyhow::Error> {
    use sqlx::Row;

    let database = openmines_storage::Database::open(database_path).await?;
    let mut credentials = Vec::with_capacity(count as usize);
    let mut transaction = database.pool.begin().await?;
    for index in 0..count {
        let name = format!("loadtest_{index}");
        let hash = format!("lthash_{index}");
        let row = sqlx::query(
            "INSERT INTO players (name, hash) VALUES (?1, ?2) \
             ON CONFLICT(name) DO UPDATE SET hash = excluded.hash RETURNING id",
        )
        .bind(&name)
        .bind(&hash)
        .fetch_one(&mut *transaction)
        .await?;
        credentials.push((row.get::<i64, _>(0), hash));
    }
    transaction.commit().await?;
    Ok(credentials)
}

pub async fn monitor_loop(stats: Arc<Stats>, config: Arc<Config>) {
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut last_moves = 0_u64;
    let mut last_effects = 0_u64;
    loop {
        tick.tick().await;
        let moves = stats.moves_sent.load(Ordering::Relaxed);
        let effects = stats.effects_received.load(Ordering::Relaxed);
        let finished = stats
            .graceful_disconnects
            .load(Ordering::Relaxed)
            .saturating_add(stats.unexpected_disconnects.load(Ordering::Relaxed));
        let phase = match stats.phase.load(Ordering::Relaxed) {
            0 => "ramp",
            1 => "steady",
            _ => "drain",
        };
        println!(
            "  [{phase}] онлайн={}/{} логинов={} ходов/с={} effects/с={} неожиданных={} ошибок={}",
            stats
                .connected
                .load(Ordering::Relaxed)
                .saturating_sub(finished),
            config.clients,
            stats.logged_in.load(Ordering::Relaxed),
            moves.saturating_sub(last_moves),
            effects.saturating_sub(last_effects),
            stats.unexpected_disconnects.load(Ordering::Relaxed),
            stats.connect_errors.load(Ordering::Relaxed),
        );
        last_moves = moves;
        last_effects = effects;
    }
}

pub async fn wait_for_ramp(stats: &Stats, clients: u32, timeout: Duration) {
    let deadline = tokio::time::Instant::now() + timeout;
    let expected = u64::from(clients);
    loop {
        let finished = stats
            .logged_in
            .load(Ordering::Relaxed)
            .saturating_add(stats.connect_errors.load(Ordering::Relaxed))
            .saturating_add(stats.unexpected_disconnects.load(Ordering::Relaxed));
        if finished >= expected || tokio::time::Instant::now() >= deadline {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
