use crate::game::buildings::BuildingStorage;
use crate::game::player::{PlayerPosition, PlayerStats, PlayerUI};
use crate::game::{GameState, PackType, PlayerId, StorageGuiView};
use std::sync::Arc;

pub fn prepare_view(
    state: &Arc<GameState>,
    pid: PlayerId,
    x: i32,
    y: i32,
) -> Option<StorageGuiView> {
    let view = state.get_pack_at(x, y)?;
    if view.pack_type != PackType::Storage {
        return None;
    }
    let (player_x, player_y, player_clan, player_crystals) =
        state.query_player_opt(pid, |ecs, entity| {
            let position = ecs.get::<PlayerPosition>(entity)?;
            let player_stats = ecs.get::<PlayerStats>(entity)?;
            Some((
                position.x,
                position.y,
                player_stats.clan_id.unwrap_or(0),
                player_stats.crystals,
            ))
        })?;
    crate::game::buildings::validate_pack_access(&view, (player_x, player_y), player_clan, pid)
        .ok()?;
    let storage_crystals = state.query_building_opt(x, y, |ecs, entity| {
        Some(ecs.get::<BuildingStorage>(entity)?.crystals)
    })?;
    let crystal_lines = (0..6)
        .map(|index| {
            let total = player_crystals[index] + storage_crystals[index];
            format!("0:0:{total}:{}:", storage_crystals[index])
        })
        .collect();
    Some(StorageGuiView {
        x,
        y,
        crystal_lines,
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
