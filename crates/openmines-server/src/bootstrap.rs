use crate::db;
use crate::world;
use crate::world::WorldProvider as _;
use anyhow::Result;
use std::path::Path;

#[derive(Clone, Copy)]
enum LoadtestArena {
    Movement,
    Dig,
    BuildCycle,
    Granular,
}

impl LoadtestArena {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "movement" => Ok(Self::Movement),
            "dig" => Ok(Self::Dig),
            "build-cycle" => Ok(Self::BuildCycle),
            "granular" => Ok(Self::Granular),
            _ => anyhow::bail!(
                "M3R_LOADTEST_ARENA must be movement, dig, build-cycle, or granular, got {value:?}"
            ),
        }
    }
}

/// Build a deterministic 4x4 world for an end-to-end gameplay benchmark.
/// It is intentionally opt-in: production and ordinary dev worlds never use it.
pub fn apply_loadtest_arena_from_env(world: &world::World, state_dir: &Path) -> Result<()> {
    let Ok(value) = std::env::var("M3R_LOADTEST_ARENA") else {
        return Ok(());
    };
    let confirmed_state_dir = std::env::var("M3R_LOADTEST_STATE_DIR").map_err(|_| {
        anyhow::anyhow!(
            "M3R_LOADTEST_ARENA mutates the world; set M3R_LOADTEST_STATE_DIR to the explicit isolated state directory"
        )
    })?;
    confirm_loadtest_state_dir(&confirmed_state_dir, state_dir)?;
    let arena = LoadtestArena::parse(&value)?;
    apply_loadtest_arena(world, arena)?;
    tracing::warn!(arena = value, "Applied deterministic loadtest arena");
    Ok(())
}

fn confirm_loadtest_state_dir(confirmed_state_dir: &str, state_dir: &Path) -> Result<()> {
    if Path::new(confirmed_state_dir.trim()) != state_dir {
        anyhow::bail!(
            "M3R_LOADTEST_ARENA requires M3R_LOADTEST_STATE_DIR={} to match the server state directory",
            state_dir.display()
        );
    }
    Ok(())
}

fn apply_loadtest_arena(world: &world::World, arena: LoadtestArena) -> Result<()> {
    const SIZE: i32 = 128;
    const SIZE_U32: u32 = 128;
    const COLUMNS: i32 = 32;

    if world.cells_width() < SIZE_U32 || world.cells_height() < SIZE_U32 {
        anyhow::bail!(
            "loadtest arena needs at least 4x4 world, got {}x{} cells",
            world.cells_width(),
            world.cells_height()
        );
    }
    for y in 0..SIZE {
        for x in 0..SIZE {
            world.set_cell(x, y, crate::world::cells::cell_type::GOLDEN_ROAD);
        }
    }
    if matches!(
        arena,
        LoadtestArena::Dig | LoadtestArena::BuildCycle | LoadtestArena::Granular
    ) {
        for row in 0..COLUMNS {
            for column in 0..COLUMNS {
                let x = 2 + column * 4;
                let y = 2 + row * 4;
                match arena {
                    LoadtestArena::Dig => world.write_world_cell(
                        x,
                        y + 1,
                        crate::world::WorldCell {
                            cell_type: crate::world::CellType(
                                crate::world::cells::cell_type::GREEN,
                            ),
                            // The benchmark exercises mining on every request instead of turning
                            // into empty-cell rejects after the first dig.
                            durability: 1_000_000.0,
                        },
                    ),
                    LoadtestArena::BuildCycle => world.destroy_cell_and_road(x, y + 1),
                    LoadtestArena::Granular => {
                        // Movement cycles around this empty 2x2 square. The blocked sand remains
                        // adjacent, so every accepted move wakes a real granular neighborhood.
                        for (clear_x, clear_y) in [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)] {
                            world.destroy_cell_and_road(clear_x, clear_y);
                        }
                        world.set_cell(x + 2, y, crate::world::cells::cell_type::DARK_YELLOW_SAND);
                    }
                    LoadtestArena::Movement => unreachable!("movement has no target cells"),
                }
            }
        }
    }
    Ok(())
}

/// Удаляет файлы мира для регенерации на основе конкретного имени мира.
pub fn remove_world_files(state_dir: &Path, world_name: &str) {
    let targets = [
        state_dir.join(format!("{world_name}_v2.map")),
        state_dir.join(format!("{world_name}_v2.map.bak")),
        state_dir.join(format!("{world_name}_v2.map.tmp")),
        state_dir.join(format!("{world_name}_road_v2.map")),
        state_dir.join(format!("{world_name}_road_v2.map.bak")),
        state_dir.join(format!("{world_name}_road_v2.map.tmp")),
        state_dir.join(format!("{world_name}_durability.map")),
        state_dir.join(format!("{world_name}_durability.map.bak")),
        state_dir.join(format!("{world_name}_durability.map.tmp")),
        state_dir.join(format!("{world_name}_world.journal")),
    ];

    for path in &targets {
        if path.exists() {
            if let Err(err) = std::fs::remove_file(path) {
                if err.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(path = %path.display(), error = ?err, "Failed to remove world file");
                }
            } else {
                tracing::info!(path = %path.display(), "Removed world file for full world regeneration");
            }
        }
    }
}

/// Полная очистка позиционно-привязанного состояния при регене мира.
pub async fn regen_clear_world_state(
    database: &db::Database,
    spawn_x: i32,
    spawn_y: i32,
) -> Result<()> {
    let n = database.delete_all_buildings().await?;
    tracing::info!(count = n, "World regen: cleared building rows from DB");

    let nb = database.delete_all_boxes().await?;
    tracing::info!(count = nb, "World regen: cleared crystal boxes");

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
        .reset_all_players_to_spawn(spawn_x, spawn_y)
        .await?;
    tracing::info!(
        count = np,
        spawn_x,
        spawn_y,
        "World regen: reset player positions to spawn"
    );

    Ok(())
}

/// Выставить роль админа (role = 2) по нику из списка.
pub async fn bootstrap_grant_admin(
    database: &db::Database,
    grant_admin_list: Option<&str>,
) -> Result<()> {
    let Some(raw) = grant_admin_list else {
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

/// Создание стартовых зданий и площадки золотой дороги.
pub async fn create_spawns(
    database: &db::Database,
    world: &world::World,
    spawn_x: i32,
    spawn_y: i32,
) -> Result<()> {
    use crate::game::buildings::PackType;
    use crate::game::logic::buildings::building_extra_for_pack_type;

    const GOLDEN_ROAD: u8 = 36;

    if !database.load_all_buildings().await?.is_empty() {
        return Ok(());
    }

    for rx in -10..=10 {
        for ry in -10..=10 {
            world.set_cell(spawn_x + rx, spawn_y + ry, GOLDEN_ROAD);
        }
    }

    let spawns = [
        (PackType::Market, "M", spawn_x - 7, spawn_y - 4),
        (PackType::Resp, "R", spawn_x - 8, spawn_y + 7),
        (PackType::Up, "U", spawn_x, spawn_y - 4),
    ];
    for (pack_type, code, ox, oy) in spawns {
        let extra = building_extra_for_pack_type(pack_type)?;
        database.insert_building(code, ox, oy, 0, 0, &extra).await?;
        for (dx, dy, cell) in pack_type.building_cells()? {
            world.set_cell(ox + dx, oy + dy, cell);
        }
    }
    tracing::info!(
        spawn_x,
        spawn_y,
        "CreateSpawns: площадка 21×21 + Market/Resp/Up на спавне"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loadtest_arena_requires_the_exact_confirmed_state_dir() {
        let state_dir = Path::new("/tmp/openmines-loadtest");
        assert!(confirm_loadtest_state_dir("/tmp/openmines-loadtest", state_dir).is_ok());
        assert!(confirm_loadtest_state_dir("/tmp/openmines-dev", state_dir).is_err());
    }

    #[test]
    fn loadtest_dig_arena_has_one_durable_crystal_in_front_of_each_fixture_slot() {
        let dir = std::env::temp_dir();
        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();
        let world_name = format!("loadtest_arena_{}", std::process::id());
        let world = crate::world::World::new(&world_name, 4, 4, cell_defs, &dir).unwrap();

        apply_loadtest_arena(&world, LoadtestArena::Dig).unwrap();

        assert_eq!(world.get_cell(2, 3), crate::world::cells::cell_type::GREEN);
        assert_eq!(world.get_cell(6, 7), crate::world::cells::cell_type::GREEN);
        assert!((world.get_durability(2, 3) - 1_000_000.0).abs() < f32::EPSILON);
        assert_eq!(
            world.get_cell(2, 2),
            crate::world::cells::cell_type::GOLDEN_ROAD
        );
    }

    #[test]
    fn loadtest_granular_arena_places_blocked_sand_at_each_fixture_slot() {
        let dir = std::env::temp_dir();
        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();
        let world_name = format!("loadtest_granular_arena_{}", std::process::id());
        let world = crate::world::World::new(&world_name, 4, 4, cell_defs, &dir).unwrap();

        apply_loadtest_arena(&world, LoadtestArena::Granular).unwrap();

        assert_eq!(
            world.get_cell(4, 2),
            crate::world::cells::cell_type::DARK_YELLOW_SAND
        );
        assert_eq!(
            world.get_cell(4, 3),
            crate::world::cells::cell_type::GOLDEN_ROAD
        );
        assert_eq!(world.get_cell(3, 2), crate::world::cells::cell_type::EMPTY);
    }

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

        regen_clear_world_state(&database, 10, 10).await.unwrap();

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

    #[tokio::test]
    async fn create_spawns_places_buildings_and_platform_then_idempotent() {
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
        create_spawns(&database, &world, 20, 20).await.unwrap();
        assert_eq!(
            database.load_all_buildings().await.unwrap().len(),
            3,
            "Market/Resp/Up должны быть созданы на пустом мире"
        );
        assert_eq!(
            world.get_cell(25, 15),
            36,
            "клетка площадки = золотая дорога (36)"
        );
        assert_eq!(
            world.get_cell(20, 16),
            37,
            "Up origin = дверь (37), футпринт перекрыл площадку"
        );

        // Непустой мир → no-op (1:1 C# gate), повтор не плодит здания.
        create_spawns(&database, &world, 20, 20).await.unwrap();
        assert_eq!(
            database.load_all_buildings().await.unwrap().len(),
            3,
            "повторный вызов на непустом мире не должен создавать здания"
        );
        // Очищаем файлы мира после теста
        remove_world_files(&dir, &world_name);
    }
}
