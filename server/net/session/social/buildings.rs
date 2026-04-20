//! Меню построек и установка здания на карте.
use crate::net::session::play::dig_build::broadcast_cell_update;
use crate::net::session::prelude::*;
use std::collections::HashMap;
use bevy_ecs::prelude::{Entity, World as EcsWorld};
use crate::game::buildings::{PackType, PackView, get_building_config, BuildingMetadata, BuildingStats, BuildingStorage, BuildingCrafting, BuildingOwnership, GridPosition, BuildingFlags};
use crate::game::player::{PlayerPosition, PlayerStats, PlayerUI, PlayerConnection};

// ─── Buildings ─────────────────────────────────────────────────────────

/// TY `Pope` → `StaticGUI.OpenGui` в `server_reference/.../StaticGUI.cs` (программатор).
pub fn handle_programmator_pope_menu(_state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, _pid: PlayerId) {
    let gui = serde_json::json!({
        "title": "ПРОГРАММАТОР",
        "text": "",
        "buttons": ["СОЗДАТЬ ПРОГРАММУ", "createprog_stub", "ВЫЙТИ", "exit"],
        "back": false
    });
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
}

/// TY `Blds` → `Player.OpenMyBuildings()` (список построек владельца).
pub fn handle_my_buildings_list(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId) {
    let mine: Vec<crate::db::buildings::BuildingRow> = state
        .db
        .load_all_buildings()
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.owner_id == pid)
        .collect();
    let text = if mine.is_empty() {
        "(нет построек)".to_string()
    } else {
        mine.iter()
            .map(|r| format!("{} {}:{}", r.type_code, r.x, r.y))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let gui = serde_json::json!({
        "title": "мои здания да",
        "text": text,
        "buttons": ["ВЫЙТИ", "exit"],
        "back": false
    });
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
}

/// TY `DPBX` → `Basket.OpenBoxGui` (упрощённо: показать кристаллы).
pub fn handle_dpbx_crystal_box(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId) {
    let Some(cry) = state
        .query_player(pid, |ecs, e| ecs.get::<PlayerStats>(e).map(|s| s.crystals))
        .flatten()
    else {
        return;
    };
    let lines: Vec<String> = cry
        .iter()
        .enumerate()
        .map(|(i, n)| format!("тип {i}: {n}"))
        .collect();
    let gui = serde_json::json!({
        "title": "Создание бокса",
        "text": format!("Кристаллы (как ref OpenBoxGui, без слайдеров):\n{}", lines.join("\n")),
        "buttons": ["ВЫЙТИ", "exit"],
        "back": false
    });
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
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

    let entity = state.ecs.write().spawn((
        BuildingMetadata { id: db_id, pack_type },
        GridPosition { x: bx, y: by },
        BuildingStats {
            charge: extra.charge,
            max_charge: extra.max_charge,
            cost: extra.cost,
            hp: extra.hp,
            max_hp: extra.max_hp,
        },
        BuildingStorage {
            money: extra.money_inside,
            crystals: extra.crystals_inside,
            items: extra.items_inside.clone(),
        },
        BuildingOwnership {
            owner_id: pid,
            clan_id: initial_clan,
        },
        BuildingCrafting {
            recipe_id: extra.craft_recipe_id,
            num: extra.craft_num,
            end_ts: extra.craft_end_ts,
        },
        BuildingFlags { dirty: false },
    )).id();

    state.building_index.insert((bx, by), entity);
    
    let view = PackView {
        id: db_id,
        pack_type,
        x: bx,
        y: by,
        owner_id: pid,
        clan_id: initial_clan,
        charge: extra.charge,
        max_charge: extra.max_charge,
        hp: extra.hp,
        max_hp: extra.max_hp,
    };
    place_building_in_world(state, tx, pid, &view, true);
}

pub fn place_building_in_world(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId, view: &PackView, close_gui: bool) {
    place_building_cells(state, view.x, view.y, view.pack_type);
    broadcast_pack_to_nearby(state, view);
    if close_gui {
        let g = gu_close();
        send_u_packet(tx, g.0, &g.1);
    }
    tracing::info!("Player {pid} placed building {} at ({}, {})", view.pack_type.code(), view.x, view.y);
}

pub fn handle_remove_building(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId, bx: i32, by: i32) {
    let actor = state.query_player(pid, |ecs, entity| {
        let p = ecs.get::<PlayerPosition>(entity)?;
        let s = ecs.get::<PlayerStats>(entity)?;
        Some((p.x, p.y, s.clan_id.unwrap_or(0)))
    }).flatten();

    let Some(actor) = actor else { return; };
    let Some(view) = state.get_pack_at(bx, by) else {
        send_building_error(tx, "Объект не найден"); return;
    };

    if view.owner_id != pid && !(view.clan_id != 0 && view.clan_id == actor.2) {
        send_building_error(tx, "Нет прав"); return;
    }

    if !view.pack_type.building_cells().iter().any(|(dx, dy, _)| view.x + dx == actor.0 && view.y + dy == actor.1) {
        send_building_error(tx, "Вы не у объекта"); return;
    }

    if state.db.delete_building(view.id).is_err() {
        send_u_packet(tx, "OK", &ok_message("Ошибка", "Ошибка БД").1); return;
    }

    if let Some((_, entity)) = state.building_index.remove(&(view.x, view.y)) {
        state.ecs.write().despawn(entity);
    }
    
    clear_pack_cells(state, &view);
    broadcast_pack_clear(state, &view);
    close_pack_windows(state, &view);
    
    state.modify_player(pid, |ecs, entity| {
        if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) {
            let window = format!("pack:{}:{}", view.x, view.y);
            if ui.current_window.as_deref() == Some(window.as_str()) { ui.current_window = None; }
        }
        Some(())
    });
    let g = gu_close();
    send_u_packet(tx, g.0, &g.1);
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

pub fn broadcast_pack_to_nearby(state: &Arc<GameState>, view: &PackView) {
    broadcast_pack_update(state, view);
}

fn gather_block_packs(state: &Arc<GameState>, block_pos: i32) -> Vec<(u8, u16, u16, u8, u8)> {
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

pub fn broadcast_pack_update(state: &Arc<GameState>, view: &PackView) {
    if let Some(block_pos) = state.pack_block_pos(view.x, view.y) {
        let packs = gather_block_packs(state, block_pos);
        let sub = hb_packs(block_pos, &packs);
        let data = encode_hb_bundle(&hb_bundle(&[sub]).1);
        let (cx, cy) = World::chunk_pos(view.x, view.y);
        state.broadcast_to_nearby(cx, cy, &data, None);
    }
}

pub fn broadcast_pack_clear(state: &Arc<GameState>, view: &PackView) {
    broadcast_pack_update(state, view);
}

pub fn modify_pack_with_db<F, R>(state: &Arc<GameState>, pack_x: i32, pack_y: i32, f: F) -> Result<R, String>
where F: FnOnce(&mut EcsWorld, Entity) -> R
{
    let entity = *state.building_index.get(&(pack_x, pack_y)).ok_or_else(|| "Объект не найден".to_string())?;
    let mut ecs = state.ecs.write();
    let res = f(&mut ecs, entity);
    
    // Auto-save check or just mark dirty
    if let Some(mut flags) = ecs.get_mut::<BuildingFlags>(entity) { flags.dirty = true; }
    
    // In current sync mode we might want to save immediately if it's critical
    if let Some(row) = crate::game::buildings::extract_building_row(&ecs, entity) {
        let _ = state.db.save_building(&row); 
    }
    
    Ok(res)
}

fn place_pack_cells(state: &Arc<GameState>, view: &PackView) {
    for (dx, dy, cell) in view.pack_type.building_cells() {
        state.world.set_cell(view.x + dx, view.y + dy, cell);
        broadcast_cell_update(state, view.x + dx, view.y + dy);
    }
}

fn pack_has_cell(_state: &Arc<GameState>, bx: i32, by: i32, pack_type: PackType, cx: i32, cy: i32) -> bool {
    pack_type.building_cells().iter().any(|(dx, dy, _)| bx + dx == cx && by + dy == cy)
}

fn validate_pack_footprint(state: &Arc<GameState>, old_view: &PackView, new_x: i32, new_y: i32, new_type: PackType) -> Result<(), &'static str> {
    for (dx, dy, _) in new_type.building_cells() {
        let tx = new_x + dx; let ty = new_y + dy;
        if !state.world.valid_coord(tx, ty) { return Err("Нет места"); }
        if !state.world.is_empty(tx, ty) && !pack_has_cell(state, old_view.x, old_view.y, old_view.pack_type, tx, ty) { return Err("Нет места"); }
        if let Some((px, py)) = state.find_pack_covering(tx, ty) { if px != old_view.x || py != old_view.y { return Err("Место занято"); } }
    }
    Ok(())
}

fn send_building_error(tx: &mpsc::UnboundedSender<Vec<u8>>, text: &str) {
    send_u_packet(tx, "OK", &ok_message("Ошибка", text).1);
}

fn close_pack_windows(state: &Arc<GameState>, view: &PackView) {
    let window_key = format!("pack:{}:{}", view.x, view.y);
    let (pcx, pcy) = World::chunk_pos(view.x, view.y);
    for (cx, cy) in state.visible_chunks_around(pcx, pcy) {
        if let Some(players) = state.chunk_players.get(&(cx, cy)) {
            let ids: Vec<PlayerId> = players.value().clone();
            for pid in ids {
                state.modify_player(pid, |ecs, entity| {
                    if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) {
                        if ui.current_window.as_deref() == Some(window_key.as_str()) {
                            ui.current_window = None;
                            if let Some(conn) = ecs.get::<PlayerConnection>(entity) {
                                let g = gu_close();
                                let _ = conn.tx.send(make_u_packet_bytes(g.0, &g.1));
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

pub fn clear_pack_cells(state: &Arc<GameState>, view: &PackView) {
    for (cdx, cdy, _) in view.pack_type.building_cells() {
        state.world.set_cell(view.x + cdx, view.y + cdy, cell_type::EMPTY);
        broadcast_cell_update(state, view.x + cdx, view.y + cdy);
    }
}
