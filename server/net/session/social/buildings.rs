//! Меню построек и установка здания на карте.
use crate::net::session::play::dig_build::broadcast_cell_update;
use crate::net::session::prelude::*;
use std::collections::HashMap;

// ─── Buildings ─────────────────────────────────────────────────────────

fn pack_block_pos(state: &GameState, x: i32, y: i32) -> Option<i32> {
    if x < 0 || y < 0 {
        return None;
    }
    let chunk_x = x / 32;
    let chunk_y = y / 32;
    let width = i32::try_from(state.world.chunks_w()).ok()?;
    let height = i32::try_from(state.world.chunks_h()).ok()?;
    if chunk_x >= width || chunk_y >= height {
        return None;
    }
    chunk_y.checked_mul(width)?.checked_add(chunk_x)
}

pub fn handle_buildings_menu(
    _state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    _pid: PlayerId,
) {
    // Show building placement menu
    let mut buttons = vec![
        "Респ (5000$)",
        "bld_place:R",
        "Телепорт (3000$)",
        "bld_place:T",
        "Пушка (8000$)",
        "bld_place:G",
        "UP (4000$)",
        "bld_place:U",
        "Склад (4000$)",
        "bld_place:L",
        "Крафтер (5000$)",
        "bld_place:F",
        "Спот (3000$)",
        "bld_place:O",
        "Маркет (6000$)",
        "bld_place:M",
        "Ворота (2000$)",
        "bld_place:N",
    ];
    buttons.extend(CLOSE_WINDOW_BUTTON_LABELS.iter().copied());
    let gui = serde_json::json!({
        "title": "ПОСТРОЙКИ",
        "text": "Выберите здание для постройки",
        "buttons": buttons,
        "back": false
    });
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
}

pub fn handle_place_building(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    type_code: &str,
) {
    let Some(pack_type) = PackType::from_str(type_code) else {
        return;
    };

    let Some(cfg) = crate::game::get_building_config(pack_type) else {
        return;
    };
    let cost = cfg.cost;

    // Ворота имеют смысл только внутри клана — блокируют чужих, своих пускают.
    if pack_type == PackType::Gate {
        let player_clan = state
            .active_players
            .get(&pid)
            .map_or(0, |p| p.data.clan_id.unwrap_or(0));
        if player_clan == 0 {
            send_u_packet(tx, "OK", &ok_message("Ошибка", "Для ворот нужен клан").1);
            return;
        }
    }

    let (px, py, pdir) = {
        let Some(p) = state.active_players.get(&pid) else {
            return;
        };
        (p.data.x, p.data.y, p.data.dir)
    };

    let (dx, dy) = dir_offset(pdir);
    let (bx, by) = (px + dx * 3, py + dy * 3); // Place 3 cells in front

    if let Err(msg) = validate_building_area(state, bx, by, pack_type) {
        send_building_error(tx, msg);
        return;
    }

    let extra = building_extra_for_pack_type(pack_type);

    // Reserve money first so we can rollback on DB error.
    let (old_money, owner_clan) = {
        let Some(mut p) = state.active_players.get_mut(&pid) else {
            send_u_packet(tx, "OK", &ok_message("Ошибка", "Игрок не найден").1);
            return;
        };
        let old_money = p.data.money;
        if p.data.money < cost {
            send_u_packet(tx, "OK", &ok_message("Ошибка", "Недостаточно денег").1);
            return;
        }
        p.data.money -= cost;
        send_u_packet(tx, "P$", &money(p.data.money, p.data.creds).1);
        (old_money, p.data.clan_id.unwrap_or(0))
    };

    // Gate привязывается к клану строителя, остальные здания — нейтральные.
    let initial_clan = if pack_type == PackType::Gate {
        owner_clan
    } else {
        0
    };

    let db_id = match state
        .db
        .insert_building(type_code, bx, by, pid, initial_clan, &extra)
    {
        Ok(id) => id,
        Err(err) => {
            if let Some(mut p) = state.active_players.get_mut(&pid) {
                p.data.money = old_money;
                send_u_packet(tx, "P$", &money(p.data.money, p.data.creds).1);
            }
            send_u_packet(
                tx,
                "OK",
                &ok_message("Ошибка", "Не удалось сохранить здание").1,
            );
            tracing::error!("Failed to insert building: {err}");
            return;
        }
    };

    let mut pack = make_pack_data(state, db_id, pack_type, bx, by, pid, &extra);
    pack.clan_id = initial_clan;
    place_building_in_world(state, tx, pid, &pack, true);
}

pub fn place_building_in_world(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    pack: &PackData,
    close_gui: bool,
) {
    place_building_cells(state, pack.x, pack.y, pack.pack_type);
    if state.packs.insert((pack.x, pack.y), pack.clone()).is_some() {
        tracing::warn!(
            "Overwrote existing in-memory pack at ({}, {})",
            pack.x,
            pack.y
        );
    }
    broadcast_pack_to_nearby(state, pack);
    if close_gui {
        send_u_packet(tx, "Gu", &[]);
    }
    tracing::info!(
        "Player {pid} placed building {} at ({}, {})",
        pack.pack_type.code(),
        pack.x,
        pack.y
    );
}

pub fn handle_remove_building(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    bx: i32,
    by: i32,
) {
    let actor = {
        let Some(p) = state.active_players.get(&pid) else {
            send_building_error(tx, "Игрок не найден");
            return;
        };
        (p.data.x, p.data.y, p.data.clan_id.unwrap_or(0))
    };

    let Some(pack) = state.get_pack_at(bx, by) else {
        send_building_error(tx, "Объект не найден");
        return;
    };
    let pack = pack.clone();
    if pack.owner_id != pid && !(pack.clan_id != 0 && pack.clan_id == actor.2) {
        send_building_error(tx, "Нет прав на объект");
        return;
    }
    if !pack
        .pack_type
        .building_cells()
        .iter()
        .any(|(dx, dy, _)| pack.x + dx == actor.0 && pack.y + dy == actor.1)
    {
        send_building_error(tx, "Вы не у этого объекта");
        return;
    }

    if let Err(err) = state.db.delete_building(pack.id) {
        send_u_packet(
            tx,
            "OK",
            &ok_message("Ошибка", "Не удалось удалить здание").1,
        );
        tracing::error!(
            "Failed to delete building id={} for player {pid}: {err}",
            pack.id
        );
        return;
    }

    let removed = state.packs.remove(&(pack.x, pack.y)).is_some();
    clear_pack_cells(state, &pack);
    broadcast_pack_clear(state, &pack);
    close_pack_windows(state, &pack);
    if !removed {
        tracing::warn!(
            "Building id={} at ({}, {}) not found in memory during removal",
            pack.id,
            pack.x,
            pack.y
        );
    }
    if let Some(mut p) = state.active_players.get_mut(&pid) {
        let window = format!("pack:{}:{}", pack.x, pack.y);
        if p.current_window.as_deref() == Some(window.as_str()) {
            p.current_window = None;
        }
    }
    send_u_packet(tx, "Gu", &[]);
    tracing::info!(
        "Player {pid} removed building id={} at ({}, {})",
        pack.id,
        pack.x,
        pack.y
    );
}

#[allow(clippy::missing_const_for_fn)]
pub fn building_extra_for_pack_type(pack_type: PackType) -> BuildingExtra {
    let cfg = crate::game::get_building_config(pack_type);
    BuildingExtra {
        charge: cfg.map(|c| c.charge).unwrap_or(0.0),
        items_inside: HashMap::new(),
        max_charge: cfg.map(|c| c.max_charge).unwrap_or(0.0),
        cost: cfg.map(|c| c.cost as i32).unwrap_or(10),
        hp: cfg.map(|c| c.hp).unwrap_or(1000),
        max_hp: cfg.map(|c| c.max_hp).unwrap_or(1000),
        money_inside: 0,
        crystals_inside: [0; 6],
        craft_recipe_id: None,
        craft_num: 0,
        craft_end_ts: 0,
    }
}

pub fn validate_building_area(
    state: &Arc<GameState>,
    bx: i32,
    by: i32,
    pack_type: PackType,
) -> Result<(), &'static str> {
    let cells = pack_type.building_cells();
    for &(cdx, cdy, _) in &cells {
        let cx = bx + cdx;
        let cy = by + cdy;
        if !state.world.valid_coord(cx, cy) || !state.world.is_empty(cx, cy) {
            return Err("Недостаточно места");
        }
        if state.find_pack_covering(cx, cy).is_some() {
            return Err("Место занято зданием");
        }
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
    if chunk_x < 0 || chunk_y < 0 {
        return vec![];
    }
    let (cx, cy) = (chunk_x as u32, chunk_y as u32);
    let mut out = Vec::new();
    for (code, px, py, cid, off) in state.get_packs_in_chunk_area(cx, cy) {
        let Some(bp) = state.pack_block_pos(i32::from(px), i32::from(py)) else {
            continue;
        };
        if bp == block_pos {
            out.push((code, px, py, cid, off));
        }
    }
    out
}

pub fn broadcast_pack_update(state: &Arc<GameState>, pack: &PackData) {
    let Some(block_pos) = pack_block_pos(state, pack.x, pack.y) else {
        return;
    };
    // В клиенте `HB:O(block_pos, packs[])` трактуется как "полное состояние паков в block_pos".
    // Поэтому при любом апдейте отправляем ВСЕ паки в этом block_pos, иначе остальные пропадут.
    let packs = gather_block_packs(state, block_pos);
    let pack_sub = hb_packs(block_pos, &packs);
    let hb_data = encode_hb_bundle(&hb_bundle(&[pack_sub]).1);
    let (cx, cy) = World::chunk_pos(pack.x, pack.y);
    state.broadcast_to_nearby(cx, cy, &hb_data, None);
}

pub fn broadcast_pack_clear(state: &Arc<GameState>, pack: &PackData) {
    let Some(block_pos) = pack_block_pos(state, pack.x, pack.y) else {
        return;
    };
    // После удаления одного пака всё ещё могли остаться другие в том же block_pos.
    let packs = gather_block_packs(state, block_pos);
    let pack_sub = hb_packs(block_pos, &packs);
    let hb_data = encode_hb_bundle(&hb_bundle(&[pack_sub]).1);
    let (cx, cy) = World::chunk_pos(pack.x, pack.y);
    state.broadcast_to_nearby(cx, cy, &hb_data, None);
}

pub fn update_pack_with_db(
    state: &Arc<GameState>,
    pack_x: i32,
    pack_y: i32,
    mutate: impl FnOnce(&mut PackData),
) -> Result<PackData, String> {
    let Some(mut pack) = state.packs.get_mut(&(pack_x, pack_y)) else {
        return Err("Объект не найден".to_string());
    };
    let old_pack = pack.clone();
    mutate(&mut pack);
    let extra = building_extra_from_pack(&pack);
    let pack_id = pack.id;
    let updated_pack = pack.clone();
    drop(pack);

    if let Err(err) = state.db.update_building_extra(pack_id, &extra) {
        if let Some(mut current) = state.packs.get_mut(&(pack_x, pack_y)) {
            *current = old_pack;
        }
        return Err(err.to_string());
    }

    Ok(updated_pack)
}

/// Полная синхронизация БД, `packs` и клеток мира при смене позиции/типа/владельца.
/// Админ-команды чата `/pack ...` и GUI через [`update_pack_with_db`] для полей только в `BuildingExtra`.
pub fn update_pack_with_world_sync(
    state: &Arc<GameState>,
    pack_x: i32,
    pack_y: i32,
    mutate: impl FnOnce(&mut PackData),
) -> Result<PackData, String> {
    let old_pack = state
        .packs
        .get(&(pack_x, pack_y))
        .map(|p| p.clone())
        .ok_or_else(|| "Объект не найден".to_string())?;
    let old_key = (old_pack.x, old_pack.y);

    let mut updated_pack = old_pack.clone();
    mutate(&mut updated_pack);

    let footprint_changed = old_pack.pack_type != updated_pack.pack_type
        || old_pack.x != updated_pack.x
        || old_pack.y != updated_pack.y;
    let owner_changed = old_pack.owner_id != updated_pack.owner_id;
    let clan_changed = old_pack.clan_id != updated_pack.clan_id;
    let meta_changed = footprint_changed || owner_changed || clan_changed;
    if !meta_changed {
        return Err("Не требуется изменение метаданных".to_string());
    }

    if footprint_changed {
        if let Err(msg) = validate_pack_footprint(state, &old_pack, &updated_pack) {
            return Err(msg.to_string());
        }
    }

    let new_extra = building_extra_from_pack(&updated_pack);

    if let Err(err) = state.db.update_building_state(
        updated_pack.id,
        updated_pack.pack_type.code(),
        updated_pack.x,
        updated_pack.y,
        updated_pack.owner_id,
        updated_pack.clan_id,
        &new_extra,
    ) {
        return Err(err.to_string());
    }

    let new_key = (updated_pack.x, updated_pack.y);
    let position_changed = old_key != new_key;
    if new_key != old_key && state.packs.contains_key(&new_key) {
        let old_extra = building_extra_from_pack(&old_pack);
        let _ = state.db.update_building_state(
            old_pack.id,
            old_pack.pack_type.code(),
            old_pack.x,
            old_pack.y,
            old_pack.owner_id,
            old_pack.clan_id,
            &old_extra,
        );
        return Err("Место занято зданием".to_string());
    }

    let old_entry = state.packs.remove(&old_key);
    if old_entry.is_none() {
        let old_extra = building_extra_from_pack(&old_pack);
        let _ = state.db.update_building_state(
            old_pack.id,
            old_pack.pack_type.code(),
            old_pack.x,
            old_pack.y,
            old_pack.owner_id,
            old_pack.clan_id,
            &old_extra,
        );
        return Err("Объект не найден".to_string());
    }

    if state.packs.insert(new_key, updated_pack.clone()).is_some() {
        let old_extra = building_extra_from_pack(&old_pack);
        let _ = state.db.update_building_state(
            old_pack.id,
            old_pack.pack_type.code(),
            old_pack.x,
            old_pack.y,
            old_pack.owner_id,
            old_pack.clan_id,
            &old_extra,
        );
        let _ = state.packs.insert(old_key, old_pack);
        return Err("Не удалось обновить здание".to_string());
    }

    if footprint_changed {
        if position_changed || old_pack.pack_type != updated_pack.pack_type {
            broadcast_pack_clear(state, &old_pack);
        }
        clear_pack_cells(state, &old_pack);
        place_pack_cells(state, &updated_pack);
    }

    broadcast_pack_update(state, &updated_pack);
    Ok(updated_pack)
}

fn place_pack_cells(state: &Arc<GameState>, pack: &PackData) {
    for (dx, dy, cell) in pack.pack_type.building_cells() {
        state.world.set_cell(pack.x + dx, pack.y + dy, cell);
        broadcast_cell_update(state, pack.x + dx, pack.y + dy);
    }
}

fn pack_has_cell(pack: &PackData, cx: i32, cy: i32) -> bool {
    pack.pack_type
        .building_cells()
        .iter()
        .any(|(dx, dy, _)| pack.x + dx == cx && pack.y + dy == cy)
}

fn validate_pack_footprint(
    state: &Arc<GameState>,
    old_pack: &PackData,
    updated_pack: &PackData,
) -> Result<(), &'static str> {
    for (dx, dy, _) in updated_pack.pack_type.building_cells() {
        let tx = updated_pack.x + dx;
        let ty = updated_pack.y + dy;
        if !state.world.valid_coord(tx, ty) {
            return Err("Недостаточно места");
        }
        if !state.world.is_empty(tx, ty) && !pack_has_cell(old_pack, tx, ty) {
            return Err("Недостаточно места");
        }
        if let Some((px, py)) = state.find_pack_covering(tx, ty) {
            if px != old_pack.x || py != old_pack.y {
                return Err("Место занято зданием");
            }
        }
    }
    Ok(())
}

pub fn make_pack_data(
    state: &std::sync::Arc<crate::game::GameState>,
    db_id: i32,
    pack_type: PackType,
    x: i32,
    y: i32,
    pid: PlayerId,
    extra: &BuildingExtra,
) -> PackData {
    let ecs_entity = state
        .ecs
        .write()
        .spawn((
            crate::game::buildings::Position { x, y },
            crate::game::buildings::Building {
                id: db_id,
                type_code: pack_type.code(),
            },
            crate::game::buildings::Owner { pid, clan_id: 0 },
            crate::game::buildings::Health {
                state: extra.hp,
                max_state: extra.max_hp,
            },
        ))
        .id();

    PackData {
        id: db_id,
        ecs_entity,
        pack_type,
        x,
        y,
        owner_id: pid,
        clan_id: 0,
        charge: extra.charge,
        max_charge: extra.max_charge,
        cost: extra.cost,
        hp: extra.hp,
        max_hp: extra.max_hp,
        money_inside: extra.money_inside,
        crystals_inside: extra.crystals_inside,
        items_inside: extra.items_inside.clone(),
        craft_recipe_id: extra.craft_recipe_id,
        craft_num: extra.craft_num,
        craft_end_ts: extra.craft_end_ts,
    }
}

#[allow(clippy::missing_const_for_fn)]
pub fn building_extra_from_pack(pack: &PackData) -> BuildingExtra {
    BuildingExtra {
        charge: pack.charge,
        max_charge: pack.max_charge,
        cost: pack.cost,
        hp: pack.hp,
        max_hp: pack.max_hp,
        money_inside: pack.money_inside,
        crystals_inside: pack.crystals_inside,
        items_inside: pack.items_inside.clone(),
        craft_recipe_id: pack.craft_recipe_id,
        craft_num: pack.craft_num,
        craft_end_ts: pack.craft_end_ts,
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
                if let Some(mut p) = state.active_players.get_mut(&pid) {
                    if p.current_window.as_deref() == Some(window_key.as_str()) {
                        p.current_window = None;
                        send_u_packet(&p.tx, "Gu", &[]);
                    }
                }
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
        state
            .world
            .set_cell(pack.x + cdx, pack.y + cdy, cell_type::EMPTY);
        broadcast_cell_update(state, pack.x + cdx, pack.y + cdy);
    }
}
