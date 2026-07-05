//! Alive cells physics system — 7 behavior types from C# reference (`Physics.cs`).
//!
//! Alive cells tick every 5 seconds (C# `UpdateAlive` interval = 5000ms).
//! Each alive cell type has unique spreading/colony behavior.

use crate::game::player::PlayerPosition;
use crate::game::{BroadcastEffect, BroadcastQueue, WorldResource};
use crate::world::WorldProvider;
use crate::world::cells::cell_type;
use bevy_ecs::prelude::*;
use rand::Rng;
use std::time::Instant;

/// Directions used by alive cell logic (cardinal: right, down, left, up).
const DIRS: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];

/// Resource tracking alive tick interval (5s as in C# reference).
#[derive(Resource)]
pub struct AliveTickTimer {
    pub last_tick: Instant,
}

impl Default for AliveTickTimer {
    fn default() -> Self {
        Self {
            last_tick: Instant::now(),
        }
    }
}

/// Check if a cell type is an alive cell.
const fn is_alive(cell: crate::world::CellType) -> bool {
    matches!(
        cell.0,
        cell_type::ALIVE_BLUE
            | cell_type::ALIVE_CYAN
            | cell_type::ALIVE_RED
            | cell_type::ALIVE_BLACK
            | cell_type::ALIVE_VIOL
            | cell_type::ALIVE_WHITE
            | cell_type::ALIVE_RAINBOW
    )
}

/// Collected set of player positions for occupancy checks.
struct PlayerPositions {
    positions: Vec<(i32, i32)>,
}

impl PlayerPositions {
    fn has_player_at(&self, x: i32, y: i32) -> bool {
        self.positions.iter().any(|&(px, py)| px == x && py == y)
    }
}

/// A single cell mutation produced by alive logic.
struct AliveAction {
    x: i32,
    y: i32,
    cell: u8,
    durability: Option<f32>,
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_lines)]
pub fn alive_physics_system(
    world_res: Res<WorldResource>,
    mut bcast_q: ResMut<BroadcastQueue>,
    mut timer: ResMut<AliveTickTimer>,
    query: Query<&PlayerPosition>,
) {
    // Only tick every 5 seconds (C# reference: 5000ms interval).
    if timer.last_tick.elapsed().as_millis() < 5000 {
        return;
    }
    timer.last_tick = Instant::now();

    let world = &world_res.0;
    let cell_defs = world.cell_defs();

    // Collect all player positions for occupancy check.
    let players = PlayerPositions {
        positions: query.iter().map(|p| (p.x, p.y)).collect(),
    };

    // Collect alive cells near players (radius 16, same as sand).
    let mut alive_cells: Vec<(i32, i32, u8)> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for pos in &query {
        for dy in -16..=16_i32 {
            for dx in -16..=16_i32 {
                let x = pos.x + dx;
                let y = pos.y + dy;
                if !world.valid_coord(x, y) || !seen.insert((x, y)) {
                    continue;
                }
                let cell = crate::world::CellType(world.get_cell(x, y));
                if is_alive(cell) {
                    alive_cells.push((x, y, cell.0));
                }
            }
        }
    }

    let mut rng = rand::rng();
    let mut actions: Vec<AliveAction> = Vec::new();
    let mut clears: Vec<(i32, i32)> = Vec::new();

    for (x, y, cell) in &alive_cells {
        let (x, y, cell) = (*x, *y, *cell);

        // Calculate modifier from adjacent HypnoRock (119) — C# reference.
        let mut modif = 1;
        for &(dx, dy) in &DIRS {
            if world.valid_coord(x + dx, y + dy)
                && world.get_cell(x + dx, y + dy) == cell_type::HYPNO_ROCK
            {
                modif += 2;
            }
        }
        if modif > 1 {
            modif -= 1;
        }

        match cell {
            cell_type::ALIVE_CYAN => {
                alive_cyan(x, y, modif, world, &players, &mut actions);
            }
            cell_type::ALIVE_RED => {
                alive_red(x, y, modif, world, &players, &mut actions);
            }
            cell_type::ALIVE_VIOL => {
                alive_viol(x, y, modif, world, &players, &mut actions);
            }
            cell_type::ALIVE_BLACK => {
                alive_black(
                    x,
                    y,
                    modif,
                    world,
                    &players,
                    &mut actions,
                    &mut clears,
                    &mut rng,
                );
            }
            cell_type::ALIVE_WHITE => {
                alive_white(
                    x,
                    y,
                    modif,
                    world,
                    &players,
                    &mut actions,
                    &cell_defs,
                    &mut rng,
                );
            }
            cell_type::ALIVE_BLUE => {
                alive_blue(
                    x,
                    y,
                    modif,
                    world,
                    &players,
                    &mut actions,
                    &mut clears,
                    &mut rng,
                );
            }
            cell_type::ALIVE_RAINBOW => {
                alive_rainbow(x, y, modif, world, &players, &mut actions, &cell_defs);
            }
            _ => {}
        }
    }

    // Apply clears first (e.g., AliveWhite destroying sand above).
    for (cx, cy) in &clears {
        world.set_cell(*cx, *cy, cell_type::EMPTY);
        bcast_q
            .0
            .push(BroadcastEffect::CellUpdate((*cx, *cy).into()));
    }

    // Apply actions.
    for action in &actions {
        world.set_cell(action.x, action.y, action.cell);
        if let Some(dur) = action.durability {
            world.set_durability(action.x, action.y, dur);
        }
        bcast_q
            .0
            .push(BroadcastEffect::CellUpdate((action.x, action.y).into()));
    }
}

/// `AliveCyan`: floods all empty cardinal neighbors with Cyan (durability 2*mod).
fn alive_cyan(
    x: i32,
    y: i32,
    modif: i32,
    world: &crate::world::World,
    players: &PlayerPositions,
    actions: &mut Vec<AliveAction>,
) {
    for &(dx, dy) in &DIRS {
        let nx = x + dx;
        let ny = y + dy;
        if world.valid_coord(nx, ny) && world.is_empty(nx, ny) && !players.has_player_at(nx, ny) {
            actions.push(AliveAction {
                x: nx,
                y: ny,
                cell: cell_type::CYAN,
                durability: Some(f32::from(i16::try_from(2 * modif).unwrap_or(i16::MAX))),
            });
        }
    }
}

/// `AliveRed`: requires adjacent `BlackRock` in 3x3; floods empty cardinal neighbors with Red.
fn alive_red(
    x: i32,
    y: i32,
    modif: i32,
    world: &crate::world::World,
    players: &PlayerPositions,
    actions: &mut Vec<AliveAction>,
) {
    // Check for BlackRock in 3x3 neighborhood.
    let mut has_black_rock = false;
    for cx in -1..=1 {
        for cy in -1..=1 {
            if world.valid_coord(x + cx, y + cy)
                && world.get_cell(x + cx, y + cy) == cell_type::BLACK_ROCK
            {
                has_black_rock = true;
            }
        }
    }
    if !has_black_rock {
        return;
    }

    for &(dx, dy) in &DIRS {
        let nx = x + dx;
        let ny = y + dy;
        if world.valid_coord(nx, ny) && world.is_empty(nx, ny) && !players.has_player_at(nx, ny) {
            actions.push(AliveAction {
                x: nx,
                y: ny,
                cell: cell_type::RED,
                durability: Some(f32::from(i16::try_from(3 * modif).unwrap_or(i16::MAX))),
            });
        }
    }
}

/// `AliveViol`: requires adjacent `BlackRock` in 3x3; floods empty cardinal neighbors with Violet.
fn alive_viol(
    x: i32,
    y: i32,
    modif: i32,
    world: &crate::world::World,
    players: &PlayerPositions,
    actions: &mut Vec<AliveAction>,
) {
    let mut has_black_rock = false;
    for cx in -1..=1 {
        for cy in -1..=1 {
            if world.valid_coord(x + cx, y + cy)
                && world.get_cell(x + cx, y + cy) == cell_type::BLACK_ROCK
            {
                has_black_rock = true;
            }
        }
    }
    if !has_black_rock {
        return;
    }

    for &(dx, dy) in &DIRS {
        let nx = x + dx;
        let ny = y + dy;
        if world.valid_coord(nx, ny) && world.is_empty(nx, ny) && !players.has_player_at(nx, ny) {
            actions.push(AliveAction {
                x: nx,
                y: ny,
                cell: cell_type::VIOLET,
                durability: Some(f32::from(i16::try_from(2 * modif).unwrap_or(i16::MAX))),
            });
        }
    }
}

/// `AliveBlack`: colony behavior. If >=6 neighbors are `AliveBlack`, converts self to `BlackRock`.
/// Otherwise, if an adjacent `AliveBlack` exists and opposite side is empty, spawns Red/Cyan.
#[allow(clippy::too_many_arguments)]
fn alive_black(
    x: i32,
    y: i32,
    modif: i32,
    world: &crate::world::World,
    players: &PlayerPositions,
    actions: &mut Vec<AliveAction>,
    clears: &mut Vec<(i32, i32)>,
    rng: &mut impl Rng,
) {
    // Count AliveBlack in 3x3.
    let mut count = 0;
    for ax in -1..=1 {
        for ay in -1..=1 {
            if world.valid_coord(x + ax, y + ay)
                && world.get_cell(x + ax, y + ay) == cell_type::ALIVE_BLACK
            {
                count += 1;
            }
        }
    }

    if count >= 6 {
        // Convert self to BlackRock (114).
        // We use clears to remove the alive cell, then place BlackRock via actions.
        clears.push((x, y));
        actions.push(AliveAction {
            x,
            y,
            cell: cell_type::BLACK_ROCK,
            durability: None,
        });
        return;
    }

    if count > 0 {
        for &(dx, dy) in &DIRS {
            let nx = x + dx;
            let ny = y + dy;
            if world.valid_coord(nx, ny) && world.get_cell(nx, ny) == cell_type::ALIVE_BLACK {
                // Opposite direction.
                let ox = x - dx;
                let oy = y - dy;
                if world.valid_coord(ox, oy)
                    && world.is_empty(ox, oy)
                    && !players.has_player_at(ox, oy)
                {
                    if rng.random_range(1..=100) > 50 {
                        actions.push(AliveAction {
                            x: ox,
                            y: oy,
                            cell: cell_type::RED,
                            durability: Some(f32::from(
                                i16::try_from(3 * modif).unwrap_or(i16::MAX),
                            )),
                        });
                    } else {
                        actions.push(AliveAction {
                            x: ox,
                            y: oy,
                            cell: cell_type::CYAN,
                            durability: Some(f32::from(
                                i16::try_from(2 * modif).unwrap_or(i16::MAX),
                            )),
                        });
                    }
                    return;
                }
            }
        }
    }
}

/// `AliveWhite`: if sand is above, fills 3x3 empty cells with White and 20% chance destroys sand.
#[allow(clippy::too_many_arguments)]
fn alive_white(
    x: i32,
    y: i32,
    modif: i32,
    world: &crate::world::World,
    players: &PlayerPositions,
    actions: &mut Vec<AliveAction>,
    cell_defs: &crate::world::cells::CellDefs,
    rng: &mut impl Rng,
) {
    // Check if cell above is sand.
    if !world.valid_coord(x, y - 1) {
        return;
    }
    let above = world.get_cell(x, y - 1);
    if !cell_defs.get(above).is_sand() {
        return;
    }

    // Fill 3x3 empty cells with White.
    for wx in -1..=1 {
        for wy in -1..=1 {
            let nx = x + wx;
            let ny = y + wy;
            if world.valid_coord(nx, ny) && world.is_empty(nx, ny) && !players.has_player_at(nx, ny)
            {
                actions.push(AliveAction {
                    x: nx,
                    y: ny,
                    cell: cell_type::WHITE,
                    durability: Some(f32::from(i16::try_from(9 * modif).unwrap_or(i16::MAX))),
                });
            }
        }
    }

    // 20% chance to destroy the sand above.
    if rng.random_range(1..=100) < 20 {
        clears_sand(x, y - 1, actions);
    }
}

/// Helper: destroy a cell (set to EMPTY via action).
fn clears_sand(x: i32, y: i32, actions: &mut Vec<AliveAction>) {
    actions.push(AliveAction {
        x,
        y,
        cell: cell_type::EMPTY,
        durability: None,
    });
}

/// `AliveBlue`: 20% chance per direction — moves self there, leaves Blue (109) behind.
#[allow(clippy::too_many_arguments)]
fn alive_blue(
    x: i32,
    y: i32,
    modif: i32,
    world: &crate::world::World,
    players: &PlayerPositions,
    actions: &mut Vec<AliveAction>,
    clears: &mut Vec<(i32, i32)>,
    rng: &mut impl Rng,
) {
    for &(dx, dy) in &DIRS {
        let nx = x + dx;
        let ny = y + dy;
        if rng.random_range(1..=100) < 20
            && world.valid_coord(nx, ny)
            && world.is_empty(nx, ny)
            && !players.has_player_at(nx, ny)
        {
            // Move alive cell to new position.
            clears.push((x, y));
            actions.push(AliveAction {
                x: nx,
                y: ny,
                cell: cell_type::ALIVE_BLUE,
                durability: None,
            });
            // Leave Blue (109) at old position.
            actions.push(AliveAction {
                x,
                y,
                cell: cell_type::BLUE,
                durability: Some(f32::from(i16::try_from(20 * modif).unwrap_or(i16::MAX))),
            });
            return;
        }
    }
}

/// `AliveRainbow`: copies the cell from the opposite direction into empty cardinal neighbors.
fn alive_rainbow(
    x: i32,
    y: i32,
    modif: i32,
    world: &crate::world::World,
    players: &PlayerPositions,
    actions: &mut Vec<AliveAction>,
    cell_defs: &crate::world::cells::CellDefs,
) {
    for &(dx, dy) in &DIRS {
        let nx = x + dx;
        let ny = y + dy;
        // Opposite cell.
        let ox = x - dx;
        let oy = y - dy;

        if !world.valid_coord(nx, ny) || !world.valid_coord(ox, oy) {
            continue;
        }

        if !world.is_empty(nx, ny) || players.has_player_at(nx, ny) {
            continue;
        }

        let opposite_cell = crate::world::CellType(world.get_cell(ox, oy));
        let odef = cell_defs.get_typed(opposite_cell);

        // C# conditions: not alive, not empty, is_diggable, is_destructible.
        if is_alive(opposite_cell)
            || odef.cell_is_empty()
            || !odef.is_diggable()
            || !odef.physical.is_destructible
        {
            continue;
        }

        let target_def = cell_defs.get_typed(opposite_cell);
        actions.push(AliveAction {
            x: nx,
            y: ny,
            cell: opposite_cell.0,
            durability: Some(
                target_def.durability * f32::from(i16::try_from(modif).unwrap_or(i16::MAX)),
            ),
        });
    }
}
