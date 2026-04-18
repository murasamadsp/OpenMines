//! Обработка нажатий GUI-кнопок игроком.
use crate::net::session::outbound::inventory_sync::send_inventory;
use crate::net::session::play::chunks::check_chunk_changed;
use crate::net::session::prelude::*;
use crate::net::session::social::buildings::{
    building_extra_from_pack, building_extra_for_pack_type, update_pack_with_db,
};
use crate::game::player::{PlayerPosition, PlayerStats, PlayerInventory, PlayerSkills, PlayerUI, PlayerFlags, PlayerMetadata};
use crate::game::buildings::{PackData, PackType};

pub fn handle_gui_button(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId, button: &str) {
    if button == "exit" || button == "close" {
        state.modify_player(pid, |ecs, entity| {
            if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) { ui.current_window = None; }
            Some(())
        });
        send_u_packet(tx, "Gu", &[]);
        return;
    }

    if let Some(rest) = button.strip_prefix("clan_view:") {
        if let Ok(id) = rest.parse::<i32>() {
            crate::net::session::social::clans::handle_clan_preview(state, tx, pid, id);
        }
        return;
    }

    match button {
        "clan_menu" => crate::net::session::social::clans::handle_clan_menu(state, tx, pid),
        "clan_back" => crate::net::session::social::clans::handle_clan_menu(state, tx, pid),
        "clan_create_view" => handle_clan_create_view(tx),
        "clan_requests" => crate::net::session::social::clans::handle_clan_requests_view(state, tx, pid),
        "clan_members" => crate::net::session::social::clans::handle_clan_members_view(state, tx, pid),
        "clan_invite_list" => crate::net::session::social::clans::handle_clan_invite_list(state, tx, pid),
        "clan_invites_view" => crate::net::session::social::clans::handle_clan_invites_view(state, tx, pid),
        "clan_leave" => crate::net::session::social::clans::handle_clan_leave(state, tx, pid),
        _ => handle_complex_button(state, tx, pid, button),
    }
}

fn handle_clan_create_view(tx: &mpsc::UnboundedSender<Vec<u8>>) {
    let gui = serde_json::json!({
        "title": "СОЗДАНИЕ КЛАНА",
        "text": "Введите название и тег (3 симв.) через пробел в чат после нажатия кнопки 'ВВОД'",
        "buttons": ["ВВОД", "clan_create_input", "Назад", "clan_back"],
        "back": false
    });
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
}

fn handle_complex_button(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId, button: &str) {
    if let Some(rest) = button.strip_prefix("clan_request:") {
        if let Ok(id) = rest.parse::<i32>() {
            crate::net::session::social::clans::handle_clan_join_request(state, tx, pid, id);
        }
    } else if let Some(rest) = button.strip_prefix("clan_accept:") {
        if let Ok(id) = rest.parse::<i32>() {
            crate::net::session::social::clans::handle_clan_accept(state, tx, pid, id);
        }
    } else if let Some(rest) = button.strip_prefix("clan_invite_send:") {
        if let Ok(id) = rest.parse::<i32>() {
            crate::net::session::social::clans::handle_clan_invite_send(state, tx, pid, id);
        }
    } else if let Some(rest) = button.strip_prefix("clan_invite_accept:") {
        if let Ok(id) = rest.parse::<i32>() {
            crate::net::session::social::clans::handle_clan_invite_accept(state, tx, pid, id);
        }
    } else if let Some(rest) = button.strip_prefix("bld_place:") {
        crate::net::session::social::buildings::handle_place_building(state, tx, pid, rest);
    } else if let Some(rest) = button.strip_prefix("pack_op:") {
        handle_pack_operation(state, tx, pid, rest);
    }
}

fn handle_pack_operation(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId, op: &str) {
    let parts: Vec<&str> = op.split(':').collect();
    if parts.len() < 3 { return; }
    let cmd = parts[0];
    let x = parts[1].parse::<i32>().unwrap_or(0);
    let y = parts[2].parse::<i32>().unwrap_or(0);

    let Some(pack) = state.get_pack_at(x, y).map(|p| p.clone()) else { return; };
    
    let p_info = state.query_player(pid, |ecs, entity| {
        let pos = ecs.get::<PlayerPosition>(entity)?;
        let stats = ecs.get::<PlayerStats>(entity)?;
        Some((pos.x, pos.y, stats.clan_id.unwrap_or(0)))
    }).flatten();
    
    let Some((px, py, p_clan)) = p_info else { return; };
    if validate_pack_access(&pack, (px, py), p_clan, pid).is_err() { return; }

    match cmd {
        "open" => open_pack_gui(state, tx, pid, &pack),
        "take_money" => handle_pack_take_money(state, tx, pid, &pack),
        "take_crys" => handle_pack_take_crystals(state, tx, pid, &pack),
        "remove" => crate::net::session::social::buildings::handle_remove_building(state, tx, pid, x, y),
        _ => {}
    }
}

fn open_pack_gui(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId, pack: &PackData) {
    let title = pack.pack_type.name();
    let text = format!("Здание: {}\nЗаряд: {:.1}/{:.1}\nПрочность: {}/{}", title, pack.charge, pack.max_charge, pack.hp, pack.max_hp);
    let mut buttons = vec![
        serde_json::json!("Забрать деньги"), serde_json::json!(format!("pack_op:take_money:{}:{}", pack.x, pack.y)),
        serde_json::json!("Забрать кристаллы"), serde_json::json!(format!("pack_op:take_crys:{}:{}", pack.x, pack.y)),
        serde_json::json!("Удалить"), serde_json::json!(format!("pack_op:remove:{}:{}", pack.x, pack.y)),
    ];
    buttons.extend(CLOSE_WINDOW_BUTTON_LABELS.iter().map(|l| serde_json::json!(l)));
    
    let gui = serde_json::json!({ "title": title, "text": text, "buttons": buttons, "back": false });
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
    
    state.modify_player(pid, |ecs, entity| {
        if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) { ui.current_window = Some(format!("pack:{}:{}", pack.x, pack.y)); }
        Some(())
    });
}

fn handle_pack_take_money(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId, pack: &PackData) {
    if pack.money_inside <= 0 { return; }
    let amount = pack.money_inside;
    
    if update_pack_with_db(state, pack.x, pack.y, |p| { p.money_inside = 0; }).is_ok() {
        state.modify_player(pid, |ecs, entity| {
            let mut s = ecs.get_mut::<PlayerStats>(entity)?;
            s.money += amount;
            send_u_packet(tx, "P$", &money(s.money, s.creds).1);
            Some(())
        });
    }
}

fn handle_pack_take_crystals(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId, pack: &PackData) {
    let amount = pack.crystals_inside;
    if amount.iter().sum::<i64>() <= 0 { return; }
    
    if update_pack_with_db(state, pack.x, pack.y, |p| { p.crystals_inside = [0; 6]; }).is_ok() {
        state.modify_player(pid, |ecs, entity| {
            let mut s = ecs.get_mut::<PlayerStats>(entity)?;
            for i in 0..6 { s.crystals[i] += amount[i]; }
            send_u_packet(tx, "@B", &basket(&s.crystals, 1000).1);
            Some(())
        });
    }
}
