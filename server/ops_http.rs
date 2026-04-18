use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::game::GameState;
use crate::world::WorldProvider;

#[derive(Clone)]
struct OpsState {
    game: Arc<GameState>,
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok\n")
}

async fn readyz(State(_ops_state): State<OpsState>) -> impl IntoResponse {
    // Минимальная readiness: если процесс жив и стейт поднят — считаем ready.
    // Дальше можно ужесточать (например, проверять доступность DB/flush).
    (StatusCode::OK, "ready\n")
}

async fn metrics() -> impl IntoResponse {
    let body = crate::metrics::gather_text();
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4",
        )],
        body,
    )
}

#[derive(serde::Serialize)]
struct CellInspect {
    x: i32,
    y: i32,
    solid: u8,
    road: u8,
    effective: u8,
    durability: f32,
    def_durability: f32,
    is_diggable: bool,
    is_empty: bool,
}

async fn inspect_cell(
    State(ops_state): State<OpsState>,
    axum::extract::Query(url_query): axum::extract::Query<
        std::collections::HashMap<String, String>,
    >,
) -> Result<impl IntoResponse, StatusCode> {
    let pos_x = url_query
        .get("x")
        .and_then(|v| v.parse::<i32>().ok())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let pos_y = url_query
        .get("y")
        .and_then(|v| v.parse::<i32>().ok())
        .ok_or(StatusCode::BAD_REQUEST)?;
    if !ops_state.game.world.valid_coord(pos_x, pos_y) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let solid = ops_state.game.world.get_solid_cell(pos_x, pos_y);
    let road = ops_state.game.world.get_road_cell(pos_x, pos_y);
    let effective = ops_state.game.world.get_cell(pos_x, pos_y);
    let durability = ops_state.game.world.get_durability(pos_x, pos_y);
    let cell_defs = ops_state.game.world.cell_defs();
    let def = cell_defs.get(effective);
    let payload = CellInspect {
        x: pos_x,
        y: pos_y,
        solid,
        road,
        effective,
        durability,
        def_durability: def.durability,
        is_diggable: def.is_diggable(),
        is_empty: def.cell_is_empty(),
    };
    Ok((
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        serde_json::to_vec(&payload).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    ))
}

async fn reseed_durability(
    State(ops_state): State<OpsState>,
    axum::extract::Query(url_query): axum::extract::Query<
        std::collections::HashMap<String, String>,
    >,
) -> Result<impl IntoResponse, StatusCode> {
    let pos_x = url_query
        .get("x")
        .and_then(|v| v.parse::<i32>().ok())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let pos_y = url_query
        .get("y")
        .and_then(|v| v.parse::<i32>().ok())
        .ok_or(StatusCode::BAD_REQUEST)?;
    if !ops_state.game.world.valid_coord(pos_x, pos_y) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let cell = ops_state.game.world.get_cell(pos_x, pos_y);
    let cell_defs = ops_state.game.world.cell_defs();
    let def = cell_defs.get(cell);
    ops_state
        .game
        .world
        .set_durability(pos_x, pos_y, def.durability);
    Ok((StatusCode::OK, "ok\n"))
}

async fn inspect_pack_at(
    State(ops_state): State<OpsState>,
    axum::extract::Query(url_query): axum::extract::Query<
        std::collections::HashMap<String, String>,
    >,
) -> Result<impl IntoResponse, StatusCode> {
    let pos_x = url_query
        .get("x")
        .and_then(|v| v.parse::<i32>().ok())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let pos_y = url_query
        .get("y")
        .and_then(|v| v.parse::<i32>().ok())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let Some((px, py)) = ops_state.game.find_pack_covering(pos_x, pos_y) else {
        return Err(StatusCode::NOT_FOUND);
    };
    let pack_data = ops_state
        .game
        .get_pack_at(px, py)
        .ok_or(StatusCode::NOT_FOUND)?
        .value()
        .clone();
    let payload = serde_json::to_vec(&serde_json::json!({
        "id": pack_data.id,
        "type": format!("{:?}", pack_data.pack_type),
        "x": pack_data.x,
        "y": pack_data.y,
        "owner_id": pack_data.owner_id,
        "clan_id": pack_data.clan_id,
        "charge": pack_data.charge,
        "max_charge": pack_data.max_charge,
        "hp": pack_data.hp,
        "max_hp": pack_data.max_hp,
    }))
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        payload,
    ))
}

fn enabled() -> bool {
    std::env::var("M3R_OPS_HTTP").ok().is_some_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn allow_remote() -> bool {
    std::env::var("M3R_OPS_HTTP_BIND_PUBLIC")
        .ok()
        .is_some_and(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
}

fn listen_addr() -> SocketAddr {
    let port = std::env::var("M3R_OPS_HTTP_PORT")
        .ok()
        .and_then(|v| v.trim().parse::<u16>().ok())
        .unwrap_or(8092);
    let ip = if allow_remote() {
        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
    } else {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    };
    SocketAddr::new(ip, port)
}

pub fn maybe_spawn(game: Arc<GameState>, shutdown: broadcast::Sender<()>) {
    if !enabled() {
        return;
    }
    let addr = listen_addr();
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics))
        .route("/ops/world/cell", get(inspect_cell))
        .route("/ops/world/reseed_durability", get(reseed_durability))
        .route("/ops/packs/at", get(inspect_pack_at))
        .with_state(OpsState { game });

    tokio::spawn(async move {
        tracing::info!("Ops HTTP listening on http://{addr}");
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                tracing::error!("Ops HTTP bind failed: {e}");
                return;
            }
        };
        let mut shutdown_rx = shutdown.subscribe();
        let serve = axum::serve(listener, app);
        tokio::select! {
            r = serve => {
                if let Err(e) = r {
                    tracing::error!("Ops HTTP server error: {e}");
                }
            }
            _ = shutdown_rx.recv() => {}
        }
    });
}
