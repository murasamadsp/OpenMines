use crate::config::{Config, Workload};
use crate::protocol::{ty_frame, u_frame};
use crate::stats::{ActionKind, ClientReport, ReaderReport, Stats};
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
    pending: Arc<Mutex<VecDeque<PendingAction>>>,
    writer_shutdown_tx: Option<oneshot::Sender<()>>,
    writer: tokio::task::JoinHandle<()>,
    reader: tokio::task::JoinHandle<ReaderReport>,
}

#[derive(Clone, Copy)]
pub struct PendingAction {
    pub kind: ActionKind,
    pub sent_at: Instant,
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
        let pending = Arc::new(Mutex::new(VecDeque::<PendingAction>::new()));
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
                    &stats_r,
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
    let sid = match tokio::time::timeout(Duration::from_secs(10), sid_rx).await {
        Ok(Ok(sid)) => sid,
        Ok(Err(_)) => {
            stats.session_id_closed.fetch_add(1, Ordering::Relaxed);
            io.abort().await;
            stats.unexpected_disconnects.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        Err(_) => {
            stats.session_id_timeouts.fetch_add(1, Ordering::Relaxed);
            io.abort().await;
            stats.unexpected_disconnects.fetch_add(1, Ordering::Relaxed);
            return None;
        }
    };
    let token = format!("{:x}", md5::compute(format!("{hash}{sid}").as_bytes()));
    let _ = io
        .out_tx
        .send(u_frame(*b"AU", &format!("lt_{user_id}_{token}")));
    match tokio::time::timeout(Duration::from_secs(10), ready_rx).await {
        Ok(Ok(())) => {}
        Ok(Err(_)) => {
            stats.ready_closed.fetch_add(1, Ordering::Relaxed);
            io.abort().await;
            stats.unexpected_disconnects.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        Err(_) => {
            stats.ready_timeouts.fetch_add(1, Ordering::Relaxed);
            io.abort().await;
            stats.unexpected_disconnects.fetch_add(1, Ordering::Relaxed);
            return None;
        }
    }
    stats.logged_in.fetch_add(1, Ordering::Relaxed);
    Some(io)
}

async fn run_steady(
    io: &mut ClientIo,
    cfg: &Config,
    stats: &Stats,
    phase_offset_ms: u64,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> (bool, Option<ReaderReport>) {
    if *shutdown_rx.borrow() {
        return (true, None);
    }
    let start = Instant::now();
    if phase_offset_ms > 0 {
        tokio::select! {
            biased;
            changed = shutdown_rx.changed() => {
                let _ = changed;
                return (true, None);
            }
            () = tokio::time::sleep(Duration::from_millis(phase_offset_ms)) => {}
        }
    }
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
                let time = u32::try_from(start.elapsed().as_millis()).unwrap_or(u32::MAX);
                let action = next_action(cfg.workload, seq);
                let packet = action_packet(action, time, seq);
                io.pending.lock().expect("pending action lock").push_back(PendingAction {
                    kind: action,
                    sent_at: Instant::now(),
                });
                if io.out_tx.send(packet).is_err() {
                    io.pending.lock().expect("pending action lock").pop_back();
                    return (false, None);
                }
                stats.actions.sent(action);
                if action == ActionKind::Movement {
                    stats.moves_sent.fetch_add(1, Ordering::Relaxed);
                }
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
        while !io.pending.lock().expect("pending action lock").is_empty() {
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

    let rejected: Vec<_> = {
        let mut pending = io.pending.lock().expect("pending action lock");
        pending.drain(..).collect()
    };
    for action in rejected {
        stats.actions.rejected(action.kind);
    }

    ClientReport {
        latencies_us: reader_report.map_or_else(Vec::new, |report| report.latencies_us),
    }
}

pub const fn next_action(workload: Workload, sequence: u32) -> ActionKind {
    match workload {
        Workload::Movement => ActionKind::Movement,
        Workload::Dig => ActionKind::Dig,
        Workload::Build => ActionKind::Build,
        Workload::BuildCycle => {
            // A green block has durability 1 while baseline digging removes 0.2 per
            // accepted hit. Twelve attempts leave room for 200ms cooldown jitter.
            if sequence.is_multiple_of(13) {
                ActionKind::Build
            } else {
                ActionKind::Dig
            }
        }
        Workload::Mixed => match sequence % 4 {
            0 | 3 => ActionKind::Movement,
            1 => ActionKind::Dig,
            _ => ActionKind::Build,
        },
    }
}

fn action_packet(action: ActionKind, time: u32, sequence: u32) -> Vec<u8> {
    match action {
        ActionKind::Movement => {
            let dir = (sequence % 4).to_string();
            ty_frame(*b"Xmov", time, 0, 0, dir.as_bytes())
        }
        ActionKind::Dig => ty_frame(*b"Xdig", time, 0, 0, b"0"),
        ActionKind::Build => ty_frame(*b"Xbld", time, 0, 0, b"0G"),
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
    let phase_offset_ms = action_phase_offset_ms(user_id, cfg.move_ms, cfg.synchronized_actions);
    let (graceful_shutdown, reader_report) =
        run_steady(&mut io, &cfg, &stats, phase_offset_ms, &mut shutdown_rx).await;
    finish_client(io, &cfg, &stats, graceful_shutdown, reader_report).await
}

const fn action_phase_offset_ms(user_id: i64, move_ms: u64, synchronized: bool) -> u64 {
    if synchronized || move_ms <= 1 {
        0
    } else {
        user_id.unsigned_abs() % move_ms
    }
}

#[cfg(test)]
pub const fn action_phase_offset_ms_for_test(
    user_id: i64,
    move_ms: u64,
    synchronized: bool,
) -> u64 {
    action_phase_offset_ms(user_id, move_ms, synchronized)
}

/// Вынуть из буфера все полные фреймы; ловит `sid` из AU, отвечает PO на PI.
pub fn drain_frames(
    buf: &mut Vec<u8>,
    out: &mpsc::UnboundedSender<Vec<u8>>,
    sid_tx: &mut Option<oneshot::Sender<String>>,
    ready_tx: &mut Option<oneshot::Sender<()>>,
    pending: &Mutex<VecDeque<PendingAction>>,
    stats: &Stats,
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
        } else if packet.event_name == *b"@T" {
            effects = effects.saturating_add(acknowledge_action(
                pending,
                stats,
                ActionKind::Movement,
                latencies_us,
            ));
        } else if packet.event_name == *b"HB" {
            effects = effects.saturating_add(acknowledge_first_visual_action(
                pending,
                stats,
                latencies_us,
            ));
        } else if packet.event_name == *b"@B" {
            effects = effects.saturating_add(acknowledge_first_economy_action(
                pending,
                stats,
                latencies_us,
            ));
        }
        let consumed = before - frames.len();
        buf.drain(..consumed);
    }
}

fn acknowledge_action(
    pending: &Mutex<VecDeque<PendingAction>>,
    stats: &Stats,
    expected: ActionKind,
    latencies_us: &mut Vec<u64>,
) -> u64 {
    let action = {
        let mut pending = pending.lock().expect("pending action lock");
        let Some(index) = pending.iter().position(|action| action.kind == expected) else {
            return 0;
        };
        pending.remove(index)
    };
    let Some(action) = action else {
        return 0;
    };
    stats.actions.acknowledged(action.kind);
    latencies_us.push(u64::try_from(action.sent_at.elapsed().as_micros()).unwrap_or(u64::MAX));
    1
}

fn acknowledge_first_visual_action(
    pending: &Mutex<VecDeque<PendingAction>>,
    stats: &Stats,
    latencies_us: &mut Vec<u64>,
) -> u64 {
    let action = {
        let mut pending = pending.lock().expect("pending action lock");
        let Some(index) = pending
            .iter()
            .position(|action| matches!(action.kind, ActionKind::Movement | ActionKind::Dig))
        else {
            return 0;
        };
        pending.remove(index)
    };
    let Some(action) = action else {
        return 0;
    };
    stats.actions.acknowledged(action.kind);
    latencies_us.push(u64::try_from(action.sent_at.elapsed().as_micros()).unwrap_or(u64::MAX));
    1
}

fn acknowledge_first_economy_action(
    pending: &Mutex<VecDeque<PendingAction>>,
    stats: &Stats,
    latencies_us: &mut Vec<u64>,
) -> u64 {
    let action = {
        let mut pending = pending.lock().expect("pending action lock");
        let index = pending
            .iter()
            .position(|action| action.kind == ActionKind::Build)
            .or_else(|| {
                pending
                    .iter()
                    .position(|action| action.kind == ActionKind::Dig)
            });
        let Some(index) = index else {
            return 0;
        };
        pending.remove(index)
    };
    let Some(action) = action else {
        return 0;
    };
    stats.actions.acknowledged(action.kind);
    latencies_us.push(u64::try_from(action.sent_at.elapsed().as_micros()).unwrap_or(u64::MAX));
    1
}
