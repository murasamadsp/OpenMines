use prometheus::{
    Encoder, Gauge, Histogram, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGauge,
    Opts, Registry, TextEncoder,
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

pub static WORLD_FLUSH_DURABILITY_CHUNKS_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    let c = IntCounter::with_opts(Opts::new(
        "openmines_world_flush_durability_chunks_total",
        "Total dirty durability chunks flushed",
    ))
    .expect("metric");
    REGISTRY.register(Box::new(c.clone())).expect("register");
    c
});

pub static WORLD_FLUSH_DURABILITY_RANGES_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    let c = IntCounter::with_opts(Opts::new(
        "openmines_world_flush_durability_ranges_total",
        "Total contiguous durability ranges flushed",
    ))
    .expect("metric");
    REGISTRY.register(Box::new(c.clone())).expect("register");
    c
});

pub static WORLD_FLUSH_DURABILITY_BYTES_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    let c = IntCounter::with_opts(Opts::new(
        "openmines_world_flush_durability_bytes_total",
        "Total durability bytes covered by dirty-range flushes",
    ))
    .expect("metric");
    REGISTRY.register(Box::new(c.clone())).expect("register");
    c
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

pub static PERSISTENCE_QUEUE_DEPTH: LazyLock<IntGauge> = LazyLock::new(|| {
    let gauge = IntGauge::with_opts(Opts::new(
        "openmines_persistence_queue_depth",
        "Accepted durable commands not yet persisted",
    ))
    .expect("metric");
    REGISTRY
        .register(Box::new(gauge.clone()))
        .expect("register");
    gauge
});

pub static PERSISTENCE_QUEUE_HIGH_WATER: LazyLock<IntGauge> = LazyLock::new(|| {
    let gauge = IntGauge::with_opts(Opts::new(
        "openmines_persistence_queue_high_water",
        "Highest durable persistence backlog since process start",
    ))
    .expect("metric");
    REGISTRY
        .register(Box::new(gauge.clone()))
        .expect("register");
    gauge
});

pub static PERSISTENCE_OLDEST_AGE_SECONDS: LazyLock<Gauge> = LazyLock::new(|| {
    let gauge = Gauge::with_opts(Opts::new(
        "openmines_persistence_oldest_age_seconds",
        "Age of the oldest in-flight durable persistence command",
    ))
    .expect("metric");
    REGISTRY
        .register(Box::new(gauge.clone()))
        .expect("register");
    gauge
});

pub static PERSISTENCE_COMMANDS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    let counter = IntCounterVec::new(
        Opts::new(
            "openmines_persistence_commands_total",
            "Durable persistence commands by kind and result",
        ),
        &["kind", "result"],
    )
    .expect("metric");
    REGISTRY
        .register(Box::new(counter.clone()))
        .expect("register");
    counter
});

pub static PERSISTENCE_BATCH_SIZE: LazyLock<Histogram> = LazyLock::new(|| {
    let histogram = Histogram::with_opts(
        HistogramOpts::new(
            "openmines_persistence_batch_size",
            "Number of durable commands persisted in one transaction",
        )
        .buckets(vec![1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0]),
    )
    .expect("metric");
    REGISTRY
        .register(Box::new(histogram.clone()))
        .expect("register");
    histogram
});

const COMMAND_LATENCY_BUCKETS: &[f64] = &[
    0.000_1, 0.000_25, 0.000_5, 0.001, 0.0025, 0.005, 0.01, 0.02, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5,
    5.0,
];

fn command_histogram(name: &str, help: &str) -> HistogramVec {
    let opts = HistogramOpts::new(name, help).buckets(COMMAND_LATENCY_BUCKETS.to_vec());
    let histogram = HistogramVec::new(opts, &["kind"]).expect("metric");
    REGISTRY
        .register(Box::new(histogram.clone()))
        .expect("register");
    histogram
}

pub static COMMAND_RECEIVE_TO_ENQUEUE_SECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    command_histogram(
        "openmines_command_receive_to_enqueue_duration_seconds",
        "Time from decoded inbound command receipt to simulation queue enqueue",
    )
});

pub static COMMAND_QUEUE_RESIDENCE_SECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    command_histogram(
        "openmines_command_queue_residence_duration_seconds",
        "Time commands spend waiting in the simulation input queue",
    )
});

pub static COMMAND_RECEIVE_TO_APPLY_SECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    command_histogram(
        "openmines_command_receive_to_apply_duration_seconds",
        "Time from decoded inbound command receipt to simulation apply start",
    )
});

pub static COMMAND_APPLY_SECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    command_histogram(
        "openmines_command_apply_duration_seconds",
        "Simulation command apply duration",
    )
});

pub static COMMANDS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    let counter = IntCounterVec::new(
        Opts::new(
            "openmines_commands_total",
            "Commands by stable kind and processing result",
        ),
        &["kind", "result"],
    )
    .expect("metric");
    REGISTRY
        .register(Box::new(counter.clone()))
        .expect("register");
    counter
});

pub static COMMAND_QUEUE_DEPTH: LazyLock<IntGauge> = LazyLock::new(|| {
    let gauge = IntGauge::with_opts(Opts::new(
        "openmines_command_queue_depth",
        "Current simulation input command queue depth",
    ))
    .expect("metric");
    REGISTRY
        .register(Box::new(gauge.clone()))
        .expect("register");
    gauge
});

pub static COMMAND_QUEUE_HIGH_WATER: LazyLock<IntGauge> = LazyLock::new(|| {
    let gauge = IntGauge::with_opts(Opts::new(
        "openmines_command_queue_high_water",
        "Highest observed simulation input command queue depth since process start",
    ))
    .expect("metric");
    REGISTRY
        .register(Box::new(gauge.clone()))
        .expect("register");
    gauge
});

pub static PRESENTATION_QUEUE_DEPTH: LazyLock<IntGauge> = LazyLock::new(|| {
    let gauge = IntGauge::with_opts(Opts::new(
        "openmines_presentation_queue_depth",
        "Current bounded presentation event queue depth",
    ))
    .expect("metric");
    REGISTRY
        .register(Box::new(gauge.clone()))
        .expect("register");
    gauge
});

pub static PRESENTATION_EVENTS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    let counter = IntCounterVec::new(
        Opts::new(
            "openmines_presentation_events_total",
            "Presentation events by stable kind and processing result",
        ),
        &["kind", "result"],
    )
    .expect("metric");
    REGISTRY
        .register(Box::new(counter.clone()))
        .expect("register");
    counter
});

pub static COMMAND_SEQUENCE: LazyLock<IntGauge> = LazyLock::new(|| {
    let gauge = IntGauge::with_opts(Opts::new(
        "openmines_command_last_applied_sequence",
        "Sequence of the last command applied by the simulation",
    ))
    .expect("metric");
    REGISTRY
        .register(Box::new(gauge.clone()))
        .expect("register");
    gauge
});

pub static SIMULATION_TICK: LazyLock<IntGauge> = LazyLock::new(|| {
    let gauge = IntGauge::with_opts(Opts::new(
        "openmines_simulation_tick",
        "Current monotonic simulation tick",
    ))
    .expect("metric");
    REGISTRY
        .register(Box::new(gauge.clone()))
        .expect("register");
    gauge
});

const TICK_CADENCE_BUCKETS: &[f64] = &[
    0.000_1, 0.000_25, 0.000_5, 0.001, 0.0025, 0.005, 0.01, 0.0125, 0.02, 0.05, 0.1, 0.25, 0.5, 1.0,
];

fn tick_histogram(name: &str, help: &str) -> Histogram {
    let histogram =
        Histogram::with_opts(HistogramOpts::new(name, help).buckets(TICK_CADENCE_BUCKETS.to_vec()))
            .expect("metric");
    REGISTRY
        .register(Box::new(histogram.clone()))
        .expect("register");
    histogram
}

pub static TICK_START_INTERVAL_SECONDS: LazyLock<Histogram> = LazyLock::new(|| {
    tick_histogram(
        "openmines_tick_start_interval_seconds",
        "Wall-clock interval between consecutive simulation tick starts",
    )
});

pub static TICK_WAKE_LATENESS_SECONDS: LazyLock<Histogram> = LazyLock::new(|| {
    tick_histogram(
        "openmines_tick_wake_lateness_seconds",
        "Simulation tick start lateness after the requested sleep deadline",
    )
});

pub static BOTS_RENDER_OBSERVERS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    let counter = IntCounterVec::new(
        Opts::new(
            "openmines_bots_render_observers_total",
            "Bots-render observers by batch result",
        ),
        &["result"],
    )
    .expect("metric");
    REGISTRY
        .register(Box::new(counter.clone()))
        .expect("register");
    counter
});

pub static BOTS_RENDER_BYTES_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    let counter = IntCounter::with_opts(Opts::new(
        "openmines_bots_render_bytes_total",
        "Total encoded bots-render wire bytes enqueued",
    ))
    .expect("metric");
    REGISTRY
        .register(Box::new(counter.clone()))
        .expect("register");
    counter
});

pub static BOTS_RENDER_SNAPSHOT_CHUNKS: LazyLock<IntGauge> = LazyLock::new(|| {
    let gauge = IntGauge::with_opts(Opts::new(
        "openmines_bots_render_snapshot_chunks",
        "Unique chunks in the latest bots-render batch snapshot",
    ))
    .expect("metric");
    REGISTRY
        .register(Box::new(gauge.clone()))
        .expect("register");
    gauge
});

pub static CRAFTING_DUE_BATCH_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    let counter = IntCounter::with_opts(Opts::new(
        "openmines_crafting_due_batch_total",
        "Total due crafting entries selected for targeted apply",
    ))
    .expect("metric");
    REGISTRY
        .register(Box::new(counter.clone()))
        .expect("register");
    counter
});

pub static CRAFTING_DUE_DEPTH: LazyLock<IntGauge> = LazyLock::new(|| {
    let gauge = IntGauge::with_opts(Opts::new(
        "openmines_crafting_due_depth",
        "Current crafting deadline heap depth after due selection",
    ))
    .expect("metric");
    REGISTRY
        .register(Box::new(gauge.clone()))
        .expect("register");
    gauge
});

pub fn gather_text() -> Vec<u8> {
    let metric_families = REGISTRY.gather();
    let encoder = TextEncoder::new();
    let mut buf = Vec::new();
    encoder.encode(&metric_families, &mut buf).expect("encode");
    buf
}
