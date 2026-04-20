//! Active acid physics system.
//!
//! Two acid cell types from C# `CellType` enum:
//! - `LivingActiveAcid` (95): corrodes adjacent diggable cells over time (damages durability,
//!   destroys to EMPTY when durability reaches 0).
//! - `CorrosiveActiveAcid` (96): same as Living but replaces destroyed cells with `PassiveAcid` (86)
//!   instead of EMPTY, creating acid pools.
//!
//! Ticks every 3 seconds, scans radius 16 around players (same pattern as alive.rs).

use crate::game::player::PlayerPosition;
use crate::game::{BroadcastEffect, BroadcastQueue, GameStateResource};
use crate::world::WorldProvider;
use crate::world::cells::cell_type;
use bevy_ecs::prelude::*;
use rand::Rng;
use std::time::Instant;

/// Cardinal directions for acid corrosion checks.
const DIRS: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];

/// Acid damage per tick applied to adjacent cells' durability.
const ACID_DAMAGE_PER_TICK: f32 = 2.0;

/// Resource tracking acid tick interval (3s).
#[derive(Resource)]
pub struct AcidTickTimer {
    pub last_tick: Instant,
}

impl Default for AcidTickTimer {
    fn default() -> Self {
        Self {
            last_tick: Instant::now(),
        }
    }
}

/// Check if a cell type is an active acid cell.
const fn is_active_acid(cell: u8) -> bool {
    matches!(
        cell,
        cell_type::LIVING_ACTIVE_ACID | cell_type::CORROSIVE_ACTIVE_ACID
    )
}

/// A single cell mutation produced by acid logic.
struct AcidAction {
    x: i32,
    y: i32,
    cell: u8,
    durability: Option<f32>,
}

#[allow(clippy::needless_pass_by_value)]
pub fn acid_physics_system(
    state_res: Res<GameStateResource>,
    mut bcast_q: ResMut<BroadcastQueue>,
    mut timer: ResMut<AcidTickTimer>,
    query: Query<&PlayerPosition>,
) {
    // Tick every 3 seconds.
    if timer.last_tick.elapsed().as_millis() < 3000 {
        return;
    }
    timer.last_tick = Instant::now();

    let state = &state_res.0;
    let cell_defs = state.world.cell_defs();

    // Collect active acid cells near players (radius 16).
    let mut acid_cells: Vec<(i32, i32, u8)> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for pos in &query {
        for dy in -16..=16_i32 {
            for dx in -16..=16_i32 {
                let x = pos.x + dx;
                let y = pos.y + dy;
                if !state.world.valid_coord(x, y) || !seen.insert((x, y)) {
                    continue;
                }
                let cell = state.world.get_cell(x, y);
                if is_active_acid(cell) {
                    acid_cells.push((x, y, cell));
                }
            }
        }
    }

    if acid_cells.is_empty() {
        return;
    }

    let mut rng = rand::rng();
    let mut actions: Vec<AcidAction> = Vec::new();

    for &(x, y, cell) in &acid_cells {
        let is_corrosive = cell == cell_type::CORROSIVE_ACTIVE_ACID;

        for &(dx, dy) in &DIRS {
            let nx = x + dx;
            let ny = y + dy;

            if !state.world.valid_coord(nx, ny) {
                continue;
            }

            let neighbor = state.world.get_cell(nx, ny);
            let ndef = cell_defs.get(neighbor);

            // Skip empty cells, non-diggable, non-destructible, other acids, and borders.
            if ndef.cell_is_empty()
                || !ndef.is_diggable()
                || !ndef.physical.is_destructible
                || is_active_acid(neighbor)
                || neighbor == cell_type::PASSIVE_ACID
                || neighbor == cell_type::ACID_ROCK
                || neighbor == cell_type::BORDER
            {
                continue;
            }

            // 50% chance per direction per tick to corrode (stochastic to avoid instant destruction).
            if rng.random_range(1..=100) > 50 {
                continue;
            }

            let current_dur = state.world.get_durability(nx, ny);
            let new_dur = current_dur - ACID_DAMAGE_PER_TICK;

            if new_dur <= 0.0 {
                // Cell destroyed.
                let replacement = if is_corrosive {
                    cell_type::PASSIVE_ACID
                } else {
                    cell_type::EMPTY
                };
                actions.push(AcidAction {
                    x: nx,
                    y: ny,
                    cell: replacement,
                    durability: if is_corrosive {
                        Some(cell_defs.get(cell_type::PASSIVE_ACID).durability)
                    } else {
                        None
                    },
                });
            } else {
                // Reduce durability only (cell type stays).
                state.world.set_durability(nx, ny, new_dur);
            }
        }
    }

    // Apply actions.
    for action in &actions {
        state.world.set_cell(action.x, action.y, action.cell);
        if let Some(dur) = action.durability {
            state.world.set_durability(action.x, action.y, dur);
        }
        bcast_q
            .0
            .push(BroadcastEffect::CellUpdate(action.x, action.y));
    }
}
