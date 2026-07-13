use crate::game::buildings::{BuildingMetadata, BuildingStats, GridPosition};
use crate::game::player::{PlayerPosition, PlayerStats, PlayerUI};
use crate::net::session::prelude::*;

const RANGE_CELLS: i32 = 1_000;
const MAP_RADIUS_CHUNKS: i32 = 8;

pub fn prepare_view(
    state: &Arc<GameState>,
    pid: PlayerId,
    x: i32,
    y: i32,
) -> Option<crate::game::TeleportGuiView> {
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
    state
        .modify_player(pid, |ecs, entity| {
            ecs.get_mut::<PlayerUI>(entity)?.current_window = Some(format!("pack:{x}:{y}"));
            Some(())
        })
        .flatten()?;

    Some(crate::game::TeleportGuiView {
        source: (view.x, view.y).into(),
        charge: view.charge,
        hp: view.hp,
        max_hp: view.max_hp,
        destinations,
        map_tiles,
    })
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

fn nearby_destinations(state: &Arc<GameState>, source: &PackView) -> Vec<crate::game::WorldPos> {
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

fn capture_map(state: &Arc<GameState>, center: crate::game::WorldPos) -> Vec<Option<bool>> {
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

pub fn render(view: &crate::game::TeleportGuiView) -> Vec<u8> {
    use super::horb::{Button, Horb, ListRow};

    let text = if view.destinations.is_empty() {
        format!(
            "Заряд: {}\nПрочность: {}/{}\n\nНет доступных телепортов поблизости.",
            view.charge, view.hp, view.max_hp
        )
    } else {
        format!(
            "Заряд: {}\nПрочность: {}/{}\n\nДоступные телепорты:",
            view.charge, view.hp, view.max_hp
        )
    };

    let markers: Vec<(i32, i32, String)> = view
        .destinations
        .iter()
        .map(|pos| (pos.0, pos.1, format!("tp:{}:{}", pos.0, pos.1)))
        .collect();
    let mut win = Horb::new("Тп").text(text).minimap(
        view.source.0,
        view.source.1,
        MAP_RADIUS_CHUNKS,
        |x, y| map_tile(view, x, y),
        &markers,
    );

    for destination in &view.destinations {
        win = win.list_row(ListRow::new(
            format!("TP {}:{}", destination.0, destination.1),
            "ТЕЛЕПОРТ",
            format!("tp:{}:{}", destination.0, destination.1),
        ));
    }
    win.button(Button::new(
        "Забрать деньги",
        format!("pack_op:take_money:{}:{}", view.source.0, view.source.1),
    ))
    .button(Button::new(
        "Забрать кристаллы",
        format!("pack_op:take_crys:{}:{}", view.source.0, view.source.1),
    ))
    .button(Button::new(
        "Удалить",
        format!("pack_op:remove:{}:{}", view.source.0, view.source.1),
    ))
    .close_button()
    .payload()
}

fn map_tile(view: &crate::game::TeleportGuiView, x: i32, y: i32) -> Option<bool> {
    let center_chunk = (view.source.0.div_euclid(32), view.source.1.div_euclid(32));
    let dx = x.div_euclid(32) - center_chunk.0;
    let dy = y.div_euclid(32) - center_chunk.1;
    if dx.abs() > MAP_RADIUS_CHUNKS || dy.abs() > MAP_RADIUS_CHUNKS {
        return None;
    }
    let side = MAP_RADIUS_CHUNKS * 2 + 1;
    let index = (dy + MAP_RADIUS_CHUNKS) * side + dx + MAP_RADIUS_CHUNKS;
    view.map_tiles.get(usize::try_from(index).ok()?).copied()?
}

pub(super) fn open(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, view: &PackView) {
    let Some(view) = prepare_view(state, pid, view.x, view.y) else {
        return;
    };
    send_u_packet(tx, "GU", &render(&view));
}

pub(super) fn apply(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, coords: &str) {
    let parts: Vec<&str> = coords.split(':').collect();
    if parts.len() != 2 {
        return;
    }
    let Ok(dest_x) = parts[0].parse::<i32>() else {
        return;
    };
    let Ok(dest_y) = parts[1].parse::<i32>() else {
        return;
    };

    let Some(dest_view) = state.get_pack_at(dest_x, dest_y) else {
        tracing::warn!(player_id = %pid, destination_x = dest_x, destination_y = dest_y, "TP action: destination not found");
        return;
    };
    if dest_view.pack_type != PackType::Teleport || dest_view.charge <= 0 {
        tracing::warn!(player_id = %pid, destination_x = dest_x, destination_y = dest_y, "TP action: destination not a valid teleport");
        return;
    }

    let src_coords = state.query_player_opt(pid, |ecs, entity| {
        let ui = ecs.get::<PlayerUI>(entity)?;
        let window = ui.current_window.as_deref()?;
        let rest = window.strip_prefix("pack:")?;
        let p: Vec<&str> = rest.split(':').collect();
        if p.len() == 2 {
            Some((p[0].parse::<i32>().ok()?, p[1].parse::<i32>().ok()?))
        } else {
            None
        }
    });
    let Some((src_x, src_y)) = src_coords else {
        tracing::warn!(player_id = %pid, "TP action: player not at a teleport window");
        return;
    };
    let Some(src_view) = state.get_pack_at(src_x, src_y) else {
        return;
    };
    if src_view.pack_type != PackType::Teleport || src_view.charge <= 0 {
        return;
    }

    state.modify_player(pid, |ecs, entity| {
        ecs.get_mut::<PlayerUI>(entity)?.current_window = None;
        Some(())
    });
    let close = gu_close();
    send_u_packet(tx, close.0, &close.1);

    let tp_y = dest_y + 3;
    state.modify_player(pid, |ecs, entity| {
        let mut position = ecs.get_mut::<PlayerPosition>(entity)?;
        position.x = dest_x;
        position.y = tp_y;
        Some(())
    });
    if let Some(entity) = state.get_player_entity(pid) {
        state.schedule_hazard(entity, std::time::Instant::now());
    }
    state.seed_granular_region(dest_x, tp_y);
    let packet = tp(dest_x, tp_y);
    send_u_packet(tx, packet.0, &packet.1);
    crate::net::session::play::chunks::check_chunk_changed(state, tx, pid);

    tracing::info!(player_id = %pid, from_x = src_x, from_y = src_y, to_x = dest_x, to_y = tp_y, "Teleported player");
}
