//! Alive cells physics system — 7 behavior types from C# reference (`Physics.cs`).
//!
//! Alive cells tick every 5 seconds (C# `UpdateAlive` interval = 5000ms).
//! Each alive cell type has unique spreading/colony behavior.

use crate::game::player::{PlayerConnection, PlayerPosition};
use crate::game::programmator::ProgrammatorState;
use crate::game::{
    BroadcastEffect, BroadcastQueue, ScheduleConfigResource, WorldPos, WorldResource,
};
use crate::world::WorldProvider;
use crate::world::cells::cell_type;
use bevy_ecs::prelude::*;
use parking_lot::Mutex;
use rand::Rng;
#[cfg(test)]
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Directions used by alive cell logic (cardinal: right, down, left, up).
const DIRS: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];
const ACTIVE_RADIUS: i32 = 16;

/// Check if a cell type is an alive cell.
pub const fn is_alive(cell: crate::world::CellType) -> bool {
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

#[derive(Default)]
struct AliveWorkState {
    cells: HashSet<WorldPos>,
    region_seeds: HashSet<WorldPos>,
    active: bool,
}

#[derive(Resource, Clone, Default)]
pub struct AliveWorkQueue(Arc<Mutex<AliveWorkState>>);

impl AliveWorkQueue {
    pub fn seed_region(&self, x: i32, y: i32) {
        let mut state = self.0.lock();
        state.region_seeds.insert((x, y).into());
        state.active = true;
    }

    pub fn note_cell(&self, x: i32, y: i32, cell: crate::world::CellType) {
        let mut state = self.0.lock();
        if is_alive(cell) {
            state.cells.insert((x, y).into());
            state.active = true;
        } else {
            state.cells.remove(&WorldPos::from((x, y)));
        }
    }

    fn take_region_seeds(&self) -> Vec<WorldPos> {
        std::mem::take(&mut self.0.lock().region_seeds)
            .into_iter()
            .collect()
    }

    fn cells(&self) -> Vec<WorldPos> {
        self.0.lock().cells.iter().copied().collect()
    }

    fn set_active(&self, active: bool) {
        self.0.lock().active = active;
    }

    pub fn has_work(&self) -> bool {
        let state = self.0.lock();
        state.active || !state.region_seeds.is_empty()
    }
}

/// Collected set of player positions for occupancy checks.
struct PlayerPositions {
    positions: HashSet<(i32, i32)>,
}

impl PlayerPositions {
    fn has_player_at(&self, x: i32, y: i32) -> bool {
        self.positions.contains(&(x, y))
    }

    fn is_active_for(&self, x: i32, y: i32) -> bool {
        self.positions.iter().any(|&(px, py)| {
            x.saturating_sub(px).unsigned_abs() <= ACTIVE_RADIUS.unsigned_abs()
                && y.saturating_sub(py).unsigned_abs() <= ACTIVE_RADIUS.unsigned_abs()
        })
    }
}

#[cfg(test)]
struct ActiveFrontier {
    rows: Vec<(i32, Vec<(i32, i32)>)>,
}

#[cfg(test)]
impl ActiveFrontier {
    fn around(players: &PlayerPositions) -> Self {
        let mut rows: BTreeMap<i32, Vec<(i32, i32)>> = BTreeMap::new();
        for &(px, py) in &players.positions {
            let start_x = px.saturating_sub(ACTIVE_RADIUS);
            let end_x = px.saturating_add(ACTIVE_RADIUS);
            for y in py.saturating_sub(ACTIVE_RADIUS)..=py.saturating_add(ACTIVE_RADIUS) {
                rows.entry(y).or_default().push((start_x, end_x));
            }
        }

        let rows = rows
            .into_iter()
            .map(|(y, mut intervals)| {
                intervals.sort_unstable();
                let mut merged: Vec<(i32, i32)> = Vec::with_capacity(intervals.len());
                for (start, end) in intervals {
                    if let Some((_, previous_end)) = merged.last_mut()
                        && start <= previous_end.saturating_add(1)
                    {
                        *previous_end = (*previous_end).max(end);
                    } else {
                        merged.push((start, end));
                    }
                }
                (y, merged)
            })
            .collect();
        Self { rows }
    }
}

/// A single cell mutation produced by alive logic.
struct AliveAction {
    x: i32,
    y: i32,
    cell: crate::world::CellType,
    durability: Option<f32>,
}

fn dedup_alive_actions(actions: Vec<AliveAction>) -> Vec<AliveAction> {
    let mut by_pos = std::collections::HashMap::with_capacity(actions.len());
    for action in actions {
        by_pos.insert((action.x, action.y), action);
    }
    by_pos.into_values().collect()
}

fn seed_alive_regions(queue: &AliveWorkQueue, world: &crate::world::World) -> (usize, usize) {
    let mut seeds = queue.take_region_seeds();
    seeds.sort_unstable_by_key(|pos| {
        let (x, y): (i32, i32) = (*pos).into();
        (y, x)
    });
    seeds.dedup();
    let mut scanned = 0usize;
    for seed in seeds.iter().copied() {
        let (px, py): (i32, i32) = seed.into();
        for y in py.saturating_sub(ACTIVE_RADIUS)..=py.saturating_add(ACTIVE_RADIUS) {
            for x in px.saturating_sub(ACTIVE_RADIUS)..=px.saturating_add(ACTIVE_RADIUS) {
                scanned += 1;
                if world.valid_coord(x, y) {
                    queue.note_cell(x, y, world.get_cell_typed(x, y));
                }
            }
        }
    }
    (seeds.len(), scanned)
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_lines)]
pub fn alive_physics_system(
    world_res: Res<WorldResource>,
    schedule_cfg: Res<ScheduleConfigResource>,
    alive_work: Res<AliveWorkQueue>,
    mut bcast_q: ResMut<BroadcastQueue>,
    query: Query<(
        &PlayerPosition,
        Option<&PlayerConnection>,
        Option<&ProgrammatorState>,
    )>,
) {
    let started_at = Instant::now();
    let world = &world_res.0;
    let cell_defs = world.cell_defs();

    // Collect all player positions for occupancy check.
    let player_collect_t0 = Instant::now();
    let simulated_positions: HashSet<(i32, i32)> = query
        .iter()
        .filter_map(|(pos, conn, prog)| {
            (conn.is_some() || prog.is_some_and(|prog| prog.running)).then_some((pos.x, pos.y))
        })
        .collect();
    let players = PlayerPositions {
        positions: simulated_positions,
    };
    let player_collect_time = player_collect_t0.elapsed();

    // Full world windows are scanned only after a position transition. The
    // periodic path evaluates the explicit set of known alive cells.
    let collect_t0 = Instant::now();
    let (seed_regions, cells_scanned) = seed_alive_regions(&alive_work, world);
    let mut alive_cells = Vec::new();
    for pos in alive_work.cells() {
        let (x, y): (i32, i32) = pos.into();
        if !world.valid_coord(x, y) {
            alive_work.note_cell(x, y, crate::world::CellType(cell_type::EMPTY));
            continue;
        }
        let cell = world.get_cell_typed(x, y);
        alive_work.note_cell(x, y, cell);
        if is_alive(cell) && players.is_active_for(x, y) {
            alive_cells.push((x, y, cell));
        }
    }
    let collect_time = collect_t0.elapsed();
    alive_work.set_active(!alive_cells.is_empty());

    let mut rng = rand::rng();
    let mut actions: Vec<AliveAction> = Vec::new();
    let mut clears: Vec<(i32, i32)> = Vec::new();

    let eval_t0 = Instant::now();
    for (x, y, cell) in &alive_cells {
        let (x, y, cell) = (*x, *y, *cell);

        // Calculate modifier from adjacent HypnoRock (119) — C# reference.
        let mut modif = 1;
        for &(dx, dy) in &DIRS {
            if world.valid_coord(x + dx, y + dy)
                && world
                    .get_cell_typed(x + dx, y + dy)
                    .is(cell_type::HYPNO_ROCK)
            {
                modif += 2;
            }
        }
        if modif > 1 {
            modif -= 1;
        }

        match cell.0 {
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
    let eval_time = eval_t0.elapsed();

    let dedup_t0 = Instant::now();
    let actions_before_dedup = actions.len();
    let clears_before_dedup = clears.len();
    let actions = dedup_alive_actions(actions);
    clears.sort_unstable();
    clears.dedup();
    let dedup_time = dedup_t0.elapsed();

    // Apply clears first (e.g., AliveWhite destroying sand above).
    let apply_t0 = Instant::now();
    for (cx, cy) in &clears {
        world.set_cell_typed(*cx, *cy, crate::world::CellType(cell_type::EMPTY));
        bcast_q
            .0
            .push(BroadcastEffect::CellUpdate((*cx, *cy).into()));
    }

    // Apply actions.
    for action in &actions {
        if let Some(durability) = action.durability {
            world.write_world_cell(
                action.x,
                action.y,
                crate::world::WorldCell {
                    cell_type: action.cell,
                    durability,
                },
            );
        } else {
            world.set_cell_typed(action.x, action.y, action.cell);
        }
        bcast_q
            .0
            .push(BroadcastEffect::CellUpdate((action.x, action.y).into()));
        alive_work.note_cell(action.x, action.y, action.cell);
    }
    for &(x, y) in &clears {
        alive_work.note_cell(x, y, world.get_cell_typed(x, y));
    }
    let apply_time = apply_t0.elapsed();
    let total = started_at.elapsed();
    let threshold = Duration::from_millis(schedule_cfg.0.schedule_warn_threshold_ms);
    if total > threshold {
        tracing::warn!(
            target: "tickprof",
            players = players.positions.len(),
            seed_regions,
            cells_scanned,
            alive_cells = alive_cells.len(),
            actions = actions.len(),
            clears = clears.len(),
            actions_before_dedup,
            clears_before_dedup,
            player_collect_time = ?player_collect_time,
            collect_time = ?collect_time,
            eval_time = ?eval_time,
            dedup_time = ?dedup_time,
            apply_time = ?apply_time,
            total = ?total,
            threshold = ?threshold,
            "SLOW alive physics system"
        );
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
                cell: crate::world::CellType(cell_type::CYAN),
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
                && world
                    .get_cell_typed(x + cx, y + cy)
                    .is(cell_type::BLACK_ROCK)
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
                cell: crate::world::CellType(cell_type::RED),
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
                && world
                    .get_cell_typed(x + cx, y + cy)
                    .is(cell_type::BLACK_ROCK)
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
                cell: crate::world::CellType(cell_type::VIOLET),
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
                && world
                    .get_cell_typed(x + ax, y + ay)
                    .is(cell_type::ALIVE_BLACK)
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
            cell: crate::world::CellType(cell_type::BLACK_ROCK),
            durability: None,
        });
        return;
    }

    if count > 0 {
        for &(dx, dy) in &DIRS {
            let nx = x + dx;
            let ny = y + dy;
            if world.valid_coord(nx, ny) && world.get_cell_typed(nx, ny).is(cell_type::ALIVE_BLACK)
            {
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
                            cell: crate::world::CellType(cell_type::RED),
                            durability: Some(f32::from(
                                i16::try_from(3 * modif).unwrap_or(i16::MAX),
                            )),
                        });
                    } else {
                        actions.push(AliveAction {
                            x: ox,
                            y: oy,
                            cell: crate::world::CellType(cell_type::CYAN),
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
    let above = world.get_cell_typed(x, y - 1);
    if !cell_defs.get_typed(above).is_sand() {
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
                    cell: crate::world::CellType(cell_type::WHITE),
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
        cell: crate::world::CellType(cell_type::EMPTY),
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
                cell: crate::world::CellType(cell_type::ALIVE_BLUE),
                durability: None,
            });
            // Leave Blue (109) at old position.
            actions.push(AliveAction {
                x,
                y,
                cell: crate::world::CellType(cell_type::BLUE),
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

        let opposite_cell = world.get_cell_typed(ox, oy);
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
            cell: opposite_cell,
            durability: Some(
                target_def.durability * f32::from(i16::try_from(modif).unwrap_or(i16::MAX)),
            ),
        });
    }
}

#[cfg(test)]
mod frontier_tests {
    use super::{ACTIVE_RADIUS, ActiveFrontier, PlayerPositions};
    use std::collections::HashSet;

    fn frontier_cells(positions: &[(i32, i32)]) -> HashSet<(i32, i32)> {
        let players = PlayerPositions {
            positions: positions.iter().copied().collect(),
        };
        ActiveFrontier::around(&players)
            .rows
            .into_iter()
            .flat_map(|(y, intervals)| {
                intervals
                    .into_iter()
                    .flat_map(move |(start, end)| (start..=end).map(move |x| (x, y)))
            })
            .collect()
    }

    fn naive_cells(positions: &[(i32, i32)]) -> HashSet<(i32, i32)> {
        positions
            .iter()
            .flat_map(|&(px, py)| {
                (py - ACTIVE_RADIUS..=py + ACTIVE_RADIUS).flat_map(move |y| {
                    (px - ACTIVE_RADIUS..=px + ACTIVE_RADIUS).map(move |x| (x, y))
                })
            })
            .collect()
    }

    #[test]
    fn active_frontier_matches_exact_union_of_overlapping_windows() {
        let positions = [(10, 10), (10, 10), (15, 12), (100, 50)];
        assert_eq!(frontier_cells(&positions), naive_cells(&positions));
    }
}
