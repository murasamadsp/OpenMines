use crate::net::session::constants;
use crate::protocol::packets::{PongClient, ping};
use std::time::{Duration, Instant};

pub struct SessionHeartbeat {
    pong_at: Instant,
    pi_sent_at: Option<Instant>,
    rtt_ms: i32,
    pong_client_time: i32,
}

impl SessionHeartbeat {
    pub const fn new(now: Instant) -> Self {
        Self {
            pong_at: now,
            pi_sent_at: None,
            rtt_ms: constants::HEARTBEAT_RTT_BASE_MS,
            pong_client_time: 0,
        }
    }

    pub const fn reset_liveness(&mut self, now: Instant) {
        self.pong_at = now;
    }

    pub fn is_timed_out(&self, now: Instant, timeout: Duration) -> bool {
        now.saturating_duration_since(self.pong_at) > timeout
    }

    pub fn record_pong(&mut self, pong: &PongClient, now: Instant) {
        self.pong_at = now;
        if !matches!(
            pong.response,
            constants::HEARTBEAT_HANDSHAKE_PONG_RESPONSE | constants::HEARTBEAT_PONG_RESPONSE
        ) {
            tracing::warn!(
                expected = constants::HEARTBEAT_PONG_RESPONSE,
                actual = pong.response,
                "Unexpected PO response id"
            );
        }
        if let Some(sent) = self.pi_sent_at {
            let rtt_ms = now.saturating_duration_since(sent).as_millis();
            self.rtt_ms = i32::try_from(rtt_ms).unwrap_or(i32::MAX).clamp(1, 99_999);
        }
        self.pong_client_time = pong.current_time;
    }

    pub fn next_ping_packet(&mut self, now: Instant) -> (&'static str, Vec<u8>) {
        let since_pong_ms = i32::try_from(now.saturating_duration_since(self.pong_at).as_millis())
            .unwrap_or(i32::MAX);
        let client_time = self
            .pong_client_time
            .saturating_add(since_pong_ms)
            .saturating_add(1);
        let text = format!("{} ", self.rtt_ms);
        self.pi_sent_at = Some(now);
        ping(constants::HEARTBEAT_PONG_RESPONSE, client_time, &text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_pong_zero_is_valid_liveness() {
        let now = Instant::now();
        let mut heartbeat = SessionHeartbeat::new(now);
        let pong = PongClient {
            response: constants::HEARTBEAT_HANDSHAKE_PONG_RESPONSE,
            current_time: 123,
        };

        heartbeat.record_pong(&pong, now + Duration::from_millis(10));

        assert!(!heartbeat.is_timed_out(now + Duration::from_millis(11), Duration::from_millis(1)));
        assert_eq!(heartbeat.pong_client_time, 123);
    }
}
