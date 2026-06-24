use crate::game::player::PlayerPosition;
use crate::game::{BroadcastEffect, BroadcastQueue, WorldResource};
use crate::world::WorldProvider;
use crate::world::cells::cell_type;
use crate::world::cells::is_boulder;
use bevy_ecs::prelude::*;
use rand::Rng;

/// Проходима ли клетка для падающего блока. База — `is_empty` (≡ JS
/// `!BlockStats[id].solid`): песок течёт сквозь фон/обычную дорогу(35)/ворота(30).
/// НО плюс C#-блоклист `TrueEmpty`: `cell ∉ {0,36,37,39}` — инфраструктура зданий
/// (NOTHING, ЗОЛОТАЯ ДОРОГА 36, ДВЕРЬ 37, ПОЛИМЕР 39) НЕ проходима, иначе песок
/// затирает её («жрёт золотую дорогу»). Чистый JS-geophys это не ловит (там 36
/// не-solid), но C# `Physics` именно так и защищает паки. OOB = непроходимо.
fn is_passable(world: &crate::world::World, x: i32, y: i32) -> bool {
    use crate::world::cells::cell_type as ct;
    world.valid_coord(x, y)
        && world.is_empty(x, y)
        && !matches!(
            world.get_cell(x, y),
            ct::NOTHING | ct::GOLDEN_ROAD | ct::BUILDING_DOOR | ct::POLYMER_ROAD
        )
}

/// Есть ли у клетки `falltype` (песок/валун) — JS `BlockStats[id].falltype != null`.
fn has_falltype(world: &crate::world::World, x: i32, y: i32) -> bool {
    if !world.valid_coord(x, y) {
        return false;
    }
    let cell = world.get_cell(x, y);
    world.cell_defs().get(cell).is_sand() || is_boulder(cell)
}

/// JS `GeoPhisics.DownFree`: 1 = падать прямо вниз, 0 = диагональный соскок,
/// 2 = ждать/заблокировано (хода нет).
fn down_free(world: &crate::world::World, x: i32, y: i32) -> u8 {
    if is_passable(world, x, y + 1) {
        if has_falltype(world, x, y + 2) {
            if !is_passable(world, x, y + 3) {
                return 1;
            }
            return 2;
        }
        return 1;
    }
    if has_falltype(world, x, y + 1) {
        return 0;
    }
    2
}

#[derive(Resource)]
pub struct SandTickTimer(pub std::time::Instant);

impl Default for SandTickTimer {
    fn default() -> Self {
        Self(std::time::Instant::now())
    }
}

#[allow(clippy::needless_pass_by_value)]
pub fn sand_physics_system(
    world_res: Res<WorldResource>,
    mut bcast_q: ResMut<BroadcastQueue>,
    mut timer: ResMut<SandTickTimer>,
    query: Query<&PlayerPosition>,
) {
    // Run only once every 100ms
    if timer.0.elapsed().as_millis() < 100 {
        return;
    }
    timer.0 = std::time::Instant::now();

    let world = &world_res.0;
    let cell_defs = world.cell_defs();
    let mut rng = rand::rng();

    // (src_x, src_y, dest_x, dest_y, cell)
    let mut tasks: Vec<(i32, i32, i32, i32, u8)> = Vec::new();

    for pos in &query {
        let (player_x, player_y) = (pos.x, pos.y);

        // Scan 33x33 area around player
        for dy in (-16..=16_i32).rev() {
            for dx in -16..=16_i32 {
                let sx = player_x + dx;
                let sy = player_y + dy;

                if !world.valid_coord(sx, sy) {
                    continue;
                }

                let cell = world.get_cell(sx, sy);
                let is_s = cell_defs.get(cell).is_sand();
                let is_b = is_boulder(cell);

                if !is_s && !is_b {
                    continue;
                }

                // JS `FallingCycle`: ветвление по `DownFree`.
                // df==1 — прямо вниз; df==0 — диагональ; df==2 — стоим.
                let df = down_free(world, sx, sy);
                if df == 1 {
                    tasks.push((sx, sy, sx, sy + 1, cell));
                    continue;
                }
                if df != 0 {
                    continue;
                }

                // df==0: диагональный соскок. Сторона-первоочередь — по монетке
                // (JS `RandInt(0,1)`). Песку нужна свободная только клетка (x±1,y+1);
                // валуну — ещё и боковая (x±1,y), чтобы не «просочиться» в щель.
                let (first_x, second_x) = if rng.random_bool(0.5) {
                    (sx + 1, sx - 1)
                } else {
                    (sx - 1, sx + 1)
                };
                let can_slide = |tx: i32| {
                    is_passable(world, tx, sy + 1) && (is_s || is_passable(world, tx, sy))
                };
                if can_slide(first_x) {
                    tasks.push((sx, sy, first_x, sy + 1, cell));
                } else if can_slide(second_x) {
                    tasks.push((sx, sy, second_x, sy + 1, cell));
                }
            }
        }
    }

    // Apply moves
    tasks.sort_unstable();
    tasks.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);

    for (sx, sy, dest_x, dest_y, cell) in tasks {
        if is_passable(world, dest_x, dest_y) {
            // 1:1 C# `World.MoveCell` (Physics): durability ПЕРЕНОСИТСЯ. `set_cell`
            // сбрасывает её на дефолт типа → без переноса недокопанный валун «лечится»
            // до полной при падении. Читаем durability ДО очистки источника.
            let dur = world.get_durability(sx, sy);
            world.set_cell(sx, sy, cell_type::EMPTY);
            world.set_cell(dest_x, dest_y, cell);
            world.set_durability(dest_x, dest_y, dur);

            bcast_q.0.push(BroadcastEffect::CellUpdate(sx, sy));
            bcast_q.0.push(BroadcastEffect::CellUpdate(dest_x, dest_y));
        }
    }
}

#[cfg(test)]
mod physics_repro {
    //! Изолированный прогон cell-мутирующих систем (sand/alive) без сети:
    //! реальный `World`, игрок-entity, форс таймеров → проверяем (1) двигает ли
    //! физика клетки вообще, (2) не плодит ли НЕВАЛИДНЫЕ байты (порча карты).
    use crate::game::player::PlayerPosition;
    use crate::game::{BroadcastQueue, WorldResource, alive};
    use crate::world::cells::{CellDefs, cell_type};
    use crate::world::{World, WorldProvider};
    use bevy_ecs::prelude::*;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    fn past() -> Instant {
        Instant::now().checked_sub(Duration::from_secs(30)).unwrap()
    }

    #[test]
    fn physics_runs_and_makes_no_garbage() {
        let dir = std::env::temp_dir().join(format!("phys_repro_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let _ = std::fs::remove_file(e.path());
            }
        }
        let cd = CellDefs::load("configs/cells.json").unwrap();
        let n_defs = cd.cells.len();
        let world = Arc::new(World::new("phys", 4, 4, cd, &dir).unwrap());

        // Контролируемая сцена в очищённой области.
        for y in 50..90 {
            for x in 50..78 {
                world.set_cell(x, y, cell_type::EMPTY);
            }
        }
        // Песчинка вверху столбца — должна упасть на дно области.
        world.set_cell(64, 52, 100); // тёмный песок (is_sand)
        assert!(world.cell_defs().get(100).is_sand(), "100 must be sand");
        // Живая клетка — должна расползтись по пустым соседям.
        world.set_cell(60, 70, cell_type::ALIVE_CYAN);

        let mut w = bevy_ecs::world::World::new();
        w.insert_resource(WorldResource(Arc::clone(&world)));
        w.insert_resource(BroadcastQueue::default());
        w.insert_resource(super::SandTickTimer(past()));
        w.insert_resource(alive::AliveTickTimer { last_tick: past() });
        w.spawn(PlayerPosition {
            x: 64,
            y: 65,
            dir: 0,
        });

        let mut sched = Schedule::default();
        sched.add_systems(super::sand_physics_system);
        sched.add_systems(alive::alive_physics_system);

        for _ in 0..80 {
            w.resource_mut::<super::SandTickTimer>().0 = past();
            w.resource_mut::<alive::AliveTickTimer>().last_tick = past();
            sched.run(&mut w);
        }

        // (1) Песок реально двигался: верхняя клетка пуста, ниже по столбцу песок.
        let top = world.get_cell(64, 52);
        let column_has_sand =
            (52..90).any(|y| world.cell_defs().get(world.get_cell(64, y)).is_sand());
        println!(
            "sand top(64,52)={top} (EMPTY={}), column_has_sand={column_has_sand}",
            cell_type::EMPTY
        );
        assert_eq!(
            top,
            cell_type::EMPTY,
            "песок не упал — физика не двигает клетки"
        );
        assert!(column_has_sand, "песок исчез полностью");

        // (2) AliveCyan заливает пустых соседей CYAN-кристаллом (НЕ ALIVE_CYAN!):
        // источник остаётся, вокруг растут CYAN. Проверяем, что нарост случился.
        let mut cyan_grown = 0usize;
        for y in 50..90 {
            for x in 50..78 {
                if world.get_cell(x, y) == cell_type::CYAN {
                    cyan_grown += 1;
                }
            }
        }
        println!("CYAN grown by AliveCyan = {cyan_grown}");
        assert!(
            cyan_grown > 0,
            "alive не разросся — живые клетки не работают"
        );

        // (3) НИ ОДНА клетка во всём мире не должна стать невалидным байтом.
        let cw = world.cells_width().cast_signed();
        let ch = world.cells_height().cast_signed();
        let mut garbage = Vec::new();
        for y in 0..ch {
            for x in 0..cw {
                let c = world.get_cell(x, y);
                if usize::from(c) >= n_defs || world.cell_defs().get(c).name.is_empty() {
                    garbage.push((x, y, c));
                }
            }
        }
        println!(
            "garbage/unnamed cells = {} (n_defs={n_defs})",
            garbage.len()
        );
        for (x, y, c) in garbage.iter().take(20) {
            println!("  ({x},{y}) = {c}");
        }
        assert!(
            garbage.is_empty(),
            "физика создала невалидные/безымянные клетки: {} шт",
            garbage.len()
        );
    }

    /// JS `geophys.js`-паритет проходимости: песок падает сквозь НЕ-`solid` клетки
    /// (дорога 35, ворота 30 — в Rust `isEmpty=true`) и НЕ затирает `solid` кристалл
    /// (regress «сожрало кусок чанка»). До фикса `&& !is_building_background` держал
    /// песок на дороге/воротах — песок не доходил до пола.
    #[test]
    fn sand_falls_through_road_gate_rests_on_crystal() {
        const CRYSTAL: u8 = 107; // зелёные кристаллы — solid (isEmpty=false)
        const SAND: u8 = 100; // тёмный жёлтый песок (is_sand)
        let dir = std::env::temp_dir().join(format!("phys_road_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let _ = std::fs::remove_file(e.path());
            }
        }
        let cd = CellDefs::load("configs/cells.json").unwrap();
        let world = Arc::new(World::new("phys_road", 4, 4, cd, &dir).unwrap());

        let x = 64;
        // Изолируем колонку solid-стенами/полом/потолком, чтобы соседний
        // сгенерированный песок не заносился диагональю/сверху в проверяемую клетку.
        for y in 50..=67 {
            world.set_cell(x - 1, y, CRYSTAL);
            world.set_cell(x + 1, y, CRYSTAL);
        }
        for y in 52..66 {
            world.set_cell(x, y, cell_type::EMPTY);
        }
        world.set_cell(x, 51, CRYSTAL); // потолок
        world.set_cell(x, 66, CRYSTAL); // пол
        world.set_cell(x, 58, cell_type::ROAD); // в JS проходима
        world.set_cell(x, 60, cell_type::GATE); // в JS проходима
        world.set_cell(x, 52, SAND);
        assert!(!world.is_empty(x, 66), "кристалл должен быть solid");
        assert!(
            world.is_empty(x, 58),
            "дорога должна быть проходима (JS !solid)"
        );
        assert!(
            world.is_empty(x, 60),
            "ворота должны быть проходимы (JS !solid)"
        );

        let mut w = bevy_ecs::world::World::new();
        w.insert_resource(WorldResource(Arc::clone(&world)));
        w.insert_resource(BroadcastQueue::default());
        w.insert_resource(super::SandTickTimer(past()));
        w.spawn(PlayerPosition { x, y: 60, dir: 0 });

        let mut sched = Schedule::default();
        sched.add_systems(super::sand_physics_system);
        for _ in 0..40 {
            w.resource_mut::<super::SandTickTimer>().0 = past();
            sched.run(&mut w);
        }

        assert_eq!(
            world.get_cell(x, 66),
            CRYSTAL,
            "песок съел solid-кристалл (eating bug)"
        );
        assert_eq!(
            world.get_cell(x, 65),
            SAND,
            "песок не упал на кристалл сквозь дорогу/ворота (over-restriction)"
        );
        assert_eq!(
            world.get_cell(x, 52),
            cell_type::EMPTY,
            "источник песка не очистился"
        );
    }

    /// C# `TrueEmpty`-блоклист {0,36,37,39}: песок НЕ должен жрать золотую дорогу(36),
    /// дверь(37), полимер(39) — инфраструктуру зданий. Садится сверху, не затирает.
    #[test]
    fn sand_does_not_eat_golden_road() {
        const SAND: u8 = 100;
        let dir = std::env::temp_dir().join(format!("phys_gold_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let _ = std::fs::remove_file(e.path());
            }
        }
        let cd = CellDefs::load("configs/cells.json").unwrap();
        let world = Arc::new(World::new("phys_gold", 4, 4, cd, &dir).unwrap());
        let x = 64;
        for y in 50..=61 {
            world.set_cell(x - 1, y, 107); // solid-стены изоляции
            world.set_cell(x + 1, y, 107);
        }
        for y in 52..60 {
            world.set_cell(x, y, cell_type::EMPTY);
        }
        world.set_cell(x, 51, 107); // потолок
        world.set_cell(x, 60, cell_type::GOLDEN_ROAD); // пол — НЕ должен быть съеден
        world.set_cell(x, 52, SAND);
        // golden road: isEmpty=true, но C#-блоклист делает её НЕпроходимой для песка.
        assert!(
            world.is_empty(x, 60),
            "golden road всё ещё isEmpty в конфиге"
        );

        let mut w = bevy_ecs::world::World::new();
        w.insert_resource(WorldResource(Arc::clone(&world)));
        w.insert_resource(BroadcastQueue::default());
        w.insert_resource(super::SandTickTimer(past()));
        w.spawn(PlayerPosition { x, y: 56, dir: 0 });
        let mut sched = Schedule::default();
        sched.add_systems(super::sand_physics_system);
        for _ in 0..30 {
            w.resource_mut::<super::SandTickTimer>().0 = past();
            sched.run(&mut w);
        }

        assert_eq!(
            world.get_cell(x, 60),
            cell_type::GOLDEN_ROAD,
            "песок СЪЕЛ золотую дорогу (блоклист {{0,36,37,39}} не сработал)"
        );
        assert_eq!(
            world.get_cell(x, 59),
            SAND,
            "песок не сел на золотую дорогу"
        );
    }
}
