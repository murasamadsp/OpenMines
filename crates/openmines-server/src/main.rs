pub use openmines_config as config;
pub use openmines_protocol as protocol;
pub use openmines_runtime::env_config;
pub use openmines_runtime::logging;
pub use openmines_runtime::metrics;
pub use openmines_runtime::time;
pub use openmines_storage as db;
pub use openmines_world as world;

mod admin;
mod bootstrap;
mod cli;
mod console;
mod doctor;
mod game;
mod migrations;
mod net;
mod persistence;
mod shutdown;
mod simulation_waker;
mod tasks;
#[cfg(test)]
mod test_support;

use crate::world::WorldProvider;
use anyhow::{Context as _, Result};
use tokio::sync::broadcast;

#[cfg(test)]
use std::path::{Path, PathBuf};

/// Имя файла `SQLite` в каталоге состояния (`data_dir` / `M3R_DATA_DIR`).
pub(crate) const DB_FILENAME: &str = "openmines.db";

#[cfg(test)]
pub(crate) fn test_config_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("server crate must live inside crates/ folder under workspace root")
        .join(relative)
}

#[allow(clippy::too_many_lines)]
#[tokio::main]
async fn main() -> Result<()> {
    // До `logging::init` паники не попадали в tracing — ловим в stderr сразу.
    logging::install_early_panic_hook();

    let args = cli::Args::parse_args();

    let mut cfg = config::Config::load(&args.config).map_err(|e| {
        println!(
            "[Main] CRITICAL: Failed to load config {}: {e}",
            args.config
        );
        e
    })?;

    // Override config options from CLI / environment overrides
    if let Some(port_override) = args.port {
        cfg.port = port_override;
    }
    if let Some(ref data_dir_override) = args.data_dir {
        cfg.data_dir = data_dir_override.clone();
    }

    if args.doctor {
        return doctor::run(&args, &cfg).await;
    }

    let admin_token = args.admin_token.clone().context(
        "M3R_ADMIN_TOKEN or --admin-token is required when starting the admin web server",
    )?;

    println!("[Main] Process started");
    println!("[Main] Config loaded, initializing logging...");
    let _logging_guard = logging::init(&cfg.logging)?;
    tracing::info!(world_name = %cfg.world_name, port = cfg.port, "Config loaded");

    let state_dir = migrations::resolve_state_dir(&cfg.data_dir, args.data_dir.clone())?;
    std::fs::create_dir_all(&state_dir)?;
    migrations::migrate_legacy_state_files(&state_dir, &cfg.world_name)?;
    migrations::migrate_mines3_db_to_openmines(&state_dir);
    tracing::info!(state_dir = %state_dir.display(), "Runtime state directory resolved");

    if args.regen {
        bootstrap::remove_world_files(&state_dir, &cfg.world_name);
    }

    let cell_defs = world::cells::CellDefs::load(&args.cells_config)?;
    tracing::info!(count = cell_defs.cells.len(), "Loaded cell definitions");

    crate::game::buildings::load_buildings_config(&args.buildings_config)?;
    tracing::info!("Loaded buildings configurations");

    let world = world::World::new(
        &cfg.world_name,
        cfg.world_chunks_w,
        cfg.world_chunks_h,
        cell_defs,
        &state_dir,
    )?;
    tracing::info!(
        width = world.cells_width(),
        height = world.cells_height(),
        chunks_w = cfg.world_chunks_w,
        chunks_h = cfg.world_chunks_h,
        "World ready"
    );

    let database = db::Database::open(state_dir.join(DB_FILENAME)).await?;
    if args.regen {
        bootstrap::regen_clear_world_state(&database, cfg.gameplay.spawn.x, cfg.gameplay.spawn.y)
            .await?;
    }
    bootstrap::bootstrap_grant_admin(&database, args.grant_admin.as_deref()).await?;
    tracing::info!("Database ready");

    // Shutdown broadcast: SIGINT/SIGTERM → graceful stop pipeline.
    let (shutdown_tx, _) = broadcast::channel::<()>(16);
    let shutdown_tx_signal = shutdown_tx.clone();
    let use_ctrl_c = args.use_ctrl_c;
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            // В Docker `ctrl_c()` иногда готовится сразу — сервер выходит до accept. В compose: `M3R_USE_CTRL_C=0`.
            // CF-4: не паникуем если signal() падает (rootless Docker / restricted ns).
            // Фолбэк: только SIGINT (Ctrl+C), SIGTERM игнорируется в этом сеансе.
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Err(e) => {
                    tracing::warn!(error = ?e, "Failed to register SIGTERM handler; only SIGINT will trigger shutdown");
                    let _ = tokio::signal::ctrl_c().await;
                    tracing::info!("Shutdown signal received (SIGINT)");
                    let _ = shutdown_tx_signal.send(());
                    let _ = tokio::signal::ctrl_c().await;
                    tracing::error!("Second SIGINT received; forcing process exit");
                    std::process::exit(130);
                }
                Ok(mut sigterm) => {
                    if use_ctrl_c {
                        let first_was_sigterm = tokio::select! {
                            _ = tokio::signal::ctrl_c() => false,
                            _ = sigterm.recv() => true,
                        };
                        tracing::info!(
                            signal = if first_was_sigterm {
                                "SIGTERM"
                            } else {
                                "SIGINT"
                            },
                            "Shutdown signal received"
                        );
                        let _ = shutdown_tx_signal.send(());
                        tokio::select! {
                            _ = tokio::signal::ctrl_c() => {
                                tracing::error!("Second SIGINT received; forcing process exit");
                                std::process::exit(130);
                            }
                            _ = sigterm.recv() => {
                                tracing::error!("Second SIGTERM received; forcing process exit");
                                std::process::exit(143);
                            }
                        }
                    } else {
                        let _ = sigterm.recv().await;
                        tracing::info!("Shutdown signal received (SIGTERM)");
                        let _ = shutdown_tx_signal.send(());
                    }
                }
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("Shutdown signal received (Ctrl+C)");
            let _ = shutdown_tx_signal.send(());
            let _ = tokio::signal::ctrl_c().await;
            tracing::error!("Second Ctrl+C received; forcing process exit");
            std::process::exit(130);
        }
    });

    // 1:1 C# CreateSpawns: стартовые здания + площадка золотой дороги при пустой
    // таблице зданий (fresh / после --regen).
    bootstrap::create_spawns(
        &database,
        &world,
        cfg.gameplay.spawn.x,
        cfg.gameplay.spawn.y,
    )
    .await?;

    let game_state = game::GameState::new(
        std::sync::Arc::new(world),
        std::sync::Arc::new(database),
        cfg.clone(),
    )
    .await?;

    // Background tasks (cron + lifecycle loops + auction loop)
    let background_tasks = tasks::spawn_background_tasks(&game_state, &shutdown_tx);

    // Spawning console REPL
    let repl_state = std::sync::Arc::clone(&game_state);
    let repl_shutdown = shutdown_tx.clone();
    tokio::spawn(async move {
        if let Err(e) = console::run_repl(repl_state, repl_shutdown).await {
            tracing::error!(error = ?e, "REPL console error");
        }
    });

    // Spawning admin web server
    let admin_state = std::sync::Arc::clone(&game_state);
    let admin_shutdown_rx = shutdown_tx.subscribe();
    let admin_port = args.admin_port;
    tokio::spawn(async move {
        if let Err(e) =
            net::web::run_web_server(admin_state, admin_shutdown_rx, admin_port, admin_token).await
        {
            tracing::error!(error = ?e, "Admin web server error");
        }
    });

    // Run TCP server until shutdown signal.
    let net_res = net::run(std::sync::Arc::clone(&game_state), shutdown_tx.clone()).await;
    match &net_res {
        Ok(()) => tracing::info!("net::run finished Ok (accept loop ended, e.g. shutdown)"),
        Err(e) => tracing::error!(error = ?e, "net::run finished with error (process may exit)"),
    }

    let _ = shutdown_tx.send(());
    background_tasks.shutdown().await;
    shutdown::shutdown_flush(&game_state).await;
    net_res
}

#[cfg(test)]
mod benchmarks {
    use std::sync::Arc;
    use std::time::Instant;

    const BENCH_N: u32 = 500;
    fn create_minimal_state() -> Arc<crate::game::GameState> {
        create_minimal_state_with_gameplay(
            "bench_dynamic",
            "bench-player-dynamic",
            crate::config::GameplayConfig::runtime_baseline(),
        )
    }

    fn create_minimal_state_with_gameplay(
        label: &str,
        username: &str,
        gameplay: crate::config::GameplayConfig,
    ) -> Arc<crate::game::GameState> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let test =
                crate::test_support::ServerTestHarness::with_gameplay(label, username, gameplay)
                    .await;
            test.state.clone()
        })
    }

    #[test]
    #[ignore = "benchmark — запускать вручную через --ignored"]
    fn bench_tick_empty_world() {
        let state = create_minimal_state();
        let rt = tokio::runtime::Runtime::new().unwrap();

        let start = Instant::now();
        for _ in 0..BENCH_N {
            let mut ecs = state.ecs.write();
            for gs in &state.schedules {
                let mut schedule = gs.schedule.write();
                schedule.run(&mut ecs);
            }
            // drain queues
            let _bc = std::mem::take(&mut ecs.resource_mut::<crate::game::BroadcastQueue>().0);
            let _pa = std::mem::take(&mut ecs.resource_mut::<crate::game::ProgrammatorQueue>().0);
            let _pr = std::mem::take(&mut ecs.resource_mut::<crate::game::PackResendQueue>().0);
            drop(ecs);
        }
        drop(rt);
        let elapsed = start.elapsed();
        let per_tick = elapsed / BENCH_N;

        eprintln!(
            "BENCH: {BENCH_N} ticks = {elapsed:?}  avg={per_tick:?}  ticks/s={:.0}",
            f64::from(BENCH_N) / elapsed.as_secs_f64()
        );

        assert!(
            per_tick < std::time::Duration::from_millis(5),
            "tick too slow: {per_tick:?} (N={BENCH_N})"
        );
    }

    #[test]
    fn test_dynamic_schedule_interval_change() {
        let state = create_minimal_state();
        let physics = state
            .schedules
            .iter()
            .find(|s| s.name == "physics")
            .expect("physics schedule must exist");

        assert_eq!(
            physics
                .interval_ms
                .load(std::sync::atomic::Ordering::Relaxed),
            400
        );

        assert!(state.set_schedule_interval("physics", 250));
        assert_eq!(
            physics
                .interval_ms
                .load(std::sync::atomic::Ordering::Relaxed),
            250
        );

        assert!(state.set_schedule_interval("physics", 0));
        assert_eq!(
            physics
                .interval_ms
                .load(std::sync::atomic::Ordering::Relaxed),
            0
        );

        assert!(!state.set_schedule_interval("missing", 100));
    }

    #[test]
    fn schedule_intervals_come_from_config() {
        let mut gameplay = crate::config::GameplayConfig::runtime_baseline();
        gameplay.schedules.physics_ms = 321;
        gameplay.schedules.programmator_ms = 654;

        let state =
            create_minimal_state_with_gameplay("bench_config", "bench-player-config", gameplay);
        let interval = |name: &str| {
            state
                .schedules
                .iter()
                .find(|s| s.name == name)
                .expect("schedule must exist")
                .interval_ms
                .load(std::sync::atomic::Ordering::Relaxed)
        };

        assert_eq!(interval("physics"), 321);
        assert_eq!(interval("programmator"), 654);
    }
}
