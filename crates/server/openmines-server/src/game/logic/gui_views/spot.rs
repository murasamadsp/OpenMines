use crate::game::player::{PlayerPosition, PlayerStats, PlayerUI};
use crate::game::{GameState, PackType, PlayerId, SpotGuiView};
use std::sync::Arc;

pub fn prepare_view(state: &Arc<GameState>, pid: PlayerId, x: i32, y: i32) -> Option<SpotGuiView> {
    let view = state.get_pack_at(x, y)?;
    if view.pack_type != PackType::Spot || view.owner_id != pid {
        return None;
    }
    let (player_x, player_y, player_clan) = state.query_player_opt(pid, |ecs, entity| {
        let position = ecs.get::<PlayerPosition>(entity)?;
        let player_stats = ecs.get::<PlayerStats>(entity)?;
        Some((position.x, position.y, player_stats.clan_id.unwrap_or(0)))
    })?;
    crate::game::buildings::validate_pack_access(&view, (player_x, player_y), player_clan, pid)
        .ok()?;
    Some(SpotGuiView { x, y })
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
