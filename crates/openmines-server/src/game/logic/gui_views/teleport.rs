use crate::game::buildings::{BuildingMetadata, BuildingStats, GridPosition};
use crate::game::player::{PlayerPosition, PlayerStats, PlayerUI};
use crate::game::{GameState, PackType, PackView, PlayerId, TeleportGuiView, WorldPos};
use crate::world::{World, WorldProvider};
use std::sync::Arc;

const RANGE_CELLS: i32 = 1_000;
const MAP_RADIUS_CHUNKS: i32 = 8;

pub fn prepare_view(
    state: &Arc<GameState>,
    pid: PlayerId,
    x: i32,
    y: i32,
) -> Option<TeleportGuiView> {
    let view = state.get_pack_at(x, y)?;
    if view.pack_type != PackType::Teleport {
        return None;
    }
    let (px, py, player_clan) = state.query_player_opt(pid, |ecs, entity| {
        let position = ecs.get::<PlayerPosition>(entity)?;
        let player_state = ecs.get::<PlayerStats>(entity)?;
        Some((position.x, position.y, player_state.clan_id.unwrap_or(0)))
    })?;
    if !can_open(&view, (px, py), player_clan) {
        return None;
    }

    let mut destinations = nearby_destinations(state, &view);
    destinations.sort_unstable();
    let map_tiles = capture_map(state, (view.x, view.y).into());
    Some(TeleportGuiView {
        source: (view.x, view.y).into(),
        charge: view.charge,
        hp: view.hp,
        max_hp: view.max_hp,
        destinations,
        map_tiles,
    })
}

pub fn activate_window(state: &Arc<GameState>, pid: PlayerId, x: i32, y: i32) -> bool {
    state
        .modify_player(pid, |ecs, entity| {
            ecs.get_mut::<PlayerUI>(entity)?.current_window = Some(format!("pack:{x}:{y}"));
            Some(())
        })
        .flatten()
        .is_some()
}

fn can_open(view: &PackView, player_pos: (i32, i32), player_clan: i32) -> bool {
    let Ok(cells) = view.pack_type.building_cells() else {
        return false;
    };
    cells
        .iter()
        .any(|(dx, dy, _)| view.x + dx == player_pos.0 && view.y + dy == player_pos.1)
        && (view.clan_id == 0 || view.clan_id == player_clan)
}

#[allow(clippy::significant_drop_tightening)]
fn nearby_destinations(state: &Arc<GameState>, source: &PackView) -> Vec<WorldPos> {
    let source_chunk = World::chunk_pos(source.x, source.y);
    let chunk_radius = u32::try_from(RANGE_CELLS.div_euclid(32) + 1).unwrap_or(0);
    let world_last_chunk = (
        state.world.chunks_w().saturating_sub(1),
        state.world.chunks_h().saturating_sub(1),
    );
    let chunk_min = (
        source_chunk.0.saturating_sub(chunk_radius),
        source_chunk.1.saturating_sub(chunk_radius),
    );
    let chunk_max = (
        source_chunk
            .0
            .saturating_add(chunk_radius)
            .min(world_last_chunk.0),
        source_chunk
            .1
            .saturating_add(chunk_radius)
            .min(world_last_chunk.1),
    );
    let ecs = state.ecs_read_profiled("gui.teleport_view");
    let mut destinations = Vec::new();
    for chunk in (chunk_min.1..=chunk_max.1)
        .flat_map(|row| (chunk_min.0..=chunk_max.0).map(move |column| (column, row)))
    {
        for entity in state.building_entities_in_chunk_snapshot(chunk.0, chunk.1) {
            if ecs
                .get::<crate::game::BuildingDeletePending>(entity)
                .is_some()
            {
                continue;
            }
            let (Some(metadata), Some(position), Some(building_state)) = (
                ecs.get::<BuildingMetadata>(entity),
                ecs.get::<GridPosition>(entity),
                ecs.get::<BuildingStats>(entity),
            ) else {
                continue;
            };
            if metadata.pack_type == PackType::Teleport
                && building_state.charge > 0
                && (position.x != source.x || position.y != source.y)
                && (position.x - source.x).abs() < RANGE_CELLS
                && (position.y - source.y).abs() < RANGE_CELLS
            {
                destinations.push((position.x, position.y).into());
            }
        }
    }
    destinations
}

fn capture_map(state: &Arc<GameState>, center: WorldPos) -> Vec<Option<bool>> {
    let center_chunk = (center.0.div_euclid(32), center.1.div_euclid(32));
    let mut tiles =
        Vec::with_capacity(usize::try_from((MAP_RADIUS_CHUNKS * 2 + 1).pow(2)).unwrap_or(0));
    for dy in -MAP_RADIUS_CHUNKS..=MAP_RADIUS_CHUNKS {
        for dx in -MAP_RADIUS_CHUNKS..=MAP_RADIUS_CHUNKS {
            let (x, y) = (
                (center_chunk.0 + dx) * 32 + 16,
                (center_chunk.1 + dy) * 32 + 16,
            );
            tiles.push(
                state
                    .world
                    .valid_coord(x, y)
                    .then(|| state.world.is_empty(x, y)),
            );
        }
    }
    tiles
}
