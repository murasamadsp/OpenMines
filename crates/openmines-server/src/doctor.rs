use crate::{cli, config, game, migrations, world};
use anyhow::{Context as _, Result};
use std::path::Path;
use std::str::FromStr;

fn check_file(path: &str, label: &str) -> Result<()> {
    let meta = std::fs::metadata(path).with_context(|| format!("{label}: stat {path}"))?;
    if !meta.is_file() {
        anyhow::bail!("{label}: {path} is not a file");
    }
    Ok(())
}

fn check_state_dir(path: &Path) -> Result<()> {
    if path.exists() {
        if !path.is_dir() {
            anyhow::bail!(
                "state_dir exists but is not a directory: {}",
                path.display()
            );
        }
        println!("OK state_dir exists: {}", path.display());
    } else {
        println!("OK state_dir would be created: {}", path.display());
    }
    Ok(())
}

pub async fn run(args: &cli::Args, cfg: &config::Config) -> Result<()> {
    println!("OpenMines doctor");
    println!("config: {}", args.config);

    check_file(&args.config, "config")?;
    println!("OK config schema: {}", args.config);

    if cfg.port == args.admin_port {
        anyhow::bail!(
            "port collision: game TCP port and admin HTTP port are both {}",
            cfg.port
        );
    }
    println!("OK ports: game={} admin={}", cfg.port, args.admin_port);

    let state_dir = migrations::resolve_state_dir(&cfg.data_dir, args.data_dir.clone())?;
    check_state_dir(&state_dir)?;

    // Проверяем целостность и миграции базы данных, если она существует
    let db_path = state_dir.join("openmines.db");
    if db_path.exists() {
        check_file(db_path.to_str().unwrap(), "database")?;
        println!("OK database file exists: {}", db_path.display());

        let connection_str = format!("sqlite://{}", db_path.to_str().unwrap());
        let options = sqlx::sqlite::SqliteConnectOptions::from_str(&connection_str)?
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .read_only(true);

        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .context("Failed to connect to database in doctor mode")?;

        let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_one(&pool)
            .await
            .context("PRAGMA integrity_check query failed")?;

        if integrity != "ok" {
            anyhow::bail!("Database integrity check failed: {integrity}");
        }
        println!("OK database integrity: {integrity}");

        let applied_versions: Vec<i64> =
            sqlx::query_scalar("SELECT version FROM _sqlx_migrations WHERE success = 1")
                .fetch_all(&pool)
                .await
                .context("read applied SQLite migrations")?;

        let migrator = sqlx::migrate!("../openmines-shared/migrations");
        let mut missing_migrations = Vec::new();
        for m in migrator.iter() {
            if !applied_versions.contains(&m.version) {
                missing_migrations.push(format!("{} ({})", m.version, m.description));
            }
        }
        if !missing_migrations.is_empty() {
            anyhow::bail!("Database has pending migrations: {missing_migrations:?}");
        }
        println!(
            "OK database migrations: all {} migrations applied",
            migrator.iter().count()
        );
    } else {
        println!("OK database file does not exist yet (will be created on startup)");
    }

    check_file(&args.cells_config, "cells")?;
    let cell_defs = world::cells::CellDefs::load(&args.cells_config)
        .with_context(|| format!("load cells config {}", args.cells_config))?;
    println!("OK cells: {} entries", cell_defs.cells.len());

    check_file(&args.buildings_config, "buildings")?;
    game::buildings::load_buildings_config(&args.buildings_config)
        .with_context(|| format!("load buildings config {}", args.buildings_config))?;
    println!("OK buildings: {}", args.buildings_config);

    println!(
        "OK world geometry: {}x{} chunks ({}x{} cells)",
        cfg.world_chunks_w,
        cfg.world_chunks_h,
        cfg.world_chunks_w.saturating_mul(32),
        cfg.world_chunks_h.saturating_mul(32)
    );
    println!("doctor: OK");
    Ok(())
}
