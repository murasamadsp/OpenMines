mod lifecycle;
pub mod session;

use crate::game::GameState;
use anyhow::Result;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::broadcast;

pub async fn run(state: Arc<GameState>, shutdown: broadcast::Sender<()>) -> Result<()> {
    let port = state.config.port;
    let listener = TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    tracing::info!("TCP server listening on 0.0.0.0:{port}");

    lifecycle::spawn_world_flush_loop(state.clone(), shutdown.subscribe());
    lifecycle::spawn_player_dirty_flush_loop(state.clone(), shutdown.subscribe());
    lifecycle::spawn_game_tick_loop(state.clone(), shutdown.subscribe());

    let mut shutdown_rx = shutdown.subscribe();
    loop {
        let (stream, addr) = tokio::select! {
            accept = listener.accept() => accept?,
            _ = shutdown_rx.recv() => break,
        };
        tracing::info!("Connection from {addr}");
        let state = state.clone();
        let shutdown_rx = shutdown.subscribe();
        tokio::spawn(async move {
            if let Err(e) = session::handle(stream, addr, state, shutdown_rx).await {
                tracing::warn!("Session {addr} ended: {e}");
            }
        });
    }
    Ok(())
}
