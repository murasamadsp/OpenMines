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
//!        --db PATH (файл БД сервера, по умолчанию data/openmines.db).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};

#[derive(Clone)]
struct Config {
    host: String,
    port: u16,
    clients: u32,
    secs: u64,
    move_ms: u64,
    ramp_ms: u64,
    db: String,
}

#[derive(Default)]
struct Stats {
    connected: AtomicU64,
    logged_in: AtomicU64,
    moves_sent: AtomicU64,
    disconnected: AtomicU64,
    connect_errors: AtomicU64,
}

// ─── Wire-хелперы (self-contained: bin-крейт не делит модули с сервером) ──────

/// Внешний фрейм: `[i32 LE len=7+payload][1B type][2B event][payload]`.
fn frame(data_type: u8, event: [u8; 2], payload: &[u8]) -> Vec<u8> {
    let total = i32::try_from(7 + payload.len()).unwrap_or(i32::MAX);
    let mut v = Vec::with_capacity(7 + payload.len());
    v.extend_from_slice(&total.to_le_bytes());
    v.push(data_type);
    v.extend_from_slice(&event);
    v.extend_from_slice(payload);
    v
}

fn u_frame(event: [u8; 2], s: &str) -> Vec<u8> {
    frame(b'U', event, s.as_bytes())
}

/// TY-пакет: внешний `B`/"TY", payload = `[4B ev][u32 time][u32 x][u32 y][sub]` (LE).
fn ty_frame(inner_event: [u8; 4], time: u32, x: u32, y: u32, sub: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(16 + sub.len());
    p.extend_from_slice(&inner_event);
    p.extend_from_slice(&time.to_le_bytes());
    p.extend_from_slice(&x.to_le_bytes());
    p.extend_from_slice(&y.to_le_bytes());
    p.extend_from_slice(sub);
    frame(b'B', *b"TY", &p)
}

#[tokio::main]
async fn main() {
    let cfg = parse_args();
    println!(
        "loadtest: {} клиентов → {}:{}, движение {}с (Xmov каждые {}мс), ramp {}мс/клиент",
        cfg.clients, cfg.host, cfg.port, cfg.secs, cfg.move_ms, cfg.ramp_ms
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

    let mut tasks = Vec::with_capacity(cfg.clients as usize);
    for i in 0..cfg.clients {
        let (id, hash) = creds[i as usize].clone();
        tasks.push(tokio::spawn(run_client(
            cfg.clone(),
            stats.clone(),
            id,
            hash,
        )));
        if cfg.ramp_ms > 0 {
            tokio::time::sleep(Duration::from_millis(cfg.ramp_ms)).await;
        }
    }

    tokio::time::sleep(Duration::from_secs(cfg.secs)).await;

    monitor.abort();
    for t in tasks {
        t.abort();
    }

    println!(
        "\n=== ИТОГ ===\n  коннектов: {}\n  логинов: {}\n  ходов отправлено: {}\n  обрывов: {}\n  ошибок коннекта: {}",
        stats.connected.load(Ordering::Relaxed),
        stats.logged_in.load(Ordering::Relaxed),
        stats.moves_sent.load(Ordering::Relaxed),
        stats.disconnected.load(Ordering::Relaxed),
        stats.connect_errors.load(Ordering::Relaxed),
    );
}

/// Вставить N игроков `loadtest_<i>` (детерминированный хэш) в БД сервера.
/// Идемпотентно (`ON CONFLICT` обновляет хэш). Возвращает `(id, hash)` по индексу.
async fn seed_players(db_path: &str, n: u32) -> Result<Vec<(i64, String)>, anyhow::Error> {
    use sqlx::Row;
    let database = openmines_shared::db::Database::open(db_path).await?;
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
    loop {
        tick.tick().await;
        let moves = stats.moves_sent.load(Ordering::Relaxed);
        println!(
            "  [t] онлайн={}/{} логинов={} ходов/с={} обрывов={} ошибок={}",
            stats.connected.load(Ordering::Relaxed) - stats.disconnected.load(Ordering::Relaxed),
            cfg.clients,
            stats.logged_in.load(Ordering::Relaxed),
            moves.saturating_sub(last_moves),
            stats.disconnected.load(Ordering::Relaxed),
            stats.connect_errors.load(Ordering::Relaxed),
        );
        last_moves = moves;
    }
}

async fn run_client(cfg: Arc<Config>, stats: Arc<Stats>, user_id: i64, hash: String) {
    let Ok(stream) = TcpStream::connect((cfg.host.as_str(), cfg.port)).await else {
        stats.connect_errors.fetch_add(1, Ordering::Relaxed);
        return;
    };
    let _ = stream.set_nodelay(true);
    stats.connected.fetch_add(1, Ordering::Relaxed);

    let (mut read_half, mut write_half) = stream.into_split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    let writer = tokio::spawn(async move {
        while let Some(buf) = out_rx.recv().await {
            if write_half.write_all(&buf).await.is_err() {
                break;
            }
        }
    });

    // Reader: ловит `sid` из AU-пакета (для токена), отвечает PO на PI.
    let (sid_tx, sid_rx) = oneshot::channel::<String>();
    let stats_r = stats.clone();
    let out_tx_r = out_tx.clone();
    let reader = tokio::spawn(async move {
        let mut buf: Vec<u8> = Vec::with_capacity(8192);
        let mut tmp = [0u8; 4096];
        let mut sid_tx = Some(sid_tx);
        loop {
            let n = match read_half.read(&mut tmp).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            buf.extend_from_slice(&tmp[..n]);
            drain_frames(&mut buf, &out_tx_r, &mut sid_tx);
        }
        stats_r.disconnected.fetch_add(1, Ordering::Relaxed);
    });

    // Regular-auth: token = MD5(hash + sid). Без auth-failure → IP не банится.
    let Ok(Ok(sid)) = tokio::time::timeout(Duration::from_secs(10), sid_rx).await else {
        reader.abort();
        writer.abort();
        return;
    };
    let token = format!("{:x}", md5::compute(format!("{hash}{sid}").as_bytes()));
    let _ = out_tx.send(u_frame(*b"AU", &format!("lt_{user_id}_{token}")));
    stats.logged_in.fetch_add(1, Ordering::Relaxed);

    // Движение: Xmov с циклическим направлением. Даже отклонённые/корректируемые
    // ходы нагружают dispatch+handle_move+broadcast.
    let start = Instant::now();
    let mut tick = tokio::time::interval(Duration::from_millis(cfg.move_ms));
    let mut seq: u32 = 0;
    loop {
        tick.tick().await;
        if out_tx.is_closed() {
            break;
        }
        let dir = (seq % 4).to_string();
        let time = u32::try_from(start.elapsed().as_millis()).unwrap_or(u32::MAX);
        let _ = out_tx.send(ty_frame(*b"Xmov", time, 0, 0, dir.as_bytes()));
        stats.moves_sent.fetch_add(1, Ordering::Relaxed);
        seq = seq.wrapping_add(1);
    }

    reader.abort();
    writer.abort();
}

/// Вынуть из буфера все полные фреймы; ловит `sid` из AU, отвечает PO на PI.
fn drain_frames(
    buf: &mut Vec<u8>,
    out: &mpsc::UnboundedSender<Vec<u8>>,
    sid_tx: &mut Option<oneshot::Sender<String>>,
) {
    loop {
        if buf.len() < 4 {
            return;
        }
        let len = i32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let Ok(len) = usize::try_from(len) else {
            buf.clear();
            return;
        };
        if !(7..=65536).contains(&len) {
            buf.clear();
            return;
        }
        if buf.len() < len {
            return;
        }
        let event = [buf[5], buf[6]];
        if &event == b"AU" {
            if let Some(tx) = sid_tx.take() {
                let sid = String::from_utf8_lossy(&buf[7..len]).into_owned();
                let _ = tx.send(sid);
            }
        } else if &event == b"PI" {
            let now = u32::try_from(
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis()),
            )
            .unwrap_or(u32::MAX);
            let _ = out.send(u_frame(*b"PO", &format!("0:{now}")));
        }
        buf.drain(..len);
    }
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
}
