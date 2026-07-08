pub mod session;
pub mod web;

use crate::game::GameState;
use crate::metrics;
use anyhow::Result;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;

pub async fn run(state: Arc<GameState>, shutdown: broadcast::Sender<()>) -> Result<()> {
    let port = state.config.port;
    let listener = TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    tracing::info!(port, "TCP server listening");

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
        metrics::TCP_CONNECTIONS_TOTAL.inc();
        metrics::TCP_CONNECTIONS_CURRENT.inc();
        let state = state.clone();
        let _shutdown_rx = shutdown.subscribe();
        tokio::spawn(async move {
            if let Err(e) = session::handle(Arc::clone(&state), stream, addr).await {
                if is_normal_disconnect(&e) {
                    tracing::debug!(client_ip = %addr, error = ?e, "Session disconnected");
                } else {
                    tracing::warn!(client_ip = %addr, error = ?e, "Session ended");
                }
            }
            metrics::TCP_CONNECTIONS_CURRENT.dec();
        });
    }
    Ok(())
}

fn is_normal_disconnect(error: &anyhow::Error) -> bool {
    error.downcast_ref::<std::io::Error>().is_some_and(|io| {
        matches!(
            io.kind(),
            std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::BrokenPipe
                | std::io::ErrorKind::UnexpectedEof
        )
    })
}
