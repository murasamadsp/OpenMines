//! Спавн сущностей в мире (клетки), без логики сессии/чата.

use crate::net::session::prelude::*;
use std::sync::Arc;

/// Временный ящик рядом с точкой смерти.
pub fn spawn_crystal_box(state: &Arc<GameState>, x: i32, y: i32) -> Option<(i32, i32)> {
    const OFFSETS: [(i32, i32); 9] = [
        (0, 0),
        (1, 0),
        (-1, 0),
        (0, 1),
        (0, -1),
        (1, 1),
        (1, -1),
        (-1, 1),
        (-1, -1),
    ];

    for (dx, dy) in OFFSETS {
        let bx = x + dx;
        let by = y + dy;
        if state.world.valid_coord(bx, by) && state.world.is_empty(bx, by) {
            state.world.set_cell(bx, by, cell_type::BOX);
            return Some((bx, by));
        }
    }

    None
}
