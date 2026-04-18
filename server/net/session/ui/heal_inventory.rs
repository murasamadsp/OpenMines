//! Лечение и инвентарь.
use crate::net::session::outbound::inventory_sync::send_inventory;
use crate::net::session::play::dig_build::broadcast_cell_update;
use crate::net::session::prelude::*;
use crate::net::session::social::buildings::{
    building_extra_for_pack_type, make_pack_data, place_building_in_world, validate_building_area,
};

// ─── Healing ────────────────────────────────────────────────────────────────

pub fn handle_heal(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId) {
    let healed = {
        let Some(mut p) = state.active_players.get_mut(&pid) else {
            return;
        };
        if p.data.health >= p.data.max_health {
            send_u_packet(tx, "OK", &ok_message("Лечение", "Здоровье уже полное").1);
            return;
        }
        // Costs 1 red crystal, heals 10 HP
        if p.data.crystals[2] < 1 {
            send_u_packet(
                tx,
                "OK",
                &ok_message("Лечение", "Недостаточно красных кристаллов").1,
            );
            return;
        }
        p.data.crystals[2] -= 1;
        p.data.health = (p.data.health + 10).min(p.data.max_health);
        (p.data.health, p.data.max_health, p.data.crystals)
    };
    send_u_packet(tx, "@L", &health(healed.0, healed.1).1);
    send_u_packet(tx, "@B", &basket(&healed.2, 1000).1);

    // Send heal FX
    if let Some(p) = state.active_players.get(&pid) {
        let fx = hb_directed_fx(
            net_u16_nonneg(pid),
            net_u16_nonneg(p.data.x),
            net_u16_nonneg(p.data.y),
            5,
            0,
            0,
        );
        let fx_data = encode_hb_bundle(&hb_bundle(&[fx]).1);
        let (cx, cy) = World::chunk_pos(p.data.x, p.data.y);
        state.broadcast_to_nearby(cx, cy, &fx_data, None);
    }
}

// ─── Inventory ──────────────────────────────────────────────────────────────

pub fn handle_inventory_open(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    if let Some(p) = state.active_players.get(&pid) {
        send_inventory(tx, &p.data.inventory, p.inv_selected);
    }
}

pub fn handle_inventory_use(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let selected = {
        let Some(p) = state.active_players.get(&pid) else {
            return;
        };
        p.inv_selected
    };

    if selected < 0 {
        return;
    }

    // Check if player has this item
    let count = {
        let Some(p) = state.active_players.get(&pid) else {
            return;
        };
        *p.data.inventory.get(&selected).unwrap_or(&0)
    };
    if count <= 0 {
        return;
    }

    // Use item based on type (from C# Inventory.typeditems)
    let used = match selected {
        // Geopacks (10-16, 34, 42, 43, 46) — place a geo cell in front
        10..=16 | 34 | 42 | 43 | 46 => {
            use_geopack(state, tx, pid, u8::try_from(selected).unwrap_or(0))
        }
        // Building shpaks — place building for free (item is the cost)
        0 => place_building_from_item(state, tx, pid, "T"),
        1 => place_building_from_item(state, tx, pid, "R"),
        2 => place_building_from_item(state, tx, pid, "U"),
        3 => place_building_from_item(state, tx, pid, "M"),
        24 => place_building_from_item(state, tx, pid, "F"),
        26 => place_building_from_item(state, tx, pid, "G"),
        29 => place_building_from_item(state, tx, pid, "L"),
        // Consumables
        5 => use_boom(state, pid),
        6 => use_protector(state, pid),
        7 => use_razryadka(state, pid),
        35 => {
            // Polik — place polymer road (cell 39)
            use_geopack(state, tx, pid, 39)
        }
        40 => use_c190(state, pid),
        _ => {
            tracing::debug!("Item {selected} use not implemented yet");
            false
        }
    };

    if used {
        if let Some(mut p) = state.active_players.get_mut(&pid) {
            let entry = p.data.inventory.entry(selected).or_insert(0);
            *entry -= 1;
            if *entry <= 0 {
                p.data.inventory.remove(&selected);
            }
            send_inventory(tx, &p.data.inventory, p.inv_selected);
        }
    }
}

pub fn handle_inventory_choose(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    payload: &[u8],
) {
    let Ok(s) = std::str::from_utf8(payload) else {
        return;
    };
    let Ok(id) = s.parse::<i32>() else {
        return;
    };

    if let Some(mut p) = state.active_players.get_mut(&pid) {
        p.inv_selected = id;
        if id == -1 {
            send_u_packet(tx, "IN", &inventory_close().1);
        } else {
            send_inventory(tx, &p.data.inventory, id);
        }
    }
}

pub fn use_geopack(
    state: &Arc<GameState>,
    _tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    cell_to_place: u8,
) -> bool {
    let (target_x, target_y) = {
        let Some(p) = state.active_players.get(&pid) else {
            return false;
        };
        let (dx, dy) = dir_offset(p.data.dir);
        (p.data.x + dx, p.data.y + dy)
    };

    if !state.world.valid_coord(target_x, target_y) {
        return false;
    }

    let cell_defs = state.world.cell_defs();
    let prop = cell_defs.get(state.world.get_cell(target_x, target_y));
    if !prop.can_place_over() {
        return false;
    }

    state.world.set_cell(target_x, target_y, cell_to_place);
    broadcast_cell_update(state, target_x, target_y);
    true
}

/// Place a building without deducting money — the item consumption is the cost.
pub fn place_building_from_item(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    type_code: &str,
) -> bool {
    let Some(pack_type) = PackType::from_str(type_code) else {
        return false;
    };

    let (px, py, pdir) = {
        let Some(p) = state.active_players.get(&pid) else {
            return false;
        };
        (p.data.x, p.data.y, p.data.dir)
    };

    let (dx, dy) = dir_offset(pdir);
    let (bx, by) = (px + dx * 3, py + dy * 3);

    if let Err(msg) = validate_building_area(state, bx, by, pack_type) {
        send_u_packet(tx, "OK", &ok_message("Ошибка", msg).1);
        return false;
    }

    // Save to DB first to avoid ghost placement on DB failure
    let extra = building_extra_for_pack_type(pack_type);

    let db_id = match state.db.insert_building(type_code, bx, by, pid, 0, &extra) {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("Failed to insert building via item: {e}");
            send_u_packet(
                tx,
                "OK",
                &ok_message("Ошибка", "Не удалось разместить здание").1,
            );
            return false;
        }
    };

    let pack = make_pack_data(state, db_id, pack_type, bx, by, pid, &extra);
    place_building_in_world(state, tx, pid, &pack, false);

    true
}

/// Destroy cells in a 3x3 area in front of the player (Boom item).
/// Also damages buildings and players in range.
pub fn use_boom(state: &Arc<GameState>, pid: PlayerId) -> bool {
    let (px, py, pdir) = {
        let Some(p) = state.active_players.get(&pid) else {
            return false;
        };
        (p.data.x, p.data.y, p.data.dir)
    };

    let (dx, dy) = dir_offset(pdir);
    // Center of explosion: 1 cell in front
    let (cx, cy) = (px + dx, py + dy);

    // 1. Destroy cells in 3x3
    for ddx in -1..=1_i32 {
        for ddy in -1..=1_i32 {
            let target_x = cx + ddx;
            let target_y = cy + ddy;
            if state.world.valid_coord(target_x, target_y) {
                state.world.set_cell(target_x, target_y, cell_type::EMPTY);
                broadcast_cell_update(state, target_x, target_y);
            }
        }
    }

    // 2. Damage nearby players (radius 3)
    let mut hit_pids = Vec::new();
    let now = std::time::Instant::now();
    for entry in &state.active_players {
        let p = entry.value();
        if (p.data.x - cx).abs() <= 3 && (p.data.y - cy).abs() <= 3 {
            // Check protection
            if p.protection_until.is_some_and(|until| now < until) {
                continue;
            }
            hit_pids.push(p.data.id);
        }
    }
    for hit_pid in hit_pids {
        if let Some(mut p_mut) = state.active_players.get_mut(&hit_pid) {
            p_mut.data.health = (p_mut.data.health - 50).max(0);
            p_mut.dirty = true;
            send_u_packet(
                &p_mut.tx,
                "@L",
                &health(p_mut.data.health, p_mut.data.max_health).1,
            );
        }
    }

    // 3. Damage nearby buildings (radius 3)
    for mut entry in state.packs.iter_mut() {
        let (_, pack) = entry.pair_mut();
        if (pack.x - cx).abs() <= 3 && (pack.y - cy).abs() <= 3 {
            pack.hp = (pack.hp - 500).max(0);
        }
    }

    // 4. Send FX
    let fx = hb_fx(cx as u16, cy as u16, 0); // 0 = boom
    let hb_data = encode_hb_bundle(&hb_bundle(&[fx]).1);
    let (chx, chy) = World::chunk_pos(cx, cy);
    state.broadcast_to_nearby(chx, chy, &hb_data, None);

    tracing::info!("Player {pid} used Boom at center ({cx}, {cy})");
    true
}

pub fn use_protector(state: &Arc<GameState>, pid: PlayerId) -> bool {
    if let Some(mut p) = state.active_players.get_mut(&pid) {
        // 30 seconds protection
        p.protection_until = Some(std::time::Instant::now() + std::time::Duration::from_secs(30));

        // FX
        let fx = hb_directed_fx(
            net_u16_nonneg(pid),
            net_u16_nonneg(p.data.x),
            net_u16_nonneg(p.data.y),
            6,
            0,
            0,
        ); // 6 = prot fx
        let hb_data = encode_hb_bundle(&hb_bundle(&[fx]).1);
        let (cx, cy) = World::chunk_pos(p.data.x, p.data.y);
        state.broadcast_to_nearby(cx, cy, &hb_data, None);

        return true;
    }
    false
}

pub fn use_razryadka(state: &Arc<GameState>, pid: PlayerId) -> bool {
    let (px, py) = {
        let Some(p) = state.active_players.get(&pid) else {
            return false;
        };
        (p.data.x, p.data.y)
    };

    // Discharge all guns in radius 15
    for mut entry in state.packs.iter_mut() {
        let (_, pack) = entry.pair_mut();
        if pack.pack_type == PackType::Gun {
            if (pack.x - px).abs() <= 15 && (pack.y - py).abs() <= 15 {
                pack.charge = 0.0;
            }
        }
    }

    true
}

pub fn use_c190(state: &Arc<GameState>, pid: PlayerId) -> bool {
    let (px, py, pdir) = {
        let Some(p) = state.active_players.get(&pid) else {
            return false;
        };
        (p.data.x, p.data.y, p.data.dir)
    };

    let (dx, dy) = dir_offset(pdir);
    let now = std::time::Instant::now();

    // Shoot in a line (distance 10)
    for i in 1..=10 {
        let tx = px + dx * i;
        let ty = py + dy * i;

        if !state.world.valid_coord(tx, ty) {
            break;
        }
        if !state.world.is_empty(tx, ty) {
            // Hit a block!
            state.world.damage_cell(tx, ty, 10.0);
            broadcast_cell_update(state, tx, ty);
            break;
        }

        let mut target_pid = None;
        for entry in &state.active_players {
            let target = entry.value();
            if target.data.x == tx && target.data.y == ty {
                if let Some(until) = target.protection_until {
                    if now < until {
                        continue;
                    }
                }
                target_pid = Some(target.data.id);
                break;
            }
        }

        if let Some(t_pid) = target_pid {
            if let Some(mut p_mut) = state.active_players.get_mut(&t_pid) {
                p_mut.data.health = (p_mut.data.health - 20).max(0);
                p_mut.dirty = true;
                send_u_packet(
                    &p_mut.tx,
                    "@L",
                    &health(p_mut.data.health, p_mut.data.max_health).1,
                );
            }
            break;
        }
    }

    // Send shot FX
    let fx = hb_directed_fx(
        net_u16_nonneg(pid),
        net_u16_nonneg(px),
        net_u16_nonneg(py),
        1,
        pdir as u8,
        0,
    );
    let hb_data = encode_hb_bundle(&hb_bundle(&[fx]).1);
    let (cx, cy) = World::chunk_pos(px, py);
    state.broadcast_to_nearby(cx, cy, &hb_data, None);

    true
}
