//! Периодические IO-задачи вне authoritative simulation runtime.

use crate::game::GameState;
use crate::world::WorldProvider;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::broadcast;

pub fn spawn_world_flush_loop(state: Arc<GameState>, mut shutdown: broadcast::Receiver<()>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_mins(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = shutdown.recv() => break,
            }
            state.prune_auth_failures_by_addr(Instant::now());
            crate::game::market::tick_crystal_prices(&state);
            let t0 = Instant::now();
            tracing::debug!(target: "tickprof", "WORLD FLUSH start");
            let state_c = state.clone();
            match tokio::task::spawn_blocking(move || state_c.world.flush()).await {
                Ok(Ok(flush_stats)) => {
                    crate::metrics::WORLD_FLUSH_DURABILITY_CHUNKS_TOTAL.inc_by(
                        u64::try_from(flush_stats.durability.dirty_chunks).unwrap_or(u64::MAX),
                    );
                    crate::metrics::WORLD_FLUSH_DURABILITY_RANGES_TOTAL
                        .inc_by(u64::try_from(flush_stats.durability.ranges).unwrap_or(u64::MAX));
                    crate::metrics::WORLD_FLUSH_DURABILITY_BYTES_TOTAL
                        .inc_by(u64::try_from(flush_stats.durability.bytes).unwrap_or(u64::MAX));
                }
                Ok(Err(error)) => tracing::error!(?error, "World flush error"),
                Err(error) => tracing::error!(?error, "World flush task failed"),
            }
            tracing::debug!(target: "tickprof", elapsed = ?t0.elapsed(), "WORLD FLUSH end");
            crate::metrics::WORLD_FLUSH_TOTAL.inc();
            crate::metrics::WORLD_FLUSH_SECONDS.observe(t0.elapsed().as_secs_f64());
        }
    });
}

pub fn spawn_online_count_loop(state: Arc<GameState>, mut shutdown: broadcast::Receiver<()>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_mins(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = shutdown.recv() => break,
            }
            broadcast_online_count(&state);
        }
    });
}

pub(super) fn broadcast_online_count(state: &GameState) {
    let player_ids = state.active_player_ids();
    let online_count = i32::try_from(player_ids.len()).unwrap_or(i32::MAX);
    let packet = crate::protocol::packets::online(online_count, 0);
    let wire = crate::net::session::wire::make_u_packet_bytes(packet.0, &packet.1);
    for player_id in player_ids {
        state.send_to_player(player_id, wire.clone());
    }
}
