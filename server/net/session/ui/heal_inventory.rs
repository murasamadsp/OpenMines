//! Лечение и инвентарь.
use crate::net::session::outbound::inventory_sync::send_inventory;
use crate::net::session::play::dig_build::broadcast_cell_update;
use crate::net::session::prelude::*;
use crate::net::session::social::buildings::{
    building_extra_for_pack_type, make_pack_data, place_building_in_world, validate_building_area,
};
use crate::game::player::{PlayerPosition, PlayerStats, PlayerCooldowns, PlayerConnection, PlayerInventory};

// ─── Healing ────────────────────────────────────────────────────────────────

pub fn handle_heal(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId) {
    let result = state.modify_player(pid, |ecs, entity| {
        let (h, mh, cry2, px, py) = {
            let stats = ecs.get::<PlayerStats>(entity)?;
            let pos = ecs.get::<PlayerPosition>(entity)?;
            (stats.health, stats.max_health, stats.crystals[2], pos.x, pos.y)
        };
        if h >= mh || cry2 < 1 { return None; }
        let mut stats_mut = ecs.get_mut::<PlayerStats>(entity)?;
        stats_mut.crystals[2] -= 1;
        stats_mut.health = (h + 10).min(mh);
        Some((stats_mut.health, mh, stats_mut.crystals, px, py))
    }).flatten();

    if let Some((h, mh, crys, px, py)) = result {
        send_u_packet(tx, "@L", &health(h, mh).1);
        send_u_packet(tx, "@B", &basket(&crys, 1000).1);
        let fx = hb_directed_fx(net_u16_nonneg(pid), net_u16_nonneg(px), net_u16_nonneg(py), 5, 0, 0);
        state.broadcast_to_nearby(World::chunk_pos(px, py).0, World::chunk_pos(px, py).1, &encode_hb_bundle(&hb_bundle(&[fx]).1), None);
    }
}

// ─── Inventory ──────────────────────────────────────────────────────────────

pub fn handle_inventory_open(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId) {
    state.query_player(pid, |ecs, entity| {
        if let Some(inv) = ecs.get::<PlayerInventory>(entity) { send_inventory(tx, inv); }
    });
}

pub fn handle_inventory_use(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId) {
    let (sel, count) = state.query_player(pid, |ecs, entity| {
        let inv = ecs.get::<PlayerInventory>(entity)?;
        Some((inv.selected, *inv.items.get(&inv.selected).unwrap_or(&0)))
    }).flatten().unwrap_or((-1, 0));

    if sel < 0 || count <= 0 { return; }

    let used = match sel {
        10..=16 | 34 | 42 | 43 | 46 => use_geopack(state, tx, pid, u8::try_from(sel).unwrap_or(0)),
        0 => place_building_from_item(state, tx, pid, "T"),
        1 => place_building_from_item(state, tx, pid, "R"),
        2 => place_building_from_item(state, tx, pid, "U"),
        3 => place_building_from_item(state, tx, pid, "M"),
        24 => place_building_from_item(state, tx, pid, "F"),
        26 => place_building_from_item(state, tx, pid, "G"),
        29 => place_building_from_item(state, tx, pid, "L"),
        5 => use_boom(state, pid),
        6 => use_protector(state, pid),
        7 => use_razryadka(state, pid),
        35 => use_geopack(state, tx, pid, 39),
        40 => use_c190(state, pid),
        _ => false,
    };

    if used {
        state.modify_player(pid, |ecs, entity| {
            let mut inv = ecs.get_mut::<PlayerInventory>(entity)?;
            let c = inv.items.entry(sel).or_insert(0);
            *c -= 1; if *c <= 0 { inv.items.remove(&sel); }
            send_inventory(tx, &inv);
            Some(())
        });
    }
}

pub fn handle_inventory_choose(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId, payload: &[u8]) {
    let Ok(s) = std::str::from_utf8(payload) else { return; };
    let Ok(id) = s.parse::<i32>() else { return; };
    state.modify_player(pid, |ecs, entity| {
        let mut inv = ecs.get_mut::<PlayerInventory>(entity)?;
        inv.selected = id;
        if id == -1 { send_u_packet(tx, "IN", &inventory_close().1); }
        else { send_inventory(tx, &inv); }
        Some(())
    });
}

pub fn use_geopack(state: &Arc<GameState>, _tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId, cell: u8) -> bool {
    let pos = state.query_player(pid, |ecs, entity| {
        let p = ecs.get::<PlayerPosition>(entity)?;
        Some((p.x, p.y, p.dir))
    }).flatten();
    let Some((px, py, pdir)) = pos else { return false; };
    let (dx, dy) = dir_offset(pdir);
    let (tx, ty) = (px + dx, py + dy);
    if !state.world.valid_coord(tx, ty) || !state.world.cell_defs().get(state.world.get_cell(tx, ty)).can_place_over() { return false; }
    state.world.set_cell(tx, ty, cell);
    broadcast_cell_update(state, tx, ty);
    true
}

pub fn place_building_from_item(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId, code: &str) -> bool {
    let Some(pack_type) = PackType::from_str(code) else { return false; };
    let pos = state.query_player(pid, |ecs, entity| {
        let p = ecs.get::<PlayerPosition>(entity)?;
        Some((p.x, p.y, p.dir))
    }).flatten();
    let Some((px, py, pdir)) = pos else { return false; };
    let (dx, dy) = dir_offset(pdir);
    let (bx, by) = (px + dx * 3, py + dy * 3);
    if validate_building_area(state, bx, by, pack_type).is_err() { return false; }
    let extra = building_extra_for_pack_type(pack_type);
    let id = state.db.insert_building(code, bx, by, pid, 0, &extra).ok();
    if let Some(db_id) = id {
        let pack = make_pack_data(state, db_id, pack_type, bx, by, pid, &extra);
        place_building_in_world(state, tx, pid, &pack, false);
        true
    } else { false }
}

pub fn use_boom(state: &Arc<GameState>, pid: PlayerId) -> bool {
    let pos = state.query_player(pid, |ecs, entity| {
        let p = ecs.get::<PlayerPosition>(entity)?;
        Some((p.x, p.y))
    }).flatten();
    let Some((px, py)) = pos else { return false; };
    let (cx, cy) = (px, py);
    for ddx in -1..=1 { for ddy in -1..=1 {
        let tx = cx + ddx; let ty = cy + ddy;
        if state.world.valid_coord(tx, ty) { state.world.set_cell(tx, ty, cell_type::EMPTY); broadcast_cell_update(state, tx, ty); }
    }}
    let now = std::time::Instant::now();
    for entry in &state.active_players {
        let opid = *entry.key();
        state.modify_player(opid, |ecs, entity| {
            let (px_o, py_o, h, mh, conn_tx) = {
                let p = ecs.get::<PlayerPosition>(entity)?;
                let s = ecs.get::<PlayerStats>(entity)?;
                let c = ecs.get::<PlayerConnection>(entity)?;
                (p.x, p.y, s.health, s.max_health, c.tx.clone())
            };
            if (px_o - cx).abs() <= 3 && (py_o - cy).abs() <= 3 {
                if let Some(cd) = ecs.get::<crate::game::player::PlayerCooldowns>(entity) {
                    if cd.protection_until.is_some_and(|u| now < u) { return Some(()); }
                }
                let mut s_mut = ecs.get_mut::<PlayerStats>(entity)?;
                s_mut.health = (h - 50).max(0);
                let _ = conn_tx.send(crate::net::session::wire::make_u_packet_bytes("@L", &health(s_mut.health, mh).1));
            }
            Some(())
        });
    }
    let fx = hb_fx(cx as u16, cy as u16, 0);
    state.broadcast_to_nearby(World::chunk_pos(cx, cy).0, World::chunk_pos(cx, cy).1, &encode_hb_bundle(&hb_bundle(&[fx]).1), None);
    true
}

pub fn use_protector(state: &Arc<GameState>, pid: PlayerId) -> bool {
    state.modify_player(pid, |ecs, entity| {
        let mut cd = ecs.get_mut::<PlayerCooldowns>(entity)?;
        cd.protection_until = Some(std::time::Instant::now() + std::time::Duration::from_secs(30));
        let pos = ecs.get::<PlayerPosition>(entity)?;
        let fx = hb_directed_fx(net_u16_nonneg(pid), net_u16_nonneg(pos.x), net_u16_nonneg(pos.y), 6, 0, 0);
        state.broadcast_to_nearby(World::chunk_pos(pos.x, pos.y).0, World::chunk_pos(pos.x, pos.y).1, &encode_hb_bundle(&hb_bundle(&[fx]).1), None);
        Some(true)
    }).flatten().unwrap_or(false)
}

pub fn use_razryadka(state: &Arc<GameState>, pid: PlayerId) -> bool {
    let pos = state.query_player(pid, |ecs, entity| { ecs.get::<PlayerPosition>(entity).map(|p| (p.x, p.y)) }).flatten();
    let Some((px, py)) = pos else { return false; };
    for mut entry in state.packs.iter_mut() {
        let p = entry.value_mut();
        if p.pack_type == PackType::Gun && (p.x - px).abs() <= 15 && (p.y - py).abs() <= 15 { p.charge = 0.0; }
    }
    true
}

pub fn use_c190(state: &Arc<GameState>, pid: PlayerId) -> bool {
    let data = state.query_player(pid, |ecs, entity| {
        let p = ecs.get::<PlayerPosition>(entity)?;
        Some((p.x, p.y, p.dir))
    }).flatten();
    let Some((px, py, pdir)) = data else { return false; };
    let (dx, dy) = dir_offset(pdir);
    let now = std::time::Instant::now();
    for i in 1..=10 {
        let (tx, ty) = (px + dx * i, py + dy * i);
        if !state.world.valid_coord(tx, ty) { break; }
        if !state.world.is_empty(tx, ty) { state.world.damage_cell(tx, ty, 10.0); broadcast_cell_update(state, tx, ty); break; }
        let mut hit = None;
        for entry in &state.active_players {
            let opid = *entry.key();
            let h = state.query_player(opid, |ecs, entity| {
                let p = ecs.get::<PlayerPosition>(entity)?;
                let is_hit = p.x == tx && p.y == ty;
                let protected = if let Some(cd) = ecs.get::<crate::game::player::PlayerCooldowns>(entity) {
                    cd.protection_until.is_some_and(|u| now < u)
                } else { false };
                if is_hit && !protected { Some(opid) } else { None }
            }).flatten();
            if h.is_some() { hit = h; break; }
        }
        if let Some(t_pid) = hit {
            state.modify_player(t_pid, |ecs, entity| {
                let (h_val, mh_val, conn_tx) = {
                    let s = ecs.get::<PlayerStats>(entity)?;
                    let c = ecs.get::<PlayerConnection>(entity)?;
                    (s.health, s.max_health, c.tx.clone())
                };
                let mut s_mut = ecs.get_mut::<PlayerStats>(entity)?;
                s_mut.health = (h_val - 20).max(0);
                let _ = conn_tx.send(crate::net::session::wire::make_u_packet_bytes("@L", &health(s_mut.health, mh_val).1));
                Some(())
            });
            break;
        }
    }
    let fx = hb_directed_fx(net_u16_nonneg(pid), net_u16_nonneg(px), net_u16_nonneg(py), 1, pdir as u8, 0);
    state.broadcast_to_nearby(World::chunk_pos(px, py).0, World::chunk_pos(px, py).1, &encode_hb_bundle(&hb_bundle(&[fx]).1), None);
    true
}
