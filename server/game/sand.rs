use crate::game::player::PlayerPosition;
use crate::game::{BroadcastEffect, BroadcastQueue, GameStateResource};
use crate::world::WorldProvider;
use crate::world::cells::cell_type;
use bevy_ecs::prelude::*;

#[allow(clippy::needless_pass_by_value)]
pub fn sand_physics_system(
    state_res: Res<GameStateResource>,
    mut bcast_q: ResMut<BroadcastQueue>,
    query: Query<&PlayerPosition>,
) {
    let state = &state_res.0;
    let cell_defs = state.world.cell_defs();
    let mut tasks = Vec::new();

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
                if cell_defs.get(cell).is_sand()
                    && state.world.valid_coord(sx, sy + 1)
                    && state.world.is_empty(sx, sy + 1)
                {
                    tasks.push((sx, sy, sy + 1, cell));
                }
            }
        }
    }

    tasks.sort_unstable();
    tasks.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);

    for (sx, sy, down_y, cell) in tasks {
        if state.world.is_empty(sx, down_y) {
            state.world.set_cell(sx, sy, cell_type::EMPTY);
            state.world.set_cell(sx, down_y, cell);

            bcast_q.0.push(BroadcastEffect::CellUpdate(sx, sy));
            bcast_q.0.push(BroadcastEffect::CellUpdate(sx, down_y));
        }
    }
}
