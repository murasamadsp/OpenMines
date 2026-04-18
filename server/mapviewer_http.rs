//! HTTP-слой для браузерного map viewer и будущей админки: только чтение мира.
use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use serde::Serialize;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

use crate::game::GameState;

#[allow(dead_code)]
#[derive(Clone)]
struct MapHttpState {
    game: Arc<GameState>,
    /// `None` — без проверки (только для локальной разработки).
    token: Option<String>,
}

#[allow(dead_code)]
#[derive(Serialize)]
struct MapMeta {
    world_name: String,
    chunks_w: u32,
    chunks_h: u32,
    chunk_size: u32,
    cells_width: u32,
    cells_height: u32,
}

#[allow(dead_code)]
fn authorize(headers: &HeaderMap, token: Option<&String>) -> bool {
    let Some(expected) = token else {
        return true;
    };
    let bearer = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));
    let x = headers.get("X-Map-Token").and_then(|v| v.to_str().ok());
    match (bearer, x) {
        (Some(a), _) if a == expected => true,
        (_, Some(b)) if b == expected => true,
        _ => false,
    }
}

#[allow(dead_code)]
async fn map_meta(
    State(s): State<MapHttpState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    if !authorize(&headers, s.token.as_ref()) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let w = &s.game.world;
    let json = MapMeta {
        world_name: w.name().to_string(),
        chunks_w: w.chunks_w(),
        chunks_h: w.chunks_h(),
        chunk_size: 32,
        cells_width: w.cells_width(),
        cells_height: w.cells_height(),
    };
    match serde_json::to_vec(&json) {
        Ok(v) => Ok(([(axum::http::header::CONTENT_TYPE, "application/json")], v).into_response()),
        Err(e) => Ok((StatusCode::INTERNAL_SERVER_ERROR, format!("json: {e}")).into_response()),
    }
}

#[allow(dead_code)]
async fn map_chunk(
    State(s): State<MapHttpState>,
    Path((cx, cy)): Path<(u32, u32)>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    if !authorize(&headers, s.token.as_ref()) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let raw = s.game.world.read_chunk_cells(cx, cy);
    Ok((
        [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
        Bytes::from(raw),
    ))
}

#[allow(dead_code)]
pub async fn serve(
    game_state: Arc<GameState>,
    port: u16,
    token: Option<String>,
) -> anyhow::Result<()> {
    if token.is_none() {
        tracing::warn!(
            "M3R_MAPVIEWER_TOKEN не задан — /api/map/* доступен без авторизации (не для публичного VPS)"
        );
    }

    let state = MapHttpState {
        game: game_state,
        token,
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([axum::http::Method::GET])
        .allow_headers(Any);

    let app = Router::new()
        .route("/api/map/meta", get(map_meta))
        .route("/api/map/chunk/:cx/:cy", get(map_chunk))
        .with_state(state)
        .layer(cors);

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Map viewer / admin HTTP API: http://0.0.0.0:{port}/api/map/meta");
    axum::serve(listener, app).await?;
    Ok(())
}
