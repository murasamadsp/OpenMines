use crate::game::buildings::BuildingStorage;
use crate::game::player::{PlayerFlags, PlayerPosition, PlayerStats, PlayerUI};
use crate::net::session::prelude::*;
use crate::net::session::social::buildings::modify_pack_with_db;

use super::crystal_form::parse_amounts;
use super::pack_command::withdraw_state_ready;

#[derive(Debug, Clone)]
pub struct StorageTransfer {
    pub view: crate::game::StorageGuiView,
    pub crystals: [i64; 6],
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum StorageTransferError {
    MissingState,
}

pub fn prepare_view(
    state: &Arc<GameState>,
    pid: PlayerId,
    x: i32,
    y: i32,
) -> Option<crate::game::StorageGuiView> {
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
    validate_pack_access(&view, (player_x, player_y), player_clan, pid).ok()?;
    let storage_crystals = state.query_building_opt(x, y, |ecs, entity| {
        Some(ecs.get::<BuildingStorage>(entity)?.crystals)
    })?;
    let crystal_lines = (0..6)
        .map(|index| {
            let total = player_crystals[index] + storage_crystals[index];
            format!("0:0:{total}:{}:", storage_crystals[index])
        })
        .collect();
    Some(crate::game::StorageGuiView {
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

pub fn render(view: &crate::game::StorageGuiView) -> Vec<u8> {
    use super::horb::{Button, Horb};
    Horb::new("Склад")
        .crystals(" ", " ", false, view.crystal_lines.clone())
        .button(Button::new("Передать", "transfer:%M%"))
        .button(Button::new(
            "Удалить",
            format!("pack_op:remove:{}:{}", view.x, view.y),
        ))
        .close_button()
        .payload()
}

pub fn transfer(
    state: &Arc<GameState>,
    pid: PlayerId,
    payload: &str,
) -> Result<Option<StorageTransfer>, StorageTransferError> {
    let Some(requested_storage) = parse_amounts(payload) else {
        return Ok(None);
    };
    let coords = state.query_player_opt(pid, |ecs, entity| {
        let window = ecs.get::<PlayerUI>(entity)?.current_window.as_deref()?;
        let parts: Vec<&str> = window.strip_prefix("pack:")?.split(':').collect();
        (parts.len() == 2)
            .then(|| Some((parts[0].parse::<i32>().ok()?, parts[1].parse::<i32>().ok()?)))?
    });
    let Some((x, y)) = coords else {
        return Ok(None);
    };
    let Some(view) = state.get_pack_at(x, y) else {
        return Ok(None);
    };
    if view.pack_type != PackType::Storage {
        return Ok(None);
    }

    let access = state.query_player_opt(pid, |ecs, entity| {
        let position = ecs.get::<PlayerPosition>(entity)?;
        let player_stats = ecs.get::<PlayerStats>(entity)?;
        Some((position.x, position.y, player_stats.clan_id.unwrap_or(0)))
    });
    let Some((player_x, player_y, player_clan)) = access else {
        return Ok(None);
    };
    if validate_pack_access(&view, (player_x, player_y), player_clan, pid).is_err() {
        return Ok(None);
    }
    let Some(player_entity) = state.get_player_entity(pid) else {
        return Ok(None);
    };
    if !withdraw_state_ready(state, pid, x, y) {
        return Err(StorageTransferError::MissingState);
    }

    let result = modify_pack_with_db(state, x, y, |ecs, building_entity| {
        let storage = ecs
            .get::<BuildingStorage>(building_entity)
            .expect("BuildingStorage checked before storage transfer")
            .crystals;
        let player = ecs
            .get::<PlayerStats>(player_entity)
            .expect("PlayerStats checked before storage transfer")
            .crystals;
        let mut new_player = [0_i64; 6];
        for index in 0..6 {
            let total = player[index] + storage[index];
            if requested_storage[index] < 0 || total - requested_storage[index] < 0 {
                return None;
            }
            new_player[index] = total - requested_storage[index];
        }
        ecs.get_mut::<BuildingStorage>(building_entity)
            .expect("BuildingStorage checked before storage transfer")
            .crystals = requested_storage;
        ecs.get_mut::<PlayerStats>(player_entity)
            .expect("PlayerStats checked before storage transfer")
            .crystals = new_player;
        ecs.get_mut::<PlayerFlags>(player_entity)
            .expect("PlayerFlags checked before storage transfer")
            .dirty = true;
        Some(new_player)
    });

    let new_player = match result {
        Ok(Some(crystals)) => crystals,
        Ok(None) => return Ok(None),
        Err(error) => {
            tracing::error!(x, y, error = %error, "Storage transfer failed");
            return Err(StorageTransferError::MissingState);
        }
    };
    let Some(view) = prepare_view(state, pid, x, y) else {
        return Err(StorageTransferError::MissingState);
    };
    Ok(Some(StorageTransfer {
        view,
        crystals: new_player,
    }))
}
