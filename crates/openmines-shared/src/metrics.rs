use prometheus::{
    Encoder, Histogram, HistogramOpts, IntCounter, IntCounterVec, IntGauge, Opts, Registry,
    TextEncoder,
};
use std::sync::LazyLock;

pub static REGISTRY: LazyLock<Registry> = LazyLock::new(Registry::new);

pub static TCP_CONNECTIONS_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    let c = IntCounter::with_opts(Opts::new(
        "openmines_tcp_connections_total",
        "Total accepted TCP connections",
    ))
    .expect("metric");
    REGISTRY.register(Box::new(c.clone())).expect("register");
    c
});

pub static TCP_CONNECTIONS_CURRENT: LazyLock<IntGauge> = LazyLock::new(|| {
    let g = IntGauge::with_opts(Opts::new(
        "openmines_tcp_connections_current",
        "Current active TCP connections",
    ))
    .expect("metric");
    REGISTRY.register(Box::new(g.clone())).expect("register");
    g
});

pub static PACKETS_IN_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    let v = IntCounterVec::new(
        Opts::new(
            "openmines_packets_in_total",
            "Total inbound packets by event",
        ),
        &["event"],
    )
    .expect("metric");
    REGISTRY.register(Box::new(v.clone())).expect("register");
    v
});

pub static PACKETS_OUT_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    let v = IntCounterVec::new(
        Opts::new(
            "openmines_packets_out_total",
            "Total outbound packets by event",
        ),
        &["event"],
    )
    .expect("metric");
    REGISTRY.register(Box::new(v.clone())).expect("register");
    v
});

pub static TY_EVENTS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    let v = IntCounterVec::new(
        Opts::new("openmines_ty_events_total", "Total TY events by name"),
        &["ty"],
    )
    .expect("metric");
    REGISTRY.register(Box::new(v.clone())).expect("register");
    v
});

pub static WORLD_FLUSH_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    let c = IntCounter::with_opts(Opts::new(
        "openmines_world_flush_total",
        "Total world flush operations",
    ))
    .expect("metric");
    REGISTRY.register(Box::new(c.clone())).expect("register");
    c
});

pub static WORLD_FLUSH_SECONDS: LazyLock<Histogram> = LazyLock::new(|| {
    let h = Histogram::with_opts(HistogramOpts::new(
        "openmines_world_flush_duration_seconds",
        "World flush duration in seconds",
    ))
    .expect("metric");
    REGISTRY.register(Box::new(h.clone())).expect("register");
    h
});

pub static PLAYER_SAVE_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    let c = IntCounter::with_opts(Opts::new(
        "openmines_player_save_total",
        "Total player save operations",
    ))
    .expect("metric");
    REGISTRY.register(Box::new(c.clone())).expect("register");
    c
});

pub fn gather_text() -> Vec<u8> {
    let metric_families = REGISTRY.gather();
    let encoder = TextEncoder::new();
    let mut buf = Vec::new();
    encoder.encode(&metric_families, &mut buf).expect("encode");
    buf
}
