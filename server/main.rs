pub use openmines_shared::config;
pub use openmines_shared::db;
pub use openmines_shared::env_config;
pub use openmines_shared::logging;
pub use openmines_shared::metrics;
pub use openmines_shared::protocol;
pub use openmines_shared::world;

mod console;
mod cron;
mod game;
mod net;

use crate::world::WorldProvider;
use anyhow::Result;
use std::env;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use tokio::sync::broadcast;

/// Имя файла `SQLite` в каталоге состояния (`data_dir` / `M3R_DATA_DIR`).
const DB_FILENAME: &str = "openmines.db";

#[cfg(test)]
pub(crate) fn test_config_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("server crate must live inside workspace root")
        .join(relative)
}

#[allow(clippy::too_many_lines)]
#[tokio::main]
async fn main() -> Result<()> {
    // До `logging::init` паники не попадали в tracing — ловим в stderr сразу.
    logging::install_early_panic_hook();
    let force_regenerate =
        parse_regen_flag_from(env::args().skip(1), env::var("M3R_REGEN_WORLD").ok())
            .unwrap_or_else(|e| e.exit());

    println!("[Main] Process started");
    let mut cfg = config::Config::load("configs/config.json").map_err(|e| {
        println!("[Main] CRITICAL: Failed to load configs/config.json: {e}");
        e
    })?;
    cfg.port = parse_port_override_from(env::var("M3R_PORT").ok(), cfg.port)?;
    let use_ctrl_c =
        env_config::parse_bool_env_or("M3R_USE_CTRL_C", env::var("M3R_USE_CTRL_C").ok(), true)?;
    println!("[Main] Config loaded, initializing logging...");
    let _logging_guard = logging::init(&cfg.logging)?;
    tracing::info!(world_name = %cfg.world_name, port = cfg.port, "Config loaded");

    let state_dir = resolve_state_dir(&cfg)?;
    std::fs::create_dir_all(&state_dir)?;
    migrate_legacy_state_files(&state_dir, &cfg.world_name)?;
    migrate_mines3_db_to_openmines(&state_dir);
    tracing::info!(state_dir = %state_dir.display(), "Runtime state directory resolved");

    if force_regenerate {
        remove_world_files(&state_dir);
    }

    let cell_defs = world::cells::CellDefs::load("configs/cells.json")?;
    tracing::info!(count = cell_defs.cells.len(), "Loaded cell definitions");

    crate::game::buildings::load_buildings_config("configs/buildings.json")?;
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
    if force_regenerate {
        regen_clear_world_state(&database).await?;
    }
    bootstrap_grant_admin(&database).await?;
    tracing::info!("Database ready");

    // Shutdown broadcast: SIGINT/SIGTERM → graceful stop pipeline.
    let (shutdown_tx, _) = broadcast::channel::<()>(16);
    let shutdown_tx_signal = shutdown_tx.clone();
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
                }
                Ok(mut sigterm) => {
                    if use_ctrl_c {
                        let ctrl_c = tokio::signal::ctrl_c();
                        tokio::select! {
                            _ = ctrl_c => {},
                            _ = sigterm.recv() => {},
                        }
                    } else {
                        let _ = sigterm.recv().await;
                    }
                }
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
        let _ = shutdown_tx_signal.send(());
    });

    // 1:1 C# CreateSpawns: стартовые здания + площадка золотой дороги при пустой
    // таблице зданий (fresh / после --regen). На живом мире — no-op. ДО GameState::new,
    // чтобы вставленные здания подхватились load_buildings_into_ecs, а клетки уже
    // были в `world` до того как он уйдёт в Arc.
    create_spawns(&database, &world).await?;

    let game_state = game::GameState::new(
        std::sync::Arc::new(world),
        std::sync::Arc::new(database),
        cfg.clone(),
    )
    .await?;

    // Cron system.
    cron::CronManager::new(std::sync::Arc::clone(&game_state), shutdown_tx.clone()).spawn();

    // Spawning console REPL
    let repl_state = std::sync::Arc::clone(&game_state);
    let repl_shutdown = shutdown_tx.clone();
    tokio::spawn(async move {
        if let Err(e) = console::run_repl(repl_state, repl_shutdown).await {
            tracing::error!(error = ?e, "REPL console error");
        }
    });

    // Run TCP server until shutdown signal.
    let net_res = net::run(std::sync::Arc::clone(&game_state), shutdown_tx.clone()).await;
    match &net_res {
        Ok(()) => tracing::info!("net::run finished Ok (accept loop ended, e.g. shutdown)"),
        Err(e) => tracing::error!(error = ?e, "net::run finished with error (process may exit)"),
    }

    shutdown_flush(&game_state).await;
    net_res
}

/// Финальное сохранение при остановке: игроки (с ретраем) + грязные здания +
/// отложенная очередь боксов + flush мира. Периодические циклы при shutdown уже
/// не сработают, поэтому дренируем всё, что они обычно пишут (иначе изменения
/// последнего интервала теряются). Вынесено из `main` — лимит строк.
// `significant_drop_tightening`: ecs-guard в блоке сбора dirty-зданий — та же
// конвенция, что в `lifecycle::spawn_building_dirty_flush_loop`.
#[allow(clippy::significant_drop_tightening)]
async fn shutdown_flush(game_state: &std::sync::Arc<game::GameState>) {
    tracing::info!("Shutdown: saving players, buildings, boxes and flushing world...");

    // Игроки — финальный one-shot с ретраем (периодика уже не повторит).
    let shutdown_pids: Vec<_> = game_state.active_players.iter().map(|e| *e.key()).collect();
    for pid in shutdown_pids {
        let player_row = game_state.query_player_opt(pid, |ecs, entity| {
            crate::game::player::extract_player_row(ecs, entity)
        });
        if let Some(row) = player_row {
            let mut ok = false;
            for attempt in 1..=3u32 {
                match game_state.db.save_player(&row).await {
                    Ok(()) => {
                        ok = true;
                        break;
                    }
                    Err(e) => {
                        tracing::warn!(
                            player_id = %pid,
                            attempt,
                            error = ?e,
                            "Shutdown player save attempt failed"
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                }
            }
            if !ok {
                tracing::error!(
                    player_id = %pid,
                    "Shutdown save failed for player after 3 attempts"
                );
            }
        }
    }

    // Грязные здания — 45s-цикл мог не успеть до shutdown. Собираем под write-локом,
    // отпускаем его ДО await (save берёт свой лок per-building).
    let dirty_entities: Vec<bevy_ecs::prelude::Entity> = {
        let mut ecs = game_state.ecs.write();
        let mut query = ecs.query::<(bevy_ecs::prelude::Entity, &game::BuildingFlags)>();
        query
            .iter(&ecs)
            .filter_map(|(e, f)| f.dirty.then_some(e))
            .collect()
    };
    for entity in dirty_entities {
        let row = game_state.modify_building(entity, |ecs, ent| {
            ecs.get::<game::BuildingFlags>(ent)
                .filter(|f| f.dirty)
                .and_then(|_| crate::game::buildings::extract_building_row(ecs, ent))
        });
        if let Some(r) = row
            && let Err(e) = game_state.db.save_building(&r).await
        {
            tracing::error!(error = ?e, "Shutdown building save failed");
        }
    }

    // Боксы — слить отложенную очередь персистенции (in-memory авторитетно).
    for (pos, op) in game_state.drain_box_persist() {
        let (bx, by): (i32, i32) = pos.into();
        let r = match op {
            None => game_state.db.delete_box_at(bx, by).await,
            Some(crystals) => game_state.db.upsert_box(bx, by, &crystals).await,
        };
        if let Err(e) = r {
            tracing::error!(x = bx, y = by, error = ?e, "Shutdown box persist failed");
        }
    }

    // Ожидание завершения всех фоновых транзакций к БД
    let start_t = std::time::Instant::now();
    while game_state
        .db_pending_tasks
        .load(std::sync::atomic::Ordering::SeqCst)
        > 0
    {
        if start_t.elapsed() > std::time::Duration::from_secs(5) {
            tracing::warn!("Timeout waiting for background DB tasks to complete during shutdown");
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    if let Err(e) = game_state.world.flush() {
        tracing::error!(error = ?e, "Shutdown world flush error");
    }
}

/// Выставить роль админа (`role = 2`) по нику из `M3R_GRANT_ADMIN` (через запятую).
async fn bootstrap_grant_admin(database: &db::Database) -> Result<()> {
    let Ok(raw) = env::var("M3R_GRANT_ADMIN") else {
        return Ok(());
    };
    for name in raw.split(',') {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        if let Some(p) = database.get_player_by_name(name).await? {
            if database.set_player_role(p.id, db::Role::Admin).await? {
                tracing::info!(player_id = p.id, player_name = %name, "M3R_GRANT_ADMIN: Role::Admin granted");
            }
        } else {
            tracing::warn!(player_name = %name, "M3R_GRANT_ADMIN: player not found");
        }
    }
    Ok(())
}

/// 1:1 C# `World.CreateSpawns` (`server_reference/WorldSystem/World.cs:97`).
/// При ПУСТОЙ таблице зданий (fresh world / после `--regen`): площадка 21×21
/// золотой дороги вокруг спавна (10,10) + стартовые Market/Resp/Up (owner 0).
/// На живом (непустом) мире — no-op (как C# gate `db.reqs.Count() < 1`).
///
/// Клетки пишем напрямую в `world` (load зданий в ECS клетки НЕ ставит, а на
/// `--regen` `.mapb` пуст). Порядок 1:1 C#: сначала платформа, потом футпринты
/// (двери/стены перекрывают золотую дорогу). `set_cell` метит chunk dirty → flush.
async fn create_spawns(database: &db::Database, world: &world::World) -> Result<()> {
    use crate::game::buildings::PackType;
    use crate::net::session::social::buildings::building_extra_for_pack_type;
    use crate::world::WorldProvider as _;

    const SPAWN_X: i32 = 10;
    const SPAWN_Y: i32 = 10;
    const GOLDEN_ROAD: u8 = 36;

    if !database.load_all_buildings().await?.is_empty() {
        return Ok(());
    }

    for rx in -10..=10 {
        for ry in -10..=10 {
            world.set_cell(SPAWN_X + rx, SPAWN_Y + ry, GOLDEN_ROAD);
        }
    }

    // Координаты 1:1 C#: Market(x-7,y-4), Resp(x-8,y+7), Up(x,y-4); owner 0, clanless.
    let spawns = [
        (PackType::Market, "M", SPAWN_X - 7, SPAWN_Y - 4),
        (PackType::Resp, "R", SPAWN_X - 8, SPAWN_Y + 7),
        (PackType::Up, "U", SPAWN_X, SPAWN_Y - 4),
    ];
    for (pack_type, code, ox, oy) in spawns {
        let extra = building_extra_for_pack_type(pack_type)?;
        database.insert_building(code, ox, oy, 0, 0, &extra).await?;
        for (dx, dy, cell) in pack_type.building_cells()? {
            world.set_cell(ox + dx, oy + dy, cell);
        }
    }
    tracing::info!("CreateSpawns: площадка 21×21 + Market/Resp/Up на спавне (10,10)");
    Ok(())
}

fn resolve_state_dir(cfg: &config::Config) -> Result<PathBuf> {
    resolve_state_dir_from(
        env::var("M3R_DATA_DIR").ok(),
        &env::current_dir()?,
        &cfg.data_dir,
    )
}

fn resolve_state_dir_from(
    env_override: Option<String>,
    current_dir: &Path,
    config_data_dir: &str,
) -> Result<PathBuf> {
    if let Some(raw) = env_override {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            anyhow::bail!("M3R_DATA_DIR is set but empty");
        }
        return Ok(PathBuf::from(trimmed));
    }
    let trimmed = config_data_dir.trim();
    if trimmed.is_empty() {
        anyhow::bail!("config data_dir is empty");
    }
    Ok(current_dir.join(trimmed))
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
        format!("{world_name}_v2.map"),
        format!("{world_name}_durability.mapb"),
    ];
    for name in names {
        let from = cwd.join(&name);
        let to = state_dir.join(&name);
        if to.exists() || !from.exists() {
            continue;
        }
        match std::fs::rename(&from, &to) {
            Ok(()) => tracing::info!(
                file_name = %name,
                state_dir = %state_dir.display(),
                "Migrated legacy state file"
            ),
            Err(err) => tracing::warn!(
                file_name = %name,
                state_dir = %state_dir.display(),
                error = ?err,
                "Could not migrate legacy state file"
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
                Ok(()) => tracing::info!(
                    from = %from.display(),
                    to = %to.display(),
                    "Renamed legacy database file"
                ),
                Err(e) => tracing::warn!(
                    from = %from.display(),
                    error = ?e,
                    "Could not rename legacy database file"
                ),
            }
        }
    }
}

/// Снести ВСЕ слои мира в `state_dir`, name-agnostic по суффиксам. Так смена
/// `world_name` (напр. `world`→`test-1`) не оставляет осиротевший прежний мир —
/// удаляются и текущие, и любые прежние `*_v2.map`/`*.mapb` (+ `.bak`). БД
/// (`.db`/`-wal`/`-shm`) и конфиги не трогаются — другие суффиксы.
fn remove_world_files(state_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(state_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let fname = entry.file_name();
        let Some(name) = fname.to_str() else {
            continue;
        };
        // `_v2.map`(+.bak) = cells/road; `.mapb`(+.bak) = durability/legacy/road.
        // `contains` (а не `ends_with`) покрывает `.bak`-варианты одним условием.
        let lower = name.to_ascii_lowercase();
        let is_world_layer = lower.contains("_v2.map") || lower.contains(".mapb");
        if !is_world_layer {
            continue;
        }
        let path = entry.path();
        if let Err(err) = std::fs::remove_file(&path) {
            if err.kind() != ErrorKind::NotFound {
                tracing::warn!(path = %path.display(), error = ?err, "Failed to remove world file");
            }
        } else {
            tracing::info!(path = %path.display(), "Removed world file for full world regeneration");
        }
    }
}

/// Полная очистка позиционно-привязанного состояния при регене мира. Чистит то,
/// что завязано на старый рельеф: здания, боксы, маркет-ордера (с отменой и
/// рефандом владельцам) + СБРОС позиций игроков на спавн. Аккаунты, прогресс
/// (инвентарь/скиллы/деньги) и программы НЕ трогаем — только координаты.
async fn regen_clear_world_state(database: &db::Database) -> Result<()> {
    // Спавн 1:1 с `create_spawns` (площадка золотой дороги вокруг (10,10)).
    const SPAWN_X: i32 = 10;
    const SPAWN_Y: i32 = 10;
    let n = database.delete_all_buildings().await?;
    tracing::info!(count = n, "World regen: cleared building rows from DB");

    let nb = database.delete_all_boxes().await?;
    tracing::info!(count = nb, "World regen: cleared crystal boxes");

    // Маркет-ордера: отменить и вернуть всё владельцам — залоченные предметы
    // инициатору, текущую ставку покупателю — затем снести.
    let orders = database.all_orders().await?;
    for o in &orders {
        database
            .add_player_inventory_item(o.initiator_id, o.item_id, o.num)
            .await?;
        if o.buyer_id > 0 {
            database.add_player_money(o.buyer_id, o.cost).await?;
        }
    }
    let no = database.delete_all_orders().await?;
    tracing::info!(
        count = no,
        "World regen: cancelled and refunded market orders"
    );

    let np = database
        .reset_all_players_to_spawn(SPAWN_X, SPAWN_Y)
        .await?;
    tracing::info!(
        count = np,
        spawn_x = SPAWN_X,
        spawn_y = SPAWN_Y,
        "World regen: reset player positions to spawn"
    );

    Ok(())
}

fn parse_regen_flag_from<I, S>(args: I, env_val: Option<String>) -> Result<bool, clap::Error>
where
    I: IntoIterator<Item = S>,
    S: Into<std::ffi::OsString> + Clone,
{
    use clap::Parser;

    #[derive(Parser, Debug, Clone)]
    #[command(name = "openmines-server", about = "OpenMines — игровой сервер (Rust)")]
    struct Args {
        /// Force regeneration of the world map on startup
        #[arg(long, aliases = ["regen-world"])]
        regen: bool,
    }

    let mut os_args = vec![std::ffi::OsString::from("openmines-server")];
    os_args.extend(args.into_iter().map(Into::into));

    let parsed = Args::try_parse_from(os_args)?;

    let force_regen = parsed.regen
        || env_config::parse_bool_env_or("M3R_REGEN_WORLD", env_val, false).map_err(|err| {
            clap::Error::raw(clap::error::ErrorKind::InvalidValue, err.to_string())
        })?;

    Ok(force_regen)
}

fn parse_port_override_from(env_val: Option<String>, config_port: u16) -> Result<u16> {
    let Some(raw) = env_val else {
        return Ok(config_port);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("M3R_PORT is set but empty");
    }
    trimmed
        .parse::<u16>()
        .map_err(|err| anyhow::anyhow!("invalid M3R_PORT {trimmed:?}: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `regen_clear_world_state`: сносит здания/боксы/ордера, рефандит ордер
    /// владельцу (предметы инициатору, ставку покупателю) И сбрасывает позиции
    /// игроков на спавн (старый рельеф невалиден). Прогресс не теряется.
    #[tokio::test]
    async fn regen_clears_world_state_and_refunds_orders() {
        let dir = std::env::temp_dir();
        let db_path = dir.join(format!("regen_clear_db_{}", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let database = crate::db::Database::open(&db_path).await.unwrap();

        // Инициатор (выставил ордер) + покупатель (сделал ставку).
        let initiator = database.create_player("init", "p", "h").await.unwrap();
        let buyer = database.create_player("buyer", "p", "h").await.unwrap();
        let buyer_money_before = database
            .get_player_by_id(buyer.id)
            .await
            .unwrap()
            .unwrap()
            .money;

        // Инициатора ставим на off-spawn позицию (внутри будущего рельефа) —
        // проверим, что реген сбросит её на спавн.
        let mut init_row = database
            .get_player_by_id(initiator.id)
            .await
            .unwrap()
            .unwrap();
        init_row.x = 999;
        init_row.y = 888;
        init_row.resp_x = Some(777);
        init_row.resp_y = Some(666);
        database.save_player(&init_row).await.unwrap();

        // Бокс (выпавшие кристаллы) + ордер с ставкой.
        database
            .upsert_box(7, 7, &[1, 2, 3, 0, 0, 0])
            .await
            .unwrap();
        database
            .create_order(initiator.id, 40, 5, 100)
            .await
            .unwrap();
        // Симулируем ставку: cost=250 (залок покупателя), buyer_id=buyer.
        let oid = database.all_orders().await.unwrap()[0].id;
        database
            .update_order_bet(oid, 250, buyer.id, 0)
            .await
            .unwrap();

        regen_clear_world_state(&database).await.unwrap();

        // Всё позиционное снесено.
        assert!(
            database.load_all_boxes().await.unwrap().is_empty(),
            "боксы не очищены"
        );
        assert!(
            database.all_orders().await.unwrap().is_empty(),
            "ордера не сняты"
        );
        // Рефанд: инициатору вернулись 5×item40, покупателю — ставка 250.
        let init_after = database
            .get_player_by_id(initiator.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            init_after.inventory.get(&40).copied().unwrap_or(0),
            5,
            "предметы не возвращены инициатору"
        );
        // Позиция и точка респавна сброшены на спавн (10,10), прогресс цел.
        assert_eq!(
            (
                init_after.x,
                init_after.y,
                init_after.resp_x,
                init_after.resp_y
            ),
            (10, 10, Some(10), Some(10)),
            "позиции игрока не сброшены на спавн при регене"
        );
        let buyer_after = database.get_player_by_id(buyer.id).await.unwrap().unwrap();
        assert_eq!(
            buyer_after.money,
            buyer_money_before + 250,
            "ставка не возвращена покупателю"
        );

        let _ = std::fs::remove_file(&db_path);
    }

    // --- clean args (env::args().skip(1)) ---

    #[test]
    fn test_parse_regen_flag_no_args() {
        assert!(!parse_regen_flag_from([] as [&str; 0], None).unwrap());
    }

    #[test]
    fn test_parse_regen_flag_cli_flag() {
        assert!(parse_regen_flag_from(["--regen"], None).unwrap());
        assert!(parse_regen_flag_from(["--regen-world"], None).unwrap());
    }

    #[test]
    fn test_parse_regen_flag_env_true() {
        assert!(parse_regen_flag_from([] as [&str; 0], Some("1".to_string())).unwrap());
        assert!(parse_regen_flag_from([] as [&str; 0], Some("true".to_string())).unwrap());
        assert!(parse_regen_flag_from([] as [&str; 0], Some("YES".to_string())).unwrap());
        assert!(parse_regen_flag_from([] as [&str; 0], Some(" on ".to_string())).unwrap());
    }

    #[test]
    fn test_parse_regen_flag_env_false() {
        assert!(!parse_regen_flag_from([] as [&str; 0], Some("0".to_string())).unwrap());
        assert!(!parse_regen_flag_from([] as [&str; 0], Some("false".to_string())).unwrap());
        assert!(!parse_regen_flag_from([] as [&str; 0], Some("NO".to_string())).unwrap());
        assert!(!parse_regen_flag_from([] as [&str; 0], Some(" off ".to_string())).unwrap());
        assert!(!parse_regen_flag_from([] as [&str; 0], None).unwrap());
    }

    #[test]
    fn test_parse_regen_flag_cli_overrides_env() {
        // --regen включает реген, даже если env = false
        assert!(parse_regen_flag_from(["--regen"], Some("0".to_string())).unwrap());
    }

    #[test]
    fn test_parse_regen_flag_invalid_env_is_error_not_false() {
        assert!(parse_regen_flag_from([] as [&str; 0], Some(String::new())).is_err());
        assert!(parse_regen_flag_from([] as [&str; 0], Some("wat".to_string())).is_err());
        assert!(parse_regen_flag_from([] as [&str; 0], Some("2".to_string())).is_err());
    }

    #[test]
    fn test_parse_port_override_absent_keeps_config_port() {
        assert_eq!(parse_port_override_from(None, 8090).unwrap(), 8090);
    }

    #[test]
    fn test_parse_port_override_valid_uses_env_port() {
        assert_eq!(
            parse_port_override_from(Some(" 19090 ".to_string()), 8090).unwrap(),
            19090
        );
    }

    #[test]
    fn test_parse_port_override_invalid_is_error_not_fallback() {
        assert!(parse_port_override_from(Some("abc".to_string()), 8090).is_err());
        assert!(parse_port_override_from(Some(String::new()), 8090).is_err());
        assert!(parse_port_override_from(Some("70000".to_string()), 8090).is_err());
    }

    #[test]
    fn test_resolve_state_dir_uses_explicit_env_override() {
        let cwd = PathBuf::from("/repo");
        assert_eq!(
            resolve_state_dir_from(Some(" /tmp/openmines ".to_string()), &cwd, "data").unwrap(),
            PathBuf::from("/tmp/openmines")
        );
    }

    #[test]
    fn test_resolve_state_dir_uses_config_data_dir_without_env() {
        let cwd = PathBuf::from("/repo");
        assert_eq!(
            resolve_state_dir_from(None, &cwd, " data ").unwrap(),
            PathBuf::from("/repo/data")
        );
    }

    #[test]
    fn test_resolve_state_dir_empty_values_are_errors_not_current_dir() {
        let cwd = PathBuf::from("/repo");
        assert!(resolve_state_dir_from(Some(String::new()), &cwd, "data").is_err());
        assert!(resolve_state_dir_from(Some("   ".to_string()), &cwd, "data").is_err());
        assert!(resolve_state_dir_from(None, &cwd, " ").is_err());
    }

    // --- ошибки ---

    #[test]
    fn test_parse_regen_flag_unknown_flag() {
        assert!(parse_regen_flag_from(["--unknown-flag"], None).is_err());
    }

    #[test]
    fn test_parse_regen_flag_argv0_leak_docker() {
        // Баг-регрессия: env::args() включает argv[0] = путь к бинарю.
        // Функция сама подставляет "openmines-server" как имя программы,
        // поэтому лишний argv[0] даёт clap-ошибку.
        assert!(parse_regen_flag_from(["/usr/local/bin/openmines-server"], None).is_err());
    }

    #[test]
    fn test_parse_regen_flag_argv0_leak_relative() {
        assert!(parse_regen_flag_from(["./openmines-server", "--regen"], None).is_err());
    }

    // --- CreateSpawns (стартовые здания + площадка) ---

    #[tokio::test]
    async fn create_spawns_places_buildings_and_platform_then_idempotent() {
        use crate::world::WorldProvider as _;

        let dir = std::env::temp_dir();
        let db_path = dir.join(format!("create_spawns_db_{}", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let database = crate::db::Database::open(&db_path).await.unwrap();
        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();
        let world_name = format!("create_spawns_world_{}", std::process::id());
        let world = crate::world::World::new(&world_name, 4, 4, cell_defs, &dir).unwrap();
        // Конфиг зданий нужен для building_cells/extra (OnceLock — может быть уже задан).
        let _ = crate::game::buildings::load_buildings_config(crate::test_config_path(
            "configs/buildings.json",
        ));

        // Fresh world → создаёт Market/Resp/Up + площадку.
        create_spawns(&database, &world).await.unwrap();
        assert_eq!(
            database.load_all_buildings().await.unwrap().len(),
            3,
            "Market/Resp/Up должны быть созданы на пустом мире"
        );
        assert_eq!(
            world.get_cell(15, 5),
            36,
            "клетка площадки = золотая дорога (36)"
        );
        assert_eq!(
            world.get_cell(10, 6),
            37,
            "Up origin = дверь (37), футпринт перекрыл площадку"
        );

        // Непустой мир → no-op (1:1 C# gate), повтор не плодит здания.
        create_spawns(&database, &world).await.unwrap();
        assert_eq!(
            database.load_all_buildings().await.unwrap().len(),
            3,
            "повторный вызов на непустом мире не должен создавать здания"
        );
    }
}

/// Запуск: `cargo test --release bench_tick -- --ignored --nocapture`
#[cfg(test)]
mod benchmarks {
    use std::sync::Arc;
    use std::time::Instant;

    const BENCH_N: u32 = 500;

    fn create_minimal_state() -> Arc<crate::game::GameState> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = std::env::temp_dir();
            let db_path = dir.join(format!("bench_db_{}", std::process::id()));
            let _ = std::fs::remove_file(&db_path);
            let database = crate::db::Database::open(&db_path).await.unwrap();
            let cell_defs =
                crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                    .unwrap();
            let world_name = format!("bench_world_{}", std::process::id());
            let world = crate::world::World::new(&world_name, 2, 2, cell_defs, &dir).unwrap();
            let config = crate::config::Config {
                world_name: world_name.clone(),
                port: 8090,
                world_chunks_w: 2,
                world_chunks_h: 2,
                data_dir: dir.to_string_lossy().to_string(),
                logging: crate::config::LoggingConfig::default(),
                cron: crate::config::CronConfig::default(),
                gameplay: crate::config::GameplayConfig::default(),
            };
            let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config)
                .await
                .unwrap();

            // Чистим за собой при падении assert
            let _ = std::fs::remove_file(&db_path);
            state
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
            100
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
}
