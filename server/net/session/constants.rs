use std::time::Duration;

/// Reference server keeps `PI` cadence based on client timestamps, with delayed
/// response around 200ms and fallback ping every 10s if no `PO` received.
pub const HEARTBEAT_RTT_BASE_MS: i32 = 201;
pub const HEARTBEAT_FALLBACK_INTERVAL: Duration = Duration::from_secs(10);
pub const HEARTBEAT_DISCONNECT_TIMEOUT: Duration = Duration::from_secs(30);
pub const ROBOT_SPEED_MULTIPLIER: i32 = 5;
pub const ROBOT_XY_PAUSE_MS: i32 = 150 / ROBOT_SPEED_MULTIPLIER;
pub const ROBOT_ROAD_PAUSE_MS: i32 = 100 / ROBOT_SPEED_MULTIPLIER;
pub const CHUNK_BUNDLE_MAX_BYTES: usize = 128 * 1024;
pub const CHUNK_BUNDLE_MAX_SUBPACKETS: usize = 192;
