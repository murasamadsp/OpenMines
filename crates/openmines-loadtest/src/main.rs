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

use bytes::BytesMut;
use openmines_protocol::{Packet, b_packet, u_packet};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, watch};

#[derive(Clone)]
struct Config {
    host: String,
    port: u16,
    clients: u32,
    secs: u64,
    move_ms: u64,
    ramp_ms: u64,
    drain_secs: u64,
    db: String,
}

#[derive(Default)]
struct Stats {
    phase: AtomicU64,
    connected: AtomicU64,
    logged_in: AtomicU64,
    moves_sent: AtomicU64,
    effects_received: AtomicU64,
    graceful_disconnects: AtomicU64,
    unexpected_disconnects: AtomicU64,
    drain_timeouts: AtomicU64,
    connect_errors: AtomicU64,
}

#[derive(Default)]
struct ClientReport {
    latencies_us: Vec<u64>,
}

#[derive(Default)]
struct ReaderReport {
    latencies_us: Vec<u64>,
}

fn u_frame(event: [u8; 2], s: &str) -> Vec<u8> {
    let event = std::str::from_utf8(&event).expect("loadtest event must be ASCII");
    encode_packet(&u_packet(event, s.as_bytes()))
}

/// TY-пакет: внешний `B`/"TY", payload = `[4B ev][u32 time][u32 x][u32 y][sub]` (LE).
fn ty_frame(inner_event: [u8; 4], time: u32, x: u32, y: u32, sub: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(16 + sub.len());
    p.extend_from_slice(&inner_event);
    p.extend_from_slice(&time.to_le_bytes());
    p.extend_from_slice(&x.to_le_bytes());
    p.extend_from_slice(&y.to_le_bytes());
    p.extend_from_slice(sub);
    encode_packet(&b_packet("TY", &p))
}

fn encode_packet(packet: &Packet) -> Vec<u8> {
    let mut buf = BytesMut::with_capacity(packet.wire_len());
    packet
        .encode(&mut buf)
        .expect("loadtest packet must fit wire frame");
    buf.to_vec()
}

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

/// Вставить N игроков `loadtest_<i>` (детерминированный хэш) в БД сервера.
/// Идемпотентно (`ON CONFLICT` обновляет хэш). Возвращает `(id, hash)` по индексу.
async fn seed_players(db_path: &str, n: u32) -> Result<Vec<(i64, String)>, anyhow::Error> {
    use sqlx::Row;
    let database = openmines_storage::Database::open(db_path).await?;
    let mut out = Vec::with_capacity(n as usize);
    let mut tx = database.pool.begin().await?;
    for i in 0..n {
        let name = format!("loadtest_{i}");
        let hash = format!("lthash_{i}");
        let row = sqlx::query(
            "INSERT INTO players (name, hash) VALUES (?1, ?2) \
             ON CONFLICT(name) DO UPDATE SET hash = excluded.hash RETURNING id",
        )
        .bind(&name)
        .bind(&hash)
        .fetch_one(&mut *tx)
        .await?;
        out.push((row.get::<i64, _>(0), hash));
    }
    tx.commit().await?;
    Ok(out)
}

async fn monitor_loop(stats: Arc<Stats>, cfg: Arc<Config>) {
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut last_moves = 0u64;
    let mut last_effects = 0u64;
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
            cfg.clients,
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

async fn wait_for_ramp(stats: &Stats, clients: u32, timeout: Duration) {
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

struct ClientIo {
    out_tx: mpsc::UnboundedSender<Vec<u8>>,
    pending: Arc<Mutex<VecDeque<Instant>>>,
    writer_shutdown_tx: Option<oneshot::Sender<()>>,
    writer: tokio::task::JoinHandle<()>,
    reader: tokio::task::JoinHandle<ReaderReport>,
}

impl ClientIo {
    fn spawn(
        stream: TcpStream,
        stats: &Arc<Stats>,
    ) -> (Self, oneshot::Receiver<String>, oneshot::Receiver<()>) {
        let (mut read_half, mut write_half) = stream.into_split();
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (writer_shutdown_tx, mut writer_shutdown_rx) = oneshot::channel::<()>();
        let writer = tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = &mut writer_shutdown_rx => break,
                    packet = out_rx.recv() => {
                        let Some(packet) = packet else { break };
                        if write_half.write_all(&packet).await.is_err() {
                            break;
                        }
                    }
                }
            }
            let _ = write_half.shutdown().await;
        });

        let (sid_tx, sid_rx) = oneshot::channel::<String>();
        let (ready_tx, ready_rx) = oneshot::channel::<()>();
        let stats_r = Arc::clone(stats);
        let out_tx_r = out_tx.clone();
        let pending = Arc::new(Mutex::new(VecDeque::<Instant>::new()));
        let pending_r = pending.clone();
        let reader = tokio::spawn(async move {
            let mut buf: Vec<u8> = Vec::with_capacity(8192);
            let mut tmp = [0u8; 4096];
            let mut sid_tx = Some(sid_tx);
            let mut ready_tx = Some(ready_tx);
            let mut report = ReaderReport::default();
            loop {
                let n = match read_half.read(&mut tmp).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                buf.extend_from_slice(&tmp[..n]);
                let effects = drain_frames(
                    &mut buf,
                    &out_tx_r,
                    &mut sid_tx,
                    &mut ready_tx,
                    &pending_r,
                    &mut report.latencies_us,
                );
                stats_r
                    .effects_received
                    .fetch_add(effects, Ordering::Relaxed);
            }
            report
        });

        (
            Self {
                out_tx,
                pending,
                writer_shutdown_tx: Some(writer_shutdown_tx),
                writer,
                reader,
            },
            sid_rx,
            ready_rx,
        )
    }

    async fn close_writer(&mut self) {
        if let Some(shutdown_tx) = self.writer_shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
        let _ = tokio::time::timeout(Duration::from_secs(1), &mut self.writer).await;
    }

    async fn abort(mut self) {
        self.reader.abort();
        self.close_writer().await;
        let _ = self.reader.await;
    }
}

async fn connect_client(
    cfg: &Config,
    stats: &Arc<Stats>,
    user_id: i64,
    hash: &str,
) -> Option<ClientIo> {
    let Ok(stream) = TcpStream::connect((cfg.host.as_str(), cfg.port)).await else {
        stats.connect_errors.fetch_add(1, Ordering::Relaxed);
        return None;
    };
    let _ = stream.set_nodelay(true);
    stats.connected.fetch_add(1, Ordering::Relaxed);
    let (io, sid_rx, ready_rx) = ClientIo::spawn(stream, stats);

    // Regular-auth: token = MD5(hash + sid). Без auth-failure → IP не банится.
    let Ok(Ok(sid)) = tokio::time::timeout(Duration::from_secs(10), sid_rx).await else {
        io.abort().await;
        stats.unexpected_disconnects.fetch_add(1, Ordering::Relaxed);
        return None;
    };
    let token = format!("{:x}", md5::compute(format!("{hash}{sid}").as_bytes()));
    let _ = io
        .out_tx
        .send(u_frame(*b"AU", &format!("lt_{user_id}_{token}")));
    let Ok(Ok(())) = tokio::time::timeout(Duration::from_secs(10), ready_rx).await else {
        io.abort().await;
        stats.unexpected_disconnects.fetch_add(1, Ordering::Relaxed);
        return None;
    };
    stats.logged_in.fetch_add(1, Ordering::Relaxed);
    Some(io)
}

async fn run_steady(
    io: &mut ClientIo,
    cfg: &Config,
    stats: &Stats,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> (bool, Option<ReaderReport>) {
    if *shutdown_rx.borrow() {
        return (true, None);
    }
    // Движение: Xmov с циклическим направлением. Даже отклонённые/корректируемые
    // ходы нагружают dispatch+handle_move+broadcast.
    let start = Instant::now();
    let mut tick = tokio::time::interval(Duration::from_millis(cfg.move_ms));
    let mut seq: u32 = 0;
    loop {
        tokio::select! {
            biased;
            changed = shutdown_rx.changed() => {
                let _ = changed;
                return (true, None);
            }
            result = &mut io.reader => {
                return (false, Some(result.unwrap_or_default()));
            }
            _ = tick.tick() => {
                if io.out_tx.is_closed() {
                    return (false, None);
                }
                let dir = (seq % 4).to_string();
                let time = u32::try_from(start.elapsed().as_millis()).unwrap_or(u32::MAX);
                io.pending.lock().expect("pending move lock").push_back(Instant::now());
                if io.out_tx
                    .send(ty_frame(*b"Xmov", time, 0, 0, dir.as_bytes()))
                    .is_err()
                {
                    io.pending.lock().expect("pending move lock").pop_back();
                    return (false, None);
                }
                stats.moves_sent.fetch_add(1, Ordering::Relaxed);
                seq = seq.wrapping_add(1);
            }
        }
    }
}

async fn wait_for_steady_start(
    start_rx: &mut watch::Receiver<bool>,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> bool {
    if *start_rx.borrow() {
        return true;
    }
    tokio::select! {
        biased;
        changed = shutdown_rx.changed() => {
            let _ = changed;
            false
        }
        changed = start_rx.changed() => changed.is_ok() && *start_rx.borrow(),
    }
}

async fn finish_client(
    mut io: ClientIo,
    cfg: &Config,
    stats: &Stats,
    graceful_shutdown: bool,
    mut reader_report: Option<ReaderReport>,
) -> ClientReport {
    if graceful_shutdown {
        let drain_deadline = tokio::time::Instant::now() + Duration::from_secs(cfg.drain_secs);
        while !io.pending.lock().expect("pending move lock").is_empty() {
            if tokio::time::Instant::now() >= drain_deadline {
                stats.drain_timeouts.fetch_add(1, Ordering::Relaxed);
                break;
            }
            tokio::select! {
                result = &mut io.reader => {
                    reader_report = Some(result.unwrap_or_default());
                    break;
                }
                () = tokio::time::sleep(Duration::from_millis(5)) => {}
            }
        }
    }

    io.close_writer().await;

    if reader_report.is_none() {
        if let Ok(Ok(report)) = tokio::time::timeout(Duration::from_secs(2), &mut io.reader).await {
            reader_report = Some(report);
        } else {
            io.reader.abort();
            stats.drain_timeouts.fetch_add(1, Ordering::Relaxed);
        }
    }

    if graceful_shutdown && reader_report.is_some() {
        stats.graceful_disconnects.fetch_add(1, Ordering::Relaxed);
    } else if !graceful_shutdown {
        stats.unexpected_disconnects.fetch_add(1, Ordering::Relaxed);
    }

    ClientReport {
        latencies_us: reader_report.map_or_else(Vec::new, |report| report.latencies_us),
    }
}

async fn run_client(
    cfg: Arc<Config>,
    stats: Arc<Stats>,
    user_id: i64,
    hash: String,
    mut start_rx: watch::Receiver<bool>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> ClientReport {
    let Some(mut io) = connect_client(&cfg, &stats, user_id, &hash).await else {
        return ClientReport::default();
    };
    if !wait_for_steady_start(&mut start_rx, &mut shutdown_rx).await {
        return finish_client(io, &cfg, &stats, true, None).await;
    }
    let (graceful_shutdown, reader_report) =
        run_steady(&mut io, &cfg, &stats, &mut shutdown_rx).await;
    finish_client(io, &cfg, &stats, graceful_shutdown, reader_report).await
}

/// Вынуть из буфера все полные фреймы; ловит `sid` из AU, отвечает PO на PI.
fn drain_frames(
    buf: &mut Vec<u8>,
    out: &mpsc::UnboundedSender<Vec<u8>>,
    sid_tx: &mut Option<oneshot::Sender<String>>,
    ready_tx: &mut Option<oneshot::Sender<()>>,
    pending: &Mutex<VecDeque<Instant>>,
    latencies_us: &mut Vec<u64>,
) -> u64 {
    let mut effects = 0u64;
    let mut frames = BytesMut::from(&buf[..]);
    loop {
        let before = frames.len();
        let packet = match Packet::try_decode(&mut frames) {
            Ok(Some(packet)) => packet,
            Ok(None) => {
                let consumed = buf.len() - before;
                if consumed > 0 {
                    buf.drain(..consumed);
                }
                return effects;
            }
            Err(_) => {
                frames.clear();
                buf.clear();
                return effects;
            }
        };
        if packet.event_name == *b"AU" {
            if let Some(tx) = sid_tx.take() {
                let sid = String::from_utf8_lossy(&packet.payload).into_owned();
                let _ = tx.send(sid);
            }
        } else if packet.event_name == *b"@P" {
            // Финальный packet Player.Init (#16). `Gu` приходит раньше, до
            // ApplyInitialSync, поэтому не является steady-state barrier.
            if let Some(tx) = ready_tx.take() {
                let _ = tx.send(());
            }
        } else if packet.event_name == *b"PI" {
            let now = u32::try_from(
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis()),
            )
            .unwrap_or(u32::MAX);
            let _ = out.send(u_frame(*b"PO", &format!("0:{now}")));
        } else if packet.event_name == *b"@T"
            && let Some(sent_at) = pending.lock().expect("pending move lock").pop_front()
        {
            latencies_us.push(u64::try_from(sent_at.elapsed().as_micros()).unwrap_or(u64::MAX));
            effects = effects.saturating_add(1);
        }
        let consumed = before - frames.len();
        buf.drain(..consumed);
    }
}

fn percentile(sorted: &[u64], numerator: usize, denominator: usize) -> Option<u64> {
    if sorted.is_empty() || denominator == 0 || numerator > denominator {
        return None;
    }
    let rank = sorted
        .len()
        .saturating_mul(numerator)
        .div_ceil(denominator)
        .saturating_sub(1);
    sorted.get(rank).copied()
}

fn print_latency_summary(sorted: &[u64]) {
    let ms = |value: Option<u64>| {
        value.map_or(f64::NAN, |us| {
            Duration::from_micros(us).as_secs_f64() * 1_000.0
        })
    };
    println!(
        "  command→effect latency: samples={} p50={:.3}ms p95={:.3}ms p99={:.3}ms p99.9={:.3}ms max={:.3}ms",
        sorted.len(),
        ms(percentile(sorted, 50, 100)),
        ms(percentile(sorted, 95, 100)),
        ms(percentile(sorted, 99, 100)),
        ms(percentile(sorted, 999, 1_000)),
        ms(sorted.last().copied()),
    );
}
use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(name = "loadtest", about = "Нагрузочный замер OpenMines")]
struct Args {
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    #[arg(long, default_value_t = 8090)]
    port: u16,

    #[arg(long, default_value_t = 500)]
    clients: u32,

    #[arg(long, default_value_t = 30)]
    secs: u64,

    #[arg(long, default_value_t = 200)]
    move_ms: u64,

    #[arg(long, default_value_t = 3)]
    ramp_ms: u64,

    #[arg(long, default_value_t = 5)]
    drain_secs: u64,

    #[arg(long, default_value = "data/openmines.db")]
    db: String,
}

fn parse_args() -> Config {
    let parsed = Args::parse();
    Config {
        host: parsed.host,
        port: parsed.port,
        clients: parsed.clients,
        secs: parsed.secs,
        move_ms: parsed.move_ms,
        ramp_ms: parsed.ramp_ms,
        drain_secs: parsed.drain_secs,
        db: parsed.db,
    }
}

#[cfg(test)]
fn parse_args_from<I, S>(args: I) -> Result<Config, clap::Error>
where
    I: IntoIterator<Item = S>,
    S: Into<std::ffi::OsString> + Clone,
{
    let mut os_args = vec![std::ffi::OsString::from("loadtest")];
    os_args.extend(args.into_iter().map(Into::into));
    let parsed = Args::try_parse_from(os_args)?;
    Ok(Config {
        host: parsed.host,
        port: parsed.port,
        clients: parsed.clients,
        secs: parsed.secs,
        move_ms: parsed.move_ms,
        ramp_ms: parsed.ramp_ms,
        drain_secs: parsed.drain_secs,
        db: parsed.db,
    })
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
