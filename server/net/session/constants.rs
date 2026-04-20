use std::time::Duration;

/// Reference server keeps `PI` cadence based on client timestamps, with delayed
/// response around 200ms and fallback ping every 10s if no `PO` received.
// TODO: heartbeat/robot constants will be used when heartbeat timing and robot speed logic are fully wired
#[allow(dead_code)]
pub const HEARTBEAT_RTT_BASE_MS: i32 = 201;
#[allow(dead_code)]
pub const HEARTBEAT_FALLBACK_INTERVAL: Duration = Duration::from_secs(10);
#[allow(dead_code)]
pub const HEARTBEAT_DISCONNECT_TIMEOUT: Duration = Duration::from_secs(30);
#[allow(dead_code)]
pub const ROBOT_SPEED_MULTIPLIER: i32 = 5;
#[allow(dead_code)]
pub const ROBOT_XY_PAUSE_MS: i32 = 150 / ROBOT_SPEED_MULTIPLIER;
#[allow(dead_code)]
pub const ROBOT_ROAD_PAUSE_MS: i32 = 100 / ROBOT_SPEED_MULTIPLIER;
pub const CHUNK_BUNDLE_MAX_BYTES: usize = 128 * 1024;
pub const CHUNK_BUNDLE_MAX_SUBPACKETS: usize = 192;
