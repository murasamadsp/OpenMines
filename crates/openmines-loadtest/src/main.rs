//! Нагрузочный замер `OpenMines`: N синтетических клиентов коннектятся по TCP,
//! логинятся (Regular-auth по пред-засеянным игрокам) и двигаются.
//!
//! ЦЕЛЬ — найти реальный потолок ОДНОГО процесса. Замер делает СЕРВЕР через
//! свой tickprof: запускай сервер с `M3R_LOG=warn` (или `RUST_LOG=warn`) и
//! смотри строки `OVER-BUDGET tick` / `SLOW schedule`. Этот тул генерирует
//! трафик и печатает клиентскую статистику (коннекты/логины/ходы/обрывы).
//!
//! АВТОРИЗАЦИЯ: пред-засеваем N игроков прямо в БД сервера (`loadtest_<i>` с
//! детерминированным хэшем), затем каждый бот логинится Regular-токеном
//! `MD5(hash+sid)`. Это НЕ триггерит анти-брутфорс (`AUTH_FAILURE_LIMIT`/IP),
//! поэтому тысячи коннектов с одного `127.0.0.1` проходят. Заход через окно
//! регистрации не годится — он считается auth-failure и банит IP после 6.
//!
//! ВАЖНО: запускать против ЛОКАЛЬНОГО/тестового сервера (пишем в его БД), НЕ
//! прод. Сервер должен быть уже запущен (создаёт схему БД миграцией). На macOS
//! подними лимит дескрипторов: `ulimit -n 65535` (по умолчанию 256).
//!
//! Запуск:
//!   cargo run --release --bin loadtest -- --clients 1000 --port 8090 --secs 60
//!
//! Флаги: --clients N, --host H, --port P, --secs S (длительность движения),
//!        --move-ms M (период `Xmov`), --ramp-ms R (задержка между стартами),
//!        --drain-secs D (ожидание effects перед half-close),
//!        --db PATH (файл БД сервера, по умолчанию data/openmines.db).

mod client;
mod config;
mod protocol;
mod scenario;
mod stats;

use client::run_client;
use config::parse_args;
use scenario::{monitor_loop, seed_players, wait_for_ramp};
use stats::{Stats, print_latency_summary};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::watch;

#[cfg(test)]
use client::drain_frames;
#[cfg(test)]
use config::parse_args_from;
#[cfg(test)]
use protocol::u_frame;
#[cfg(test)]
use stats::percentile;
#[cfg(test)]
use std::collections::VecDeque;
#[cfg(test)]
use std::sync::Mutex;
#[cfg(test)]
use std::time::Instant;
#[cfg(test)]
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    let cfg = parse_args();
    println!(
        "loadtest: {} клиентов → {}:{}, движение {}с (Xmov каждые {}мс), ramp {}мс/клиент, drain {}с",
        cfg.clients, cfg.host, cfg.port, cfg.secs, cfg.move_ms, cfg.ramp_ms, cfg.drain_secs
    );

    // Пред-засев игроков в БД сервера; (user_id, hash) для Regular-auth.
    print!("  пред-засев {} игроков в {} ... ", cfg.clients, cfg.db);
    let creds = match seed_players(&cfg.db, cfg.clients).await {
        Ok(c) => {
            println!("ok ({} строк)", c.len());
            c
        }
        Err(e) => {
            println!("ОШИБКА: {e}");
            eprintln!("Запущен ли сервер и верен ли --db? (схему создаёт сервер миграцией)");
            return;
        }
    };
    println!("  → смотри tickprof СЕРВЕРА (M3R_LOG=warn): 'OVER-BUDGET tick' / 'SLOW schedule'");

    let stats = Arc::new(Stats::default());
    let cfg = Arc::new(cfg);
    let creds = Arc::new(creds);

    let monitor = tokio::spawn(monitor_loop(stats.clone(), cfg.clone()));
    let (start_tx, start_rx) = watch::channel(false);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let mut tasks = Vec::with_capacity(cfg.clients as usize);
    for i in 0..cfg.clients {
        let (id, hash) = creds[i as usize].clone();
        tasks.push(tokio::spawn(run_client(
            cfg.clone(),
            stats.clone(),
            id,
            hash,
            start_rx.clone(),
            shutdown_rx.clone(),
        )));
        if cfg.ramp_ms > 0 {
            tokio::time::sleep(Duration::from_millis(cfg.ramp_ms)).await;
        }
    }

    wait_for_ramp(&stats, cfg.clients, Duration::from_secs(30)).await;
    stats.phase.store(1, Ordering::Relaxed);
    let _ = start_tx.send(true);
    tokio::time::sleep(Duration::from_secs(cfg.secs)).await;
    stats.phase.store(2, Ordering::Relaxed);
    let _ = shutdown_tx.send(true);

    monitor.abort();
    let _ = monitor.await;

    let join_deadline =
        tokio::time::Instant::now() + Duration::from_secs(cfg.drain_secs.saturating_add(25));
    let mut latencies_us = Vec::new();
    for task in &mut tasks {
        match tokio::time::timeout_at(join_deadline, task).await {
            Ok(Ok(report)) => latencies_us.extend(report.latencies_us),
            Ok(Err(_)) => {
                stats.unexpected_disconnects.fetch_add(1, Ordering::Relaxed);
            }
            Err(_) => break,
        }
    }
    for task in tasks {
        if !task.is_finished() {
            task.abort();
        }
    }
    latencies_us.sort_unstable();

    println!(
        "\n=== ИТОГ ===\n  коннектов: {}\n  логинов: {}\n  ходов отправлено: {}\n  effects получено: {}\n  graceful disconnect: {}\n  неожиданных обрывов: {}\n  drain timeout: {}\n  ошибок коннекта: {}",
        stats.connected.load(Ordering::Relaxed),
        stats.logged_in.load(Ordering::Relaxed),
        stats.moves_sent.load(Ordering::Relaxed),
        stats.effects_received.load(Ordering::Relaxed),
        stats.graceful_disconnects.load(Ordering::Relaxed),
        stats.unexpected_disconnects.load(Ordering::Relaxed),
        stats.drain_timeouts.load(Ordering::Relaxed),
        stats.connect_errors.load(Ordering::Relaxed),
    );
    print_latency_summary(&latencies_us);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_args_defaults() {
        let args: [&str; 0] = [];
        let cfg = parse_args_from(args).unwrap();
        assert_eq!(cfg.host, "127.0.0.1");
        assert_eq!(cfg.port, 8090);
        assert_eq!(cfg.clients, 500);
        assert_eq!(cfg.secs, 30);
        assert_eq!(cfg.move_ms, 200);
        assert_eq!(cfg.ramp_ms, 3);
        assert_eq!(cfg.drain_secs, 5);
        assert_eq!(cfg.db, "data/openmines.db");
    }

    #[test]
    fn test_parse_args_custom() {
        let args = [
            "--host",
            "10.0.0.2",
            "--port",
            "9999",
            "--clients",
            "123",
            "--secs",
            "45",
            "--move-ms",
            "150",
            "--ramp-ms",
            "12",
            "--drain-secs",
            "7",
            "--db",
            "custom.db",
        ];
        let cfg = parse_args_from(args).unwrap();
        assert_eq!(cfg.host, "10.0.0.2");
        assert_eq!(cfg.port, 9999);
        assert_eq!(cfg.clients, 123);
        assert_eq!(cfg.secs, 45);
        assert_eq!(cfg.move_ms, 150);
        assert_eq!(cfg.ramp_ms, 12);
        assert_eq!(cfg.drain_secs, 7);
        assert_eq!(cfg.db, "custom.db");
    }

    #[test]
    fn test_parse_args_invalid() {
        let args = ["--port", "abc"];
        let res = parse_args_from(args);
        assert!(res.is_err());
    }

    #[test]
    fn test_parse_args_missing_value() {
        let args = ["--port"];
        let res = parse_args_from(args);
        assert!(res.is_err());
    }

    #[test]
    fn test_parse_args_duplicate_flags() {
        let args = ["--clients", "100", "--clients", "200"];
        let res = parse_args_from(args);
        assert!(res.is_err());
    }

    #[test]
    fn percentile_uses_nearest_rank() {
        let values: Vec<_> = (1..=1_000).collect();

        assert_eq!(percentile(&values, 50, 100), Some(500));
        assert_eq!(percentile(&values, 99, 100), Some(990));
        assert_eq!(percentile(&values, 999, 1_000), Some(999));
        assert_eq!(percentile(&[], 50, 100), None);
    }

    #[test]
    fn movement_effect_consumes_one_pending_timestamp() {
        let mut buf = u_frame(*b"@T", "10:10");
        let (out_tx, _out_rx) = mpsc::unbounded_channel();
        let mut sid_tx = None;
        let mut ready_tx = None;
        let pending = Mutex::new(VecDeque::from([Instant::now()]));
        let mut latencies = Vec::new();

        let effects = drain_frames(
            &mut buf,
            &out_tx,
            &mut sid_tx,
            &mut ready_tx,
            &pending,
            &mut latencies,
        );

        assert_eq!(effects, 1);
        assert!(pending.lock().unwrap().is_empty());
        assert_eq!(latencies.len(), 1);
    }
}
