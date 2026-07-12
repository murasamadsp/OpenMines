use std::sync::atomic::AtomicU64;
use std::time::Duration;

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
