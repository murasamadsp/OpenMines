mod auction;
mod lifecycle;
pub mod session;

use crate::game::GameState;
use anyhow::Result;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;

pub async fn run(state: Arc<GameState>, shutdown: broadcast::Sender<()>) -> Result<()> {
    let port = state.config.port;
    let listener = TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    tracing::info!(port, "TCP server listening");

    lifecycle::spawn_world_flush_loop(state.clone(), shutdown.subscribe());
    lifecycle::spawn_online_count_loop(state.clone(), shutdown.subscribe());
    lifecycle::spawn_player_dirty_flush_loop(state.clone(), shutdown.subscribe());
    lifecycle::spawn_building_dirty_flush_loop(state.clone(), shutdown.subscribe());
    lifecycle::spawn_game_tick_loop(state.clone(), shutdown.clone());
    auction::spawn_auction_finalize_loop(state.clone(), shutdown.subscribe());

    let mut shutdown_rx = shutdown.subscribe();
    loop {
        let (stream, addr) = tokio::select! {
            accept = listener.accept() => match accept {
                Ok(pair) => pair,
                Err(e) => {
                    // Критично: здесь раньше стояло `accept?` — одна ошибка accept()
                    // (EMFILE/ENOMEM/временный сбой) завершала весь процесс → «сервер упал».
                    tracing::error!(error = ?e, "TCP accept failed; retrying");
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    continue;
                }
            },
            recv = shutdown_rx.recv() => {
                match recv {
                    Ok(()) => break,
                    Err(RecvError::Lagged(_)) => {
                        tracing::debug!("shutdown broadcast lagged; continue accepting");
                        continue;
                    }
                    Err(RecvError::Closed) => {
                        tracing::info!("shutdown broadcast closed");
                        break;
                    }
                }
            }
        };
        tracing::info!(client_ip = %addr, "New connection accepted");
        let state = state.clone();
        let _shutdown_rx = shutdown.subscribe();
        tokio::spawn(async move {
            if let Err(e) = session::handle(Arc::clone(&state), stream, addr).await {
                tracing::warn!(client_ip = %addr, error = ?e, "Session ended");
            }
        });
    }
    Ok(())
}
