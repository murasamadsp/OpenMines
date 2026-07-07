use std::time::Duration;

/// Reference server keeps `PI` cadence based on client timestamps.
pub const HEARTBEAT_RTT_BASE_MS: i32 = 201;
pub const HEARTBEAT_PONG_RESPONSE: i32 = 52;
pub const HEARTBEAT_DISCONNECT_TIMEOUT: Duration = Duration::from_secs(30);
pub const CHUNK_BUNDLE_MAX_BYTES: usize = 128 * 1024;
pub const CHUNK_BUNDLE_MAX_SUBPACKETS: usize = 192;
