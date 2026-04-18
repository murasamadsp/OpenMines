mod config;
mod cron;
mod db;
mod game;
mod logging;
mod mapviewer_http;
mod metrics;
mod net;
mod ops_http;
mod protocol;
mod world;

use crate::world::WorldProvider;
use anyhow::Result;
use std::env;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use tokio::sync::broadcast;

/// Имя файла `SQLite` в каталоге состояния (`data_dir` / `M3R_DATA_DIR`).
const DB_FILENAME: &str = "openmines.db";

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = config::Config::load("config.json")?;
    let _logging_guard = logging::init(&cfg.logging)?;
    tracing::info!("Config loaded: world={}, port={}", cfg.world_name, cfg.port);

    let state_dir = resolve_state_dir(&cfg)?;
    std::fs::create_dir_all(&state_dir)?;
    migrate_legacy_state_files(&state_dir, &cfg.world_name)?;
    migrate_mines3_db_to_openmines(&state_dir);
    tracing::info!("Runtime state directory: {}", state_dir.display());

    let force_regenerate = env::args()
        .any(|arg| matches!(arg.as_str(), "--regen" | "--regen-world"))
        || env::var("M3R_REGEN_WORLD").ok().is_some_and(|s| {
            let t = s.trim().to_ascii_lowercase();
            matches!(t.as_str(), "1" | "true" | "yes" | "on")
        });
    if force_regenerate {
        remove_world_files(&cfg.world_name, &state_dir);
    }

    let cell_defs = world::cells::CellDefs::load("cells.json")?;
    tracing::info!("Loaded {} cell definitions", cell_defs.cells.len());

    let buildings_cfg_path = if Path::new("buildings.json").exists() {
        "buildings.json"
    } else {
        "data/buildings.json"
    };
    crate::game::load_buildings_config(buildings_cfg_path)?;
    tracing::info!("Loaded buildings configurations");

    let world = world::World::new(
        &cfg.world_name,
        cfg.world_chunks_w,
        cfg.world_chunks_h,
        cell_defs,
        &state_dir,
    )?;
    tracing::info!(
        "World ready: {}x{} cells ({}x{} chunks)",
        world.cells_width(),
        world.cells_height(),
        cfg.world_chunks_w,
        cfg.world_chunks_h
    );

    let database = db::Database::open(state_dir.join(DB_FILENAME))?;
    if force_regenerate {
        let n = database.delete_all_buildings()?;
        tracing::info!("World regen: cleared {n} building rows from DB (stale packs vs new map)");
    }
    bootstrap_grant_admin(&database)?;
    tracing::info!("Database ready");

    // Shutdown broadcast: SIGINT/SIGTERM → graceful stop pipeline.
    let (shutdown_tx, _) = broadcast::channel::<()>(16);
    let shutdown_tx_signal = shutdown_tx.clone();
    tokio::spawn(async move {
        // SIGINT (Ctrl+C)
        let ctrl_c = tokio::signal::ctrl_c();

        #[cfg(unix)]
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler");

        #[cfg(unix)]
        tokio::select! {
            _ = ctrl_c => {},
            _ = sigterm.recv() => {},
        }

        #[cfg(not(unix))]
        let _ = ctrl_c.await;

        let _ = shutdown_tx_signal.send(());
    });

    let game_state = game::GameState::new(
        std::sync::Arc::new(world),
        std::sync::Arc::new(database),
        cfg.clone(),
    );

    // Cron system.
    cron::CronManager::new(std::sync::Arc::clone(&game_state), shutdown_tx.clone()).spawn();

    {
        let mut ecs = game_state.ecs.write();
        ecs.insert_resource(game::GameStateResource(game_state.clone()));
    }

    // Mapviewer HTTP: пока не поднимаем (будет OIDC позже).

    // Ops HTTP (localhost): health/ready/metrics.
    ops_http::maybe_spawn(std::sync::Arc::clone(&game_state), shutdown_tx.clone());

    // Run TCP server until shutdown signal.
    let net_res = net::run(std::sync::Arc::clone(&game_state), shutdown_tx.clone()).await;

    // Final flush/save on shutdown.
    tracing::info!("Shutdown: saving all players and flushing world...");
    for entry in &game_state.active_players {
        if let Err(e) = game_state.db.save_player(&entry.value().data) {
            tracing::error!("Shutdown save failed for player {}: {e}", entry.key());
        }
    }
    if let Err(e) = game_state.world.flush() {
        tracing::error!("Shutdown world flush error: {e}");
    }

    net_res
}

/// Выставить роль админа (`role = 2`) по нику из `M3R_GRANT_ADMIN` (через запятую).
fn bootstrap_grant_admin(database: &db::Database) -> Result<()> {
    let Ok(raw) = env::var("M3R_GRANT_ADMIN") else {
        return Ok(());
    };
    for name in raw.split(',') {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        if let Some(p) = database.get_player_by_name(name)? {
            if database.set_player_role(p.id, db::Role::Admin)? {
                tracing::info!("M3R_GRANT_ADMIN: Role::Admin для id={} name={name:?}", p.id);
            }
        } else {
            tracing::warn!("M3R_GRANT_ADMIN: нет игрока name={name:?}");
        }
    }
    Ok(())
}

fn resolve_state_dir(cfg: &config::Config) -> Result<PathBuf> {
    if let Ok(p) = env::var("M3R_DATA_DIR") {
        return Ok(PathBuf::from(p));
    }
    Ok(env::current_dir()?.join(&cfg.data_dir))
}

/// Переносит БД и слои мира из рабочего каталога (старая схема) в `state_dir`.
fn migrate_legacy_state_files(state_dir: &Path, world_name: &str) -> Result<()> {
    let cwd = env::current_dir()?;
    let names = [
        "mines3.db".to_string(),
        "mines3.db-wal".to_string(),
        "mines3.db-shm".to_string(),
        "openmines.db".to_string(),
        "openmines.db-wal".to_string(),
        "openmines.db-shm".to_string(),
        format!("{world_name}.mapb"),
        format!("{world_name}_road.mapb"),
        format!("{world_name}_durability.mapb"),
    ];
    for name in names {
        let from = cwd.join(&name);
        let to = state_dir.join(&name);
        if to.exists() || !from.exists() {
            continue;
        }
        match std::fs::rename(&from, &to) {
            Ok(()) => tracing::info!("migrated {name} -> {}", state_dir.display()),
            Err(err) => tracing::warn!(
                "could not migrate {name} into {}: {err}",
                state_dir.display()
            ),
        }
    }
    Ok(())
}

/// Переименование legacy `mines3.db*` → `openmines.db*` внутри каталога состояния.
fn migrate_mines3_db_to_openmines(state_dir: &Path) {
    let new_main = state_dir.join(DB_FILENAME);
    if new_main.exists() {
        return;
    }
    let old_main = state_dir.join("mines3.db");
    if !old_main.exists() {
        return;
    }
    for (old_suffix, new_suffix) in [
        ("mines3.db", DB_FILENAME),
        ("mines3.db-wal", "openmines.db-wal"),
        ("mines3.db-shm", "openmines.db-shm"),
    ] {
        let from = state_dir.join(old_suffix);
        let to = state_dir.join(new_suffix);
        if from.exists() && !to.exists() {
            match std::fs::rename(&from, &to) {
                Ok(()) => tracing::info!("renamed {} -> {}", from.display(), to.display()),
                Err(e) => tracing::warn!("could not rename {}: {e}", from.display()),
            }
        }
    }
}

fn remove_world_files(world_name: &str, state_dir: &Path) {
    let files = [
        format!("{world_name}.mapb"),
        format!("{world_name}_road.mapb"),
        format!("{world_name}_durability.mapb"),
    ];
    for file in files {
        let path = state_dir.join(&file);
        if let Err(err) = std::fs::remove_file(&path) {
            if err.kind() != ErrorKind::NotFound {
                tracing::warn!("Failed to remove world file {}: {err}", path.display());
            }
        } else {
            tracing::info!("Removed {} for full world regeneration", path.display());
        }
    }
}
