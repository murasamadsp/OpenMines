//! Меню построек и установка здания на карте.
use crate::net::session::play::dig_build::broadcast_cell_update;
use crate::net::session::prelude::*;
use std::collections::HashMap;
use crate::game::buildings::{PackData, PackType, get_building_config};
use crate::game::player::{PlayerPosition, PlayerStats, PlayerUI, PlayerConnection};

// ─── Buildings ─────────────────────────────────────────────────────────

fn pack_block_pos(state: &GameState, x: i32, y: i32) -> Option<i32> {
    if x < 0 || y < 0 { return None; }
    let chunk_x = x / 32;
    let chunk_y = y / 32;
    let width = i32::try_from(state.world.chunks_w()).ok()?;
    let height = i32::try_from(state.world.chunks_h()).ok()?;
    if chunk_x >= width || chunk_y >= height { return None; }
    chunk_y.checked_mul(width)?.checked_add(chunk_x)
}

pub fn handle_buildings_menu(_state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, _pid: PlayerId) {
    let mut buttons = vec![
        "Респ (5000$)", "bld_place:R", "Телепорт (3000$)", "bld_place:T", "Пушка (8000$)", "bld_place:G",
        "UP (4000$)", "bld_place:U", "Склад (4000$)", "bld_place:L", "Крафтер (5000$)", "bld_place:F",
        "Спот (3000$)", "bld_place:O", "Маркет (6000$)", "bld_place:M", "Ворота (2000$)", "bld_place:N",
    ];
    buttons.extend(CLOSE_WINDOW_BUTTON_LABELS.iter().copied());
    let gui = serde_json::json!({ "title": "ПОСТРОЙКИ", "text": "Выберите здание", "buttons": buttons, "back": false });
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
}

pub fn handle_place_building(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId, type_code: &str) {
    let Some(pack_type) = PackType::from_str(type_code) else { return; };
    let Some(cfg) = get_building_config(pack_type) else { return; };
    let cost = cfg.cost;

    let p_data = state.query_player(pid, |ecs, entity| {
        let stats = ecs.get::<PlayerStats>(entity)?;
        let pos = ecs.get::<PlayerPosition>(entity)?;
        Some((stats.clan_id.unwrap_or(0), pos.x, pos.y, pos.dir))
    }).flatten();

    let Some((player_clan, px, py, pdir)) = p_data else { return; };

    if pack_type == PackType::Gate && player_clan == 0 {
        send_u_packet(tx, "OK", &ok_message("Ошибка", "Для ворот нужен клан").1);
        return;
    }

    let (dx, dy) = dir_offset(pdir);
    let (bx, by) = (px + dx * 3, py + dy * 3);

    if let Err(msg) = validate_building_area(state, bx, by, pack_type) {
        send_building_error(tx, msg); return;
    }

    let extra = building_extra_for_pack_type(pack_type);
    let result = state.modify_player(pid, |ecs, entity| {
        let mut s = ecs.get_mut::<PlayerStats>(entity)?;
        if s.money < i64::from(cost) { return None; }
        s.money -= i64::from(cost);
        let m = s.money; let c = s.creds;
        send_u_packet(tx, "P$", &money(m, c).1);
        Some(s.clan_id.unwrap_or(0))
    }).flatten();

    let Some(owner_clan) = result else {
        send_u_packet(tx, "OK", &ok_message("Ошибка", "Недостаточно денег").1);
        return;
    };

    let initial_clan = if pack_type == PackType::Gate { owner_clan } else { 0 };
    let db_id = match state.db.insert_building(type_code, bx, by, pid, initial_clan, &extra) {
        Ok(id) => id,
        Err(_) => {
            state.modify_player(pid, |ecs, entity| {
                if let Some(mut s) = ecs.get_mut::<PlayerStats>(entity) { s.money += i64::from(cost); }
                Some(())
            });
            send_u_packet(tx, "OK", &ok_message("Ошибка", "Ошибка БД").1);
            return;
        }
    };

    let mut pack = make_pack_data(state, db_id, pack_type, bx, by, pid, &extra);
    pack.clan_id = initial_clan;
    place_building_in_world(state, tx, pid, &pack, true);
}

pub fn place_building_in_world(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId, pack: &PackData, close_gui: bool) {
    place_building_cells(state, pack.x, pack.y, pack.pack_type);
    state.packs.insert((pack.x, pack.y), pack.clone());
    broadcast_pack_to_nearby(state, pack);
    if close_gui { send_u_packet(tx, "Gu", &[]); }
    tracing::info!("Player {pid} placed building {} at ({}, {})", pack.pack_type.code(), pack.x, pack.y);
}

pub fn handle_remove_building(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId, bx: i32, by: i32) {
    let actor = state.query_player(pid, |ecs, entity| {
        let p = ecs.get::<PlayerPosition>(entity)?;
        let s = ecs.get::<PlayerStats>(entity)?;
        Some((p.x, p.y, s.clan_id.unwrap_or(0)))
    }).flatten();

    let Some(actor) = actor else { return; };
    let Some(pack) = state.get_pack_at(bx, by).map(|p| p.clone()) else {
        send_building_error(tx, "Объект не найден"); return;
    };

    if pack.owner_id != pid && !(pack.clan_id != 0 && pack.clan_id == actor.2) {
        send_building_error(tx, "Нет прав"); return;
    }

    if !pack.pack_type.building_cells().iter().any(|(dx, dy, _)| pack.x + dx == actor.0 && pack.y + dy == actor.1) {
        send_building_error(tx, "Вы не у объекта"); return;
    }

    if state.db.delete_building(pack.id).is_err() {
        send_u_packet(tx, "OK", &ok_message("Ошибка", "Ошибка БД").1); return;
    }

    state.packs.remove(&(pack.x, pack.y));
    clear_pack_cells(state, &pack);
    broadcast_pack_clear(state, &pack);
    close_pack_windows(state, &pack);
    
    state.modify_player(pid, |ecs, entity| {
        if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) {
            let window = format!("pack:{}:{}", pack.x, pack.y);
            if ui.current_window.as_deref() == Some(window.as_str()) { ui.current_window = None; }
        }
        Some(())
    });
    send_u_packet(tx, "Gu", &[]);
}

pub fn building_extra_for_pack_type(pack_type: PackType) -> BuildingExtra {
    let cfg = get_building_config(pack_type);
    BuildingExtra {
        charge: cfg.as_ref().map_or(0.0, |c| c.charge),
        items_inside: HashMap::new(),
        max_charge: cfg.as_ref().map_or(0.0, |c| c.max_charge),
        cost: cfg.as_ref().map_or(10, |c| c.cost as i32),
        hp: cfg.as_ref().map_or(1000, |c| c.hp),
        max_hp: cfg.as_ref().map_or(1000, |c| c.max_hp),
        money_inside: 0,
        crystals_inside: [0; 6],
        craft_recipe_id: None, craft_num: 0, craft_end_ts: 0,
    }
}

pub fn validate_building_area(state: &Arc<GameState>, bx: i32, by: i32, pack_type: PackType) -> Result<(), &'static str> {
    for &(cdx, cdy, _) in &pack_type.building_cells() {
        let cx = bx + cdx; let cy = by + cdy;
        if !state.world.valid_coord(cx, cy) || !state.world.is_empty(cx, cy) { return Err("Нет места"); }
        if state.find_pack_covering(cx, cy).is_some() { return Err("Место занято"); }
    }
    Ok(())
}

pub fn broadcast_pack_to_nearby(state: &Arc<GameState>, pack: &PackData) {
    broadcast_pack_update(state, pack);
}

fn gather_block_packs(state: &Arc<GameState>, block_pos: i32) -> Vec<(u8, u16, u16, u16, u8)> {
    let width = i32::try_from(state.world.chunks_w()).unwrap_or(0).max(1);
    let chunk_y = block_pos.div_euclid(width);
    let chunk_x = block_pos.rem_euclid(width);
    if chunk_x < 0 || chunk_y < 0 { return vec![]; }
    let mut out = Vec::new();
    for (code, px, py, cid, off) in state.get_packs_in_chunk_area(chunk_x as u32, chunk_y as u32) {
        if let Some(bp) = state.pack_block_pos(i32::from(px), i32::from(py)) {
            if bp == block_pos { out.push((code, px, py, cid, off)); }
        }
    }
    out
}

pub fn broadcast_pack_update(state: &Arc<GameState>, pack: &PackData) {
    if let Some(block_pos) = pack_block_pos(state, pack.x, pack.y) {
        let packs = gather_block_packs(state, block_pos);
        let sub = hb_packs(block_pos, &packs);
        let data = encode_hb_bundle(&hb_bundle(&[sub]).1);
        let (cx, cy) = World::chunk_pos(pack.x, pack.y);
        state.broadcast_to_nearby(cx, cy, &data, None);
    }
}

pub fn broadcast_pack_clear(state: &Arc<GameState>, pack: &PackData) {
    broadcast_pack_update(state, pack);
}

pub fn update_pack_with_db(state: &Arc<GameState>, pack_x: i32, pack_y: i32, mutate: impl FnOnce(&mut PackData)) -> Result<PackData, String> {
    let mut pack = state.packs.get_mut(&(pack_x, pack_y)).ok_or_else(|| "Объект не найден".to_string())?;
    let old = pack.clone(); mutate(&mut pack);
    let extra = building_extra_from_pack(&pack);
    let id = pack.id; let updated = pack.clone(); drop(pack);
    if state.db.update_building_extra(id, &extra).is_err() {
        if let Some(mut cur) = state.packs.get_mut(&(pack_x, pack_y)) { *cur = old; }
        return Err("Ошибка БД".to_string());
    }
    Ok(updated)
}

pub fn update_pack_with_world_sync(state: &Arc<GameState>, pack_x: i32, pack_y: i32, mutate: impl FnOnce(&mut PackData)) -> Result<PackData, String> {
    let old = state.packs.get(&(pack_x, pack_y)).map(|p| p.clone()).ok_or_else(|| "Объект не найден".to_string())?;
    let mut updated = old.clone(); mutate(&mut updated);
    let footprint_changed = old.pack_type != updated.pack_type || old.x != updated.x || old.y != updated.y;
    if footprint_changed { if let Err(msg) = validate_pack_footprint(state, &old, &updated) { return Err(msg.to_string()); } }
    let extra = building_extra_from_pack(&updated);
    if state.db.update_building_state(updated.id, updated.pack_type.code(), updated.x, updated.y, updated.owner_id, updated.clan_id, &extra).is_err() { return Err("Ошибка БД".to_string()); }
    let new_key = (updated.x, updated.y);
    if (old.x, old.y) != new_key && state.packs.contains_key(&new_key) { return Err("Место занято".to_string()); }
    state.packs.remove(&(old.x, old.y));
    state.packs.insert(new_key, updated.clone());
    if footprint_changed { broadcast_pack_clear(state, &old); clear_pack_cells(state, &old); place_pack_cells(state, &updated); }
    broadcast_pack_update(state, &updated);
    Ok(updated)
}

fn place_pack_cells(state: &Arc<GameState>, pack: &PackData) {
    for (dx, dy, cell) in pack.pack_type.building_cells() {
        state.world.set_cell(pack.x + dx, pack.y + dy, cell);
        broadcast_cell_update(state, pack.x + dx, pack.y + dy);
    }
}

fn pack_has_cell(pack: &PackData, cx: i32, cy: i32) -> bool {
    pack.pack_type.building_cells().iter().any(|(dx, dy, _)| pack.x + dx == cx && pack.y + dy == cy)
}

fn validate_pack_footprint(state: &Arc<GameState>, old: &PackData, updated: &PackData) -> Result<(), &'static str> {
    for (dx, dy, _) in updated.pack_type.building_cells() {
        let tx = updated.x + dx; let ty = updated.y + dy;
        if !state.world.valid_coord(tx, ty) { return Err("Нет места"); }
        if !state.world.is_empty(tx, ty) && !pack_has_cell(old, tx, ty) { return Err("Нет места"); }
        if let Some((px, py)) = state.find_pack_covering(tx, ty) { if px != old.x || py != old.y { return Err("Место занято"); } }
    }
    Ok(())
}

pub fn make_pack_data(state: &Arc<GameState>, db_id: i32, pack_type: PackType, x: i32, y: i32, pid: PlayerId, extra: &BuildingExtra) -> PackData {
    let ecs_entity = state.ecs.write().spawn((
        crate::game::buildings::Position { x, y },
        crate::game::buildings::Building { id: db_id, type_code: pack_type.code() },
        crate::game::buildings::Owner { pid, clan_id: 0 },
        crate::game::buildings::Health { state: extra.hp, max_state: extra.max_hp },
    )).id();

    PackData {
        id: db_id, ecs_entity, pack_type, x, y, owner_id: pid, clan_id: 0,
        charge: extra.charge, max_charge: extra.max_charge, cost: extra.cost, hp: extra.hp, max_hp: extra.max_hp,
        money_inside: extra.money_inside, crystals_inside: extra.crystals_inside, items_inside: extra.items_inside.clone(),
        craft_recipe_id: extra.craft_recipe_id, craft_num: extra.craft_num, craft_end_ts: extra.craft_end_ts,
    }
}

pub fn building_extra_from_pack(pack: &PackData) -> BuildingExtra {
    BuildingExtra {
        charge: pack.charge, max_charge: pack.max_charge, cost: pack.cost, hp: pack.hp, max_hp: pack.max_hp,
        money_inside: pack.money_inside, crystals_inside: pack.crystals_inside, items_inside: pack.items_inside.clone(),
        craft_recipe_id: pack.craft_recipe_id, craft_num: pack.craft_num, craft_end_ts: pack.craft_end_ts,
    }
}

fn send_building_error(tx: &mpsc::UnboundedSender<Vec<u8>>, text: &str) {
    send_u_packet(tx, "OK", &ok_message("Ошибка", text).1);
}

fn close_pack_windows(state: &Arc<GameState>, pack: &PackData) {
    let window_key = format!("pack:{}:{}", pack.x, pack.y);
    let (pcx, pcy) = World::chunk_pos(pack.x, pack.y);
    for (cx, cy) in state.visible_chunks_around(pcx, pcy) {
        if let Some(players) = state.chunk_players.get(&(cx, cy)) {
            let ids: Vec<PlayerId> = players.value().clone();
            for pid in ids {
                state.modify_player(pid, |ecs, entity| {
                    if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) {
                        if ui.current_window.as_deref() == Some(window_key.as_str()) {
                            ui.current_window = None;
                            if let Some(conn) = ecs.get::<PlayerConnection>(entity) {
                                let _ = conn.tx.send(make_u_packet_bytes("Gu", &[]));
                            }
                        }
                    }
                    Some(())
                });
            }
        }
    }
}

pub fn place_building_cells(state: &Arc<GameState>, bx: i32, by: i32, pack_type: PackType) {
    for &(cdx, cdy, cell) in &pack_type.building_cells() {
        state.world.set_cell(bx + cdx, by + cdy, cell);
        broadcast_cell_update(state, bx + cdx, by + cdy);
    }
}

pub fn clear_pack_cells(state: &Arc<GameState>, pack: &PackData) {
    for (cdx, cdy, _) in pack.pack_type.building_cells() {
        state.world.set_cell(pack.x + cdx, pack.y + cdy, cell_type::EMPTY);
        broadcast_cell_update(state, pack.x + cdx, pack.y + cdy);
    }
}
