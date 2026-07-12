use crate::game::buildings::BuildingStorage;
use crate::game::player::{PlayerFlags, PlayerPosition, PlayerStats, PlayerUI};
use crate::net::session::prelude::*;
use crate::net::session::social::buildings::modify_pack_with_db;

use super::crystal_form::parse_amounts;
use super::pack_command::{send_action_error, send_state_error, withdraw_state_ready};

pub(super) fn open(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, view: &PackView) {
    let storage_crystals = state.query_building_opt(view.x, view.y, |ecs, entity| {
        Some(ecs.get::<BuildingStorage>(entity)?.crystals)
    });
    let Some(storage_crystals) = storage_crystals else {
        tracing::error!(
            x = view.x,
            y = view.y,
            "Building storage missing for storage GUI"
        );
        send_action_error(tx);
        return;
    };
    let player_crystals = state.query_player_opt(pid, |ecs, entity| {
        Some(ecs.get::<PlayerStats>(entity)?.crystals)
    });
    let Some(player_crystals) = player_crystals else {
        tracing::error!(player_id = %pid, "Player stats missing for storage GUI");
        send_action_error(tx);
        return;
    };

    let crystal_lines = (0..6)
        .map(|index| {
            let total = player_crystals[index] + storage_crystals[index];
            format!("0:0:{total}:{}:", storage_crystals[index])
        })
        .collect();

    use super::horb::{Button, Horb};
    Horb::new("Склад")
        .crystals(" ", " ", false, crystal_lines)
        .button(Button::new("Передать", "transfer:%M%"))
        .button(Button::new(
            "Удалить",
            format!("pack_op:remove:{}:{}", view.x, view.y),
        ))
        .close_button()
        .send(state, tx, pid, format!("pack:{}:{}", view.x, view.y));
}

pub(super) fn apply(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, payload: &str) {
    let Some(requested_storage) = parse_amounts(payload) else {
        return;
    };
    let coords = state.query_player_opt(pid, |ecs, entity| {
        let window = ecs.get::<PlayerUI>(entity)?.current_window.as_deref()?;
        let parts: Vec<&str> = window.strip_prefix("pack:")?.split(':').collect();
        (parts.len() == 2)
            .then(|| Some((parts[0].parse::<i32>().ok()?, parts[1].parse::<i32>().ok()?)))?
    });
    let Some((x, y)) = coords else {
        return;
    };
    let Some(view) = state.get_pack_at(x, y) else {
        return;
    };
    if view.pack_type != PackType::Storage {
        return;
    }

    let access = state.query_player_opt(pid, |ecs, entity| {
        let position = ecs.get::<PlayerPosition>(entity)?;
        let player_stats = ecs.get::<PlayerStats>(entity)?;
        Some((position.x, position.y, player_stats.clan_id.unwrap_or(0)))
    });
    let Some((player_x, player_y, player_clan)) = access else {
        return;
    };
    if validate_pack_access(&view, (player_x, player_y), player_clan, pid).is_err() {
        return;
    }
    let Some(player_entity) = state.get_player_entity(pid) else {
        return;
    };
    if !withdraw_state_ready(state, pid, x, y) {
        send_state_error(tx);
        return;
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
        Ok(None) => return,
        Err(error) => {
            tracing::error!(x, y, error = %error, "Storage transfer failed");
            send_state_error(tx);
            return;
        }
    };
    send_u_packet(tx, "@B", &basket(&new_player, 1).1);
    open(state, tx, pid, &view);
}
