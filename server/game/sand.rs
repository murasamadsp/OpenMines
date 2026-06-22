use crate::game::player::PlayerPosition;
use crate::game::{BroadcastEffect, BroadcastQueue, WorldResource};
use crate::world::WorldProvider;
use crate::world::cells::cell_type;
use crate::world::cells::is_boulder;
use bevy_ecs::prelude::*;
use rand::Rng;

/// Quick check: is this cell type a building background?
/// These cells have `isEmpty=true` in cells.json but are placed by `Pack.Build()`
/// as part of building footprints. Sand/boulders must not land on them.
/// C# ref: `World.PackPart` / `World.TrueEmpty` checks prevent physics overwrites.
const fn is_building_background_cell(cell: u8) -> bool {
    matches!(cell, 30 | 32 | 35 | 37 | 38 | 106)
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
