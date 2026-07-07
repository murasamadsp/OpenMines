use crate::db::{events::EventRow, players::Role};
use crate::game::{ActiveEvent, GameState};
use crate::world::WorldProvider;
use anyhow::Result;
use axum::{
    Extension, Json, Router,
    body::Body,
    extract::{Path, Request, State},
    http::{Response, StatusCode, Uri, header},
    middleware::{self, Next},
    response::IntoResponse,
    routing::{delete, get, post},
};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(RustEmbed)]
#[folder = "admin/"]
struct Assets;

#[derive(Serialize)]
struct ServerStats {
    online_count: usize,
    active_players: Vec<ActivePlayerInfo>,
    market_prices: Vec<i64>,
    cost_mod: Vec<i64>,
    active_events: Vec<ActiveEvent>,
}

#[derive(Serialize)]
struct ActivePlayerInfo {
    id: i32,
    name: String,
    x: i32,
    y: i32,
    health: i32,
    max_health: i32,
    crystals: [i64; 6],
    money: i64,
    creds: i64,
    role: i32,
}

#[derive(Serialize)]
struct MapData {
    width: u32,
    height: u32,
    players: Vec<MapPlayer>,
    buildings: Vec<MapBuilding>,
}

#[derive(Serialize)]
struct MapPlayer {
    id: i32,
    name: String,
    x: i32,
    y: i32,
}

#[derive(Serialize)]
struct MapBuilding {
    x: i32,
    y: i32,
    pack_type: String,
    hp: i32,
    max_hp: i32,
    clan_id: i32,
}

#[derive(Deserialize)]
struct AuthRequest {
    token: String,
}

#[derive(Deserialize)]
struct MarketUpdate {
    cost_mod: [i64; 6],
}

#[derive(Deserialize)]
struct RoleUpdate {
    role: i32,
}

async fn auth_middleware(
    Extension(token): Extension<String>,
    req: Request,
    next: Next,
) -> Result<Response<Body>, StatusCode> {
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "));

    let query_token = req.uri().query().and_then(|q| {
        q.split('&')
            .find(|p| p.starts_with("token="))
            .map(|p| &p["token=".len()..])
    });

    let admin_header = req
        .headers()
        .get("X-Admin-Token")
        .and_then(|h| h.to_str().ok());

    let authenticated =
        auth_header == Some(&token) || admin_header == Some(&token) || query_token == Some(&token);

    if authenticated {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

async fn handle_auth(
    Extension(token): Extension<String>,
    Json(payload): Json<AuthRequest>,
) -> impl IntoResponse {
    if payload.token == token {
        (StatusCode::OK, Json(serde_json::json!({ "success": true })))
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "success": false })),
        )
    }
}

async fn handle_stats(State(state): State<Arc<GameState>>) -> impl IntoResponse {
    let mut active_players = Vec::new();
    let ecs = state.ecs.read();
    for pid in state.active_player_ids() {
        if let Some(entity) = state.get_player_entity(pid)
            && let Some(pos) = ecs.get::<crate::game::player::PlayerPosition>(entity)
            && let Some(p_stats) = ecs.get::<crate::game::player::PlayerStats>(entity)
            && let Some(meta) = ecs.get::<crate::game::player::PlayerMetadata>(entity)
        {
            active_players.push(ActivePlayerInfo {
                id: pid.0,
                name: meta.name.clone(),
                x: pos.x,
                y: pos.y,
                health: p_stats.health,
                max_health: p_stats.max_health,
                crystals: p_stats.crystals,
                money: p_stats.money,
                creds: p_stats.creds,
                role: p_stats.role,
            });
        }
    }

    let mut market_prices = Vec::new();
    for i in 0..6 {
        market_prices.push(crate::game::economy::market::get_crystal_cost(&state, i));
    }
    let cost_mod = state.crystal_economy.lock().cost_mod.to_vec();
    let events = state.active_events.read().list.clone();

    Json(ServerStats {
        online_count: state.online_count(),
        active_players,
        market_prices,
        cost_mod,
        active_events: events,
    })
}

async fn handle_map(State(state): State<Arc<GameState>>) -> impl IntoResponse {
    let width = state.world.cells_width();
    let height = state.world.cells_height();

    let mut ecs = state.ecs.write();

    let mut players = Vec::new();
    for pid in state.active_player_ids() {
        if let Some(entity) = state.get_player_entity(pid)
            && let Some(pos) = ecs.get::<crate::game::player::PlayerPosition>(entity)
            && let Some(meta) = ecs.get::<crate::game::player::PlayerMetadata>(entity)
        {
            players.push(MapPlayer {
                id: pid.0,
                name: meta.name.clone(),
                x: pos.x,
                y: pos.y,
            });
        }
    }

    let mut b_query = ecs.query::<(
        &crate::game::buildings::GridPosition,
        &crate::game::buildings::BuildingMetadata,
        &crate::game::buildings::BuildingStats,
        &crate::game::buildings::BuildingOwnership,
    )>();

    let mut buildings = Vec::new();
    for (grid_pos, metadata, stats, ownership) in b_query.iter(&ecs) {
        buildings.push(MapBuilding {
            x: grid_pos.x,
            y: grid_pos.y,
            pack_type: format!("{:?}", metadata.pack_type),
            hp: stats.hp,
            max_hp: stats.max_hp,
            clan_id: ownership.clan_id,
        });
    }

    drop(ecs);

    Json(MapData {
        width,
        height,
        players,
        buildings,
    })
}

async fn handle_kick(
    State(state): State<Arc<GameState>>,
    Path(pid_val): Path<i32>,
) -> impl IntoResponse {
    let pid = crate::game::player::PlayerId(pid_val);
    if state.kick_channels.remove(&pid).is_some() {
        (
            StatusCode::OK,
            Json(serde_json::json!({ "success": true, "message": "Player kicked" })),
        )
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "success": false, "message": "Player not found or offline" })),
        )
    }
}

async fn handle_set_role(
    State(state): State<Arc<GameState>>,
    Path(pid_val): Path<i32>,
    Json(payload): Json<RoleUpdate>,
) -> impl IntoResponse {
    let role = Role::from_db(payload.role);
    match state.db.set_player_role(pid_val, role).await {
        Ok(true) => {
            let pid = crate::game::player::PlayerId(pid_val);
            state.modify_player(pid, |ecs, entity| {
                if let Some(mut stats) = ecs.get_mut::<crate::game::player::PlayerStats>(entity) {
                    stats.role = role as i32;
                }
                if let Some(mut flags) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) {
                    flags.dirty = true;
                }
                Some(())
            });
            (
                StatusCode::OK,
                Json(serde_json::json!({ "success": true, "role": role as i32 })),
            )
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "success": false, "message": "Player not found" })),
        ),
        Err(e) => {
            tracing::error!(player_id = pid_val, role = payload.role, error = ?e, "Failed to set player role");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "success": false, "message": e.to_string() })),
            )
        }
    }
}

async fn handle_get_events(State(state): State<Arc<GameState>>) -> impl IntoResponse {
    let events = state.active_events.read().list.clone();
    Json(events)
}

async fn handle_save_event(
    State(state): State<Arc<GameState>>,
    Json(event): Json<ActiveEvent>,
) -> impl IntoResponse {
    let row = EventRow {
        id: event.id.clone(),
        title: event.title.clone(),
        starts_at: event.starts_at,
        ends_at: event.ends_at,
        config_json: serde_json::json!({
            "xp_mult": event.xp_mult,
            "drop_mult": event.drop_mult,
        })
        .to_string(),
    };

    match state.db.save_event(&row).await {
        Ok(()) => {
            // Update in-memory state
            let mut active = state.active_events.write();
            active.list.retain(|e| e.id != event.id);
            active.list.push(event);
            drop(active);
            (StatusCode::OK, Json(serde_json::json!({ "success": true })))
        }
        Err(e) => {
            tracing::error!(error = ?e, "Failed to save event to database");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "success": false, "message": e.to_string() })),
            )
        }
    }
}

async fn handle_delete_event(
    State(state): State<Arc<GameState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.db.delete_event(&id).await {
        Ok(found) => {
            if found {
                // Update in-memory state
                let mut active = state.active_events.write();
                active.list.retain(|e| e.id != id);
                drop(active);
                (StatusCode::OK, Json(serde_json::json!({ "success": true })))
            } else {
                (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({ "success": false, "message": "Event not found" })),
                )
            }
        }
        Err(e) => {
            tracing::error!(error = ?e, "Failed to delete event from database");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "success": false, "message": e.to_string() })),
            )
        }
    }
}

async fn handle_update_market(
    State(state): State<Arc<GameState>>,
    Json(payload): Json<MarketUpdate>,
) -> impl IntoResponse {
    let mut eco = state.crystal_economy.lock();
    eco.cost_mod = payload.cost_mod;
    drop(eco);
    (StatusCode::OK, Json(serde_json::json!({ "success": true })))
}

async fn static_handler(uri: Uri) -> impl IntoResponse {
    let mut path = uri.path().trim_start_matches('/').to_string();
    if path.is_empty() {
        path = "index.html".to_string();
    }

    match Assets::get(&path) {
        Some(content) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            Response::builder()
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(content.data))
                .unwrap()
        }
        None => match Assets::get("index.html") {
            Some(content) => Response::builder()
                .header(header::CONTENT_TYPE, "text/html")
                .body(Body::from(content.data))
                .unwrap(),
            None => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("404 Not Found"))
                .unwrap(),
        },
    }
}

pub async fn run_web_server(
    state: Arc<GameState>,
    mut shutdown: tokio::sync::broadcast::Receiver<()>,
    port: u16,
    token: String,
) -> Result<()> {
    let api_routes = Router::new()
        .route("/stats", get(handle_stats))
        .route("/map", get(handle_map))
        .route("/players/:id/kick", post(handle_kick))
        .route("/players/:id/role", post(handle_set_role))
        .route("/events", get(handle_get_events).post(handle_save_event))
        .route("/events/:id", delete(handle_delete_event))
        .route("/market", post(handle_update_market))
        .route_layer(middleware::from_fn(auth_middleware))
        .with_state(state.clone())
        .layer(Extension(token.clone()));

    let app = Router::new()
        .nest("/api", api_routes)
        .route("/api/auth", post(handle_auth).layer(Extension(token)))
        .fallback(static_handler);

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(port, "Admin HTTP server listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = shutdown.recv().await;
            tracing::info!("Admin HTTP server shutting down");
        })
        .await?;

    Ok(())
}
