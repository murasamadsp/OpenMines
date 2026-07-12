use crate::config::Config;
use crate::protocol::{ty_frame, u_frame};
use crate::stats::{ClientReport, ReaderReport, Stats};
use bytes::BytesMut;
use openmines_protocol::Packet;
use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, watch};

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

pub async fn run_client(
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
pub fn drain_frames(
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
