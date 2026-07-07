use crate::{cli, config, game, migrations, world};
use anyhow::{Context as _, Result};
use std::path::Path;

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

pub fn run(args: &cli::Args, cfg: &config::Config) -> Result<()> {
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
