use crate::game::GameStateResource;
use crate::world::cells::cell_type;
use bevy_ecs::prelude::*;

#[allow(clippy::needless_pass_by_value)]
pub fn sand_physics_system(state_res: Res<GameStateResource>) {
    let state = &state_res.0;
    let mut tasks = Vec::new();

    // Scan around active players
    for entry in &state.active_players {
        let p = entry.value();
        let px = p.data.x;
        let py = p.data.y;

        // Radius of 16 cells for physics
        for dy in (-16..=16).rev() {
            for dx in -16..=16 {
                let x = px + dx;
                let y = py + dy;

                if !state.world.valid_coord(x, y) {
                    continue;
                }

                let cell = state.world.get_cell(x, y);
                let cell_defs = state.world.cell_defs();
                let prop = cell_defs.get(cell);

                if prop.is_sand() {
                    let down_y = y + 1;
                    if state.world.valid_coord(x, down_y) && state.world.is_empty(x, down_y) {
                        tasks.push((x, y, down_y, cell));
                    }
                }
            }
        }
    }

    tasks.sort_unstable();
    tasks.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);

    for (x, y, dy, cell) in tasks {
        if state.world.is_empty(x, dy) {
            state.world.set_cell(x, y, cell_type::EMPTY);
            state.world.set_cell(x, dy, cell);

            // Broadcast cell updates
            crate::game::broadcast_cell_update(state, x, y);
            crate::game::broadcast_cell_update(state, x, dy);
        }
    }
}
