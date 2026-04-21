use crate::game::player::PlayerPosition;
use crate::game::{BroadcastEffect, BroadcastQueue, GameStateResource};
use crate::world::WorldProvider;
use crate::world::cells::cell_type;
use crate::world::cells::is_boulder;
use bevy_ecs::prelude::*;
use rand::Rng;

#[allow(clippy::needless_pass_by_value)]
pub fn sand_physics_system(
    state_res: Res<GameStateResource>,
    mut bcast_q: ResMut<BroadcastQueue>,
    query: Query<&PlayerPosition>,
) {
    let state = &state_res.0;
    let cell_defs = state.world.cell_defs();
    // (src_x, src_y, dest_x, dest_y, cell)
    let mut tasks: Vec<(i32, i32, i32, i32, u8)> = Vec::new();

    for pos in &query {
        let (player_x, player_y) = (pos.x, pos.y);

        // Радиус 16 клеток для физики песка
        for dy in (-16..=16_i32).rev() {
            for dx in -16..=16_i32 {
                let sx = player_x + dx;
                let sy = player_y + dy;

                if !state.world.valid_coord(sx, sy) {
                    continue;
                }

                let cell = state.world.get_cell(sx, sy);
                if !cell_defs.get(cell).is_sand() {
                    continue;
                }

                let below_y = sy + 1;
                let below2_y = sy + 2;
                // D11: Gate pass-through — if cell below is Gate and 2 below is empty, skip through.
                if state.world.valid_coord(sx, below_y)
                    && state.world.get_cell(sx, below_y) == cell_type::GATE
                    && state.world.valid_coord(sx, below2_y)
                    && state.world.is_empty(sx, below2_y)
                {
                    tasks.push((sx, sy, sx, below2_y, cell));
                } else if state.world.valid_coord(sx, below_y) && state.world.is_empty(sx, below_y)
                {
                    // Straight down.
                    tasks.push((sx, sy, sx, below_y, cell));
                } else if state.world.valid_coord(sx, below_y) {
                    // Below is occupied — try diagonal slide (C# Physics.Sand diagonal fallback).
                    let below_cell = state.world.get_cell(sx, below_y);
                    let below_is_solid =
                        cell_defs.get(below_cell).is_sand() || is_boulder(below_cell);
                    if below_is_solid {
                        let left_x = sx - 1;
                        let right_x = sx + 1;
                        let left_ok = state.world.valid_coord(left_x, below_y)
                            && state.world.is_empty(left_x, below_y);
                        let right_ok = state.world.valid_coord(right_x, below_y)
                            && state.world.is_empty(right_x, below_y);
                        // D12: When both diagonals empty, randomly pick one.
                        if left_ok && right_ok {
                            let mut rng = rand::rng();
                            if rng.random_range(1..=100) > 50 {
                                tasks.push((sx, sy, right_x, below_y, cell));
                            } else {
                                tasks.push((sx, sy, left_x, below_y, cell));
                            }
                        } else if right_ok {
                            tasks.push((sx, sy, right_x, below_y, cell));
                        } else if left_ok {
                            tasks.push((sx, sy, left_x, below_y, cell));
                        }
                    }
                }
            }
        }
    }

    tasks.sort_unstable();
    tasks.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);

    for (sx, sy, dest_x, dest_y, cell) in tasks {
        if state.world.is_empty(dest_x, dest_y) {
            state.world.set_cell(sx, sy, cell_type::EMPTY);
            state.world.set_cell(dest_x, dest_y, cell);

            bcast_q.0.push(BroadcastEffect::CellUpdate(sx, sy));
            bcast_q.0.push(BroadcastEffect::CellUpdate(dest_x, dest_y));
        }
    }

    // Boulder fall system — same straight-down + diagonal slide logic as sand.
    let mut boulder_tasks: Vec<(i32, i32, i32, i32, u8)> = Vec::new();

    for pos in &query {
        let (player_x, player_y) = (pos.x, pos.y);

        for dy in (-16..=16_i32).rev() {
            for dx in -16..=16_i32 {
                let sx = player_x + dx;
                let sy = player_y + dy;

                if !state.world.valid_coord(sx, sy) {
                    continue;
                }

                let cell = state.world.get_cell(sx, sy);
                if !is_boulder(cell) {
                    continue;
                }

                let below_y = sy + 1;
                let below2_y = sy + 2;
                // D11: Gate pass-through for boulders.
                if state.world.valid_coord(sx, below_y)
                    && state.world.get_cell(sx, below_y) == cell_type::GATE
                    && state.world.valid_coord(sx, below2_y)
                    && state.world.is_empty(sx, below2_y)
                {
                    boulder_tasks.push((sx, sy, sx, below2_y, cell));
                } else if state.world.valid_coord(sx, below_y)
                    && state.world.is_empty(sx, below_y)
                {
                    boulder_tasks.push((sx, sy, sx, below_y, cell));
                } else if state.world.valid_coord(sx, below_y) {
                    let below_cell = state.world.get_cell(sx, below_y);
                    let below_is_solid =
                        cell_defs.get(below_cell).is_sand() || is_boulder(below_cell);
                    if below_is_solid {
                        let left_x = sx - 1;
                        let right_x = sx + 1;
                        // D13: Boulder diagonal — check side cell empty too, random > 50 for right first.
                        let right_ok = state.world.valid_coord(right_x, below_y)
                            && state.world.is_empty(right_x, below_y)
                            && state.world.valid_coord(right_x, sy)
                            && state.world.is_empty(right_x, sy);
                        let left_ok = state.world.valid_coord(left_x, below_y)
                            && state.world.is_empty(left_x, below_y)
                            && state.world.valid_coord(left_x, sy)
                            && state.world.is_empty(left_x, sy);
                        let mut rng = rand::rng();
                        if rng.random_range(1..=100) > 50 && right_ok {
                            boulder_tasks.push((sx, sy, right_x, below_y, cell));
                        } else if left_ok {
                            boulder_tasks.push((sx, sy, left_x, below_y, cell));
                        }
                    }
                }
            }
        }
    }

    boulder_tasks.sort_unstable();
    boulder_tasks.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);

    for (sx, sy, dest_x, dest_y, cell) in boulder_tasks {
        if state.world.is_empty(dest_x, dest_y) {
            state.world.set_cell(sx, sy, cell_type::EMPTY);
            state.world.set_cell(dest_x, dest_y, cell);

            bcast_q.0.push(BroadcastEffect::CellUpdate(sx, sy));
            bcast_q.0.push(BroadcastEffect::CellUpdate(dest_x, dest_y));
        }
    }
}
