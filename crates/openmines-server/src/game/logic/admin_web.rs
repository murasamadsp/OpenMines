#![allow(clippy::needless_pass_by_value)]
use crate::game::GameState;
use crate::game::player::PlayerId;
use std::sync::Arc;

pub fn apply_role_to_ecs(state: &Arc<GameState>, pid: PlayerId, role: i32) {
    state.modify_player(pid, |ecs, entity| {
        if let Some(mut stats) = ecs.get_mut::<crate::game::player::PlayerStats>(entity) {
            stats.role = role;
        }
        if let Some(mut flags) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) {
            flags.dirty = true;
        }
        Some(())
    });
}
