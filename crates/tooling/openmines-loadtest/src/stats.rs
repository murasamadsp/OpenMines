use std::sync::atomic::AtomicU64;
use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionKind {
    Movement,
    Dig,
    Build,
}

#[derive(Default)]
pub struct ActionStats {
    pub movement_sent: AtomicU64,
    pub dig_sent: AtomicU64,
    pub build_sent: AtomicU64,
    pub movement_acked: AtomicU64,
    pub dig_acked: AtomicU64,
    pub build_acked: AtomicU64,
    pub movement_rejected: AtomicU64,
    pub dig_rejected: AtomicU64,
    pub build_rejected: AtomicU64,
}

impl ActionStats {
    pub fn sent(&self, action: ActionKind) {
        let counter = match action {
            ActionKind::Movement => &self.movement_sent,
            ActionKind::Dig => &self.dig_sent,
            ActionKind::Build => &self.build_sent,
        };
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn acknowledged(&self, action: ActionKind) {
        let counter = match action {
            ActionKind::Movement => &self.movement_acked,
            ActionKind::Dig => &self.dig_acked,
            ActionKind::Build => &self.build_acked,
        };
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn rejected(&self, action: ActionKind) {
        let counter = match action {
            ActionKind::Movement => &self.movement_rejected,
            ActionKind::Dig => &self.dig_rejected,
            ActionKind::Build => &self.build_rejected,
        };
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

#[derive(Default)]
pub struct Stats {
    pub phase: AtomicU64,
    pub connected: AtomicU64,
    pub logged_in: AtomicU64,
    pub moves_sent: AtomicU64,
    pub effects_received: AtomicU64,
    pub graceful_disconnects: AtomicU64,
    pub unexpected_disconnects: AtomicU64,
    pub drain_timeouts: AtomicU64,
    pub connect_errors: AtomicU64,
    pub session_id_timeouts: AtomicU64,
    pub session_id_closed: AtomicU64,
    pub ready_timeouts: AtomicU64,
    pub ready_closed: AtomicU64,
    pub actions: ActionStats,
}

#[derive(Default)]
pub struct ClientReport {
    pub latencies_us: Vec<u64>,
}

#[derive(Default)]
pub struct ReaderReport {
    pub latencies_us: Vec<u64>,
}

pub fn percentile(sorted: &[u64], numerator: usize, denominator: usize) -> Option<u64> {
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

pub fn print_latency_summary(sorted: &[u64]) {
    let milliseconds = |value: Option<u64>| {
        value.map_or(f64::NAN, |micros| {
            Duration::from_micros(micros).as_secs_f64() * 1_000.0
        })
    };
    println!(
        "  command→effect latency: samples={} p50={:.3}ms p95={:.3}ms p99={:.3}ms p99.9={:.3}ms max={:.3}ms",
        sorted.len(),
        milliseconds(percentile(sorted, 50, 100)),
        milliseconds(percentile(sorted, 95, 100)),
        milliseconds(percentile(sorted, 99, 100)),
        milliseconds(percentile(sorted, 999, 1_000)),
        milliseconds(sorted.last().copied()),
    );
}
