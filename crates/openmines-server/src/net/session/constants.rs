/// Wire-protocol magic numbers from C# reference server (ServerTime.cs / Session.cs).
/// These are fixed by the client wire format and MUST NOT be changed.
pub const HEARTBEAT_RTT_BASE_MS: i32 = 201;
pub const HEARTBEAT_PONG_RESPONSE: i32 = 52;

/// HB bundle size limits — soft caps matching client's receive buffer expectations.
pub const CHUNK_BUNDLE_MAX_BYTES: usize = 128 * 1024;
pub const CHUNK_BUNDLE_MAX_SUBPACKETS: usize = 192;
