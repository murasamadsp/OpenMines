use crate::game::player::PlayerPosition;
use crate::game::{BroadcastEffect, BroadcastQueue, WorldResource};
use crate::world::WorldProvider;
use crate::world::cells::cell_type;
use crate::world::cells::is_boulder;
use bevy_ecs::prelude::*;
use rand::Rng;

/// Клетки, на которые песок/валуны НЕ должны падать, хотя `isEmpty=true`.
/// Замена C# `TrueEmpty` (`Physics.cs` берёт `v = World.TrueEmpty`):
///   `TrueEmpty = isEmpty && !PackPart && cell ∉ {36, 37, 0, 39}`.
/// У нас нет битмапа `packsprop`, поэтому `PackPart` приближаем по типам клеток
/// футпринта зданий (`30` ворота, `35` фон-дорога, `38` угол, `106` невидимый
/// блок). Плюс C#-литералы: `36` золотая дорога, `37` дверь, `39` полимерная
/// дорога (`0` уже отсекает `is_empty`). `32`(EMPTY/выкопано) — НЕ в списке:
/// в C# `TrueEmpty(32)=true`, песок туда падает (иначе физика мертва).
const fn is_building_background_cell(cell: u8) -> bool {
    matches!(cell, 30 | 35 | 36 | 37 | 38 | 39 | 106)
}

/// Combined empty check for physics: cell must be empty AND not a building background.
fn is_physics_empty(world: &crate::world::World, x: i32, y: i32) -> bool {
    world.is_empty(x, y) && !is_building_background_cell(world.get_cell(x, y))
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

                let below_y = sy + 1;
                let two_below_y = sy + 2;

                // Common logic for straight fall
                if world.valid_coord(sx, below_y) {
                    let below_gate = world.get_cell(sx, below_y) == cell_type::GATE;

                    if below_gate
                        && world.valid_coord(sx, two_below_y)
                        && is_physics_empty(world, sx, two_below_y)
                    {
                        // Gate pass-through
                        tasks.push((sx, sy, sx, two_below_y, cell));
                        continue;
                    }

                    if is_physics_empty(world, sx, below_y) {
                        // Straight down
                        tasks.push((sx, sy, sx, below_y, cell));
                        continue;
                    }
                }

                // Diagonal slide logic
                if world.valid_coord(sx, below_y) {
                    let below_cell = world.get_cell(sx, below_y);
                    let below_is_solid =
                        cell_defs.get(below_cell).is_sand() || is_boulder(below_cell);

                    if below_is_solid {
                        let left_x = sx - 1;
                        let right_x = sx + 1;

                        if is_s {
                            // Sand diagonal slide
                            let left_ok = world.valid_coord(left_x, below_y)
                                && is_physics_empty(world, left_x, below_y);
                            let right_ok = world.valid_coord(right_x, below_y)
                                && is_physics_empty(world, right_x, below_y);

                            if left_ok && right_ok {
                                if rng.random_bool(0.5) {
                                    tasks.push((sx, sy, right_x, below_y, cell));
                                } else {
                                    tasks.push((sx, sy, left_x, below_y, cell));
                                }
                            } else if right_ok {
                                tasks.push((sx, sy, right_x, below_y, cell));
                            } else if left_ok {
                                tasks.push((sx, sy, left_x, below_y, cell));
                            }
                        } else {
                            // Boulder diagonal slide (requires side cell empty too)
                            let right_ok = world.valid_coord(right_x, below_y)
                                && is_physics_empty(world, right_x, below_y)
                                && world.valid_coord(right_x, sy)
                                && is_physics_empty(world, right_x, sy);
                            let left_ok = world.valid_coord(left_x, below_y)
                                && is_physics_empty(world, left_x, below_y)
                                && world.valid_coord(left_x, sy)
                                && is_physics_empty(world, left_x, sy);

                            if rng.random_bool(0.5) && right_ok {
                                tasks.push((sx, sy, right_x, below_y, cell));
                            } else if left_ok {
                                tasks.push((sx, sy, left_x, below_y, cell));
                            }
                        }
                    }
                }
            }
        }
    }

    // Apply moves
    tasks.sort_unstable();
    tasks.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);

    for (sx, sy, dest_x, dest_y, cell) in tasks {
        if is_physics_empty(world, dest_x, dest_y) {
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
    //! Изолированный прогон cell-мутирующих систем (sand/acid/alive) без сети:
    //! реальный `World`, игрок-entity, форс таймеров → проверяем (1) двигает ли
    //! физика клетки вообще, (2) не плодит ли НЕВАЛИДНЫЕ байты (порча карты).
    use crate::game::player::PlayerPosition;
    use crate::game::{BroadcastQueue, WorldResource, acid, alive};
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
        w.insert_resource(acid::AcidTickTimer { last_tick: past() });
        w.insert_resource(alive::AliveTickTimer { last_tick: past() });
        w.spawn(PlayerPosition {
            x: 64,
            y: 65,
            dir: 0,
        });

        let mut sched = Schedule::default();
        sched.add_systems(super::sand_physics_system);
        sched.add_systems(acid::acid_physics_system);
        sched.add_systems(alive::alive_physics_system);

        for _ in 0..80 {
            w.resource_mut::<super::SandTickTimer>().0 = past();
            w.resource_mut::<acid::AcidTickTimer>().last_tick = past();
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
}
