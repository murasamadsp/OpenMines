//! Лечение и инвентарь.
#![allow(clippy::cast_possible_truncation)]
use crate::game::buildings::{
    BuildingCrafting, BuildingFlags, BuildingMetadata, BuildingOwnership, BuildingStats,
    BuildingStorage, GridPosition,
};
use crate::game::player::{
    PlayerConnection, PlayerCooldowns, PlayerInventory, PlayerPosition, PlayerSkills, PlayerStats,
};
use crate::net::session::outbound::inventory_sync::{add_choose_miniq, send_inventory};
use crate::net::session::play::dig_build::broadcast_cell_update;
use crate::net::session::prelude::*;
use crate::net::session::social::buildings::{
    building_extra_for_pack_type, place_building_in_world, validate_building_area,
};
use crate::net::session::play::death::handle_death;

// ─── Healing ────────────────────────────────────────────────────────────────

pub fn handle_heal(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId) {
    let result = state
        .modify_player(pid, |ecs, entity| {
            let (h, mh, cry2, _px, _py) = {
                let stats = ecs.get::<PlayerStats>(entity)?;
                let pos = ecs.get::<PlayerPosition>(entity)?;
                (
                    stats.health,
                    stats.max_health,
                    stats.crystals[2],
                    pos.x,
                    pos.y,
                )
            };
            let heal_amount = {
                let skills = ecs.get::<PlayerSkills>(entity)?;
                let v = get_player_skill_effect(&skills.states, SkillType::Repair);
                v as i32
            };
            if heal_amount <= 0 {
                return None;
            }
            if h >= mh || cry2 < 1 {
                return None;
            }
            let (new_health, new_crys, pid_val) = {
                let mut stats_mut = ecs.get_mut::<PlayerStats>(entity)?;
                stats_mut.crystals[2] -= 1;
                stats_mut.health = (h + heal_amount).min(mh);
                (stats_mut.health, stats_mut.crystals, _px)
            };
            let skill_payload = if let Some(mut skills_mut) = ecs.get_mut::<PlayerSkills>(entity) {
                add_skill_exp(&mut skills_mut.states, "e", 1.0);
                Some(skill_progress_payload(&skills_mut.states))
            } else {
                None
            };
            Some((new_health, mh, new_crys, pid_val, _py, skill_payload))
        })
        .flatten();

    if let Some((h, mh, crys, _px, _py, skill_payload)) = result {
        send_u_packet(tx, "@L", &health(h, mh).1);
        send_u_packet(tx, "@B", &basket(&crys, 1).1);
        // Always send @S after skill exp (C# Skill.AddExp always sends)
        if let Some(sp) = skill_payload {
            let sk = skills_packet(&sp);
            send_u_packet(tx, sk.0, &sk.1);
        }
        // D20: C# sends coordinates (0, 0) for heal FX
        let fx = hb_directed_fx(net_u16_nonneg(pid), 0, 0, 5, 0, 0);
        state.broadcast_to_nearby(
            World::chunk_pos(_px, _py).0,
            World::chunk_pos(_px, _py).1,
            &encode_hb_bundle(&hb_bundle(&[fx]).1),
            None,
        );
    }
}

// ─── Inventory ──────────────────────────────────────────────────────────────

/// `Session.Invn`: переключить `minv` и отправить инвентарь.
pub fn handle_invn_toggle(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    state.modify_player(pid, |ecs, entity| {
        let mut inv = ecs.get_mut::<PlayerInventory>(entity)?;
        inv.minv = !inv.minv;
        send_inventory(tx, &mut inv);
        Some(())
    });
}

/// Helper: check if an item is exempt from facing-cell placement checks.
/// C# `Inventory.Use:208`: `selected is 40 or (>=10 and <17) or 34 or 42 or 43 or 46`
fn is_exempt_item(sel: i32) -> bool {
    sel == 40 || (10..17).contains(&sel) || sel == 34 || sel == 42 || sel == 43 || sel == 46
}

pub fn handle_inventory_use(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let (sel, count) = state
        .query_player(pid, |ecs, entity| {
            let inv = ecs.get::<PlayerInventory>(entity)?;
            Some((inv.selected, *inv.items.get(&inv.selected).unwrap_or(&0)))
        })
        .flatten()
        .unwrap_or((-1, 0));

    if sel < 0 || count <= 0 {
        return;
    }

    // D27+D28: C# checks before using item:
    // 1. No building at facing cell (unless exempt item)
    // 2. Facing cell must allow placement (unless exempt item)
    if !is_exempt_item(sel) {
        let check = state
            .query_player(pid, |ecs, entity| {
                let p = ecs.get::<PlayerPosition>(entity)?;
                Some((p.x, p.y, p.dir))
            })
            .flatten();
        if let Some((px, py, pdir)) = check {
            let (dx, dy) = dir_offset(pdir);
            let (fx, fy) = (px + dx, py + dy);
            if state.world.valid_coord(fx, fy) {
                // Check: no building at facing cell
                if state.building_index.contains_key(&(fx, fy)) {
                    return;
                }
                // Check: facing cell must allow placement
                let cell = state.world.get_cell(fx, fy);
                if !state.world.cell_defs().get(cell).can_place_over() {
                    return;
                }
            }
        }
    }

    let used = match sel {
        10..=16 | 34 | 42 | 43 | 46 => use_geopack(state, tx, pid, sel),
        0 => place_building_from_item(state, tx, pid, "T"),
        1 => place_building_from_item(state, tx, pid, "R"),
        2 => place_building_from_item(state, tx, pid, "U"),
        3 => place_building_from_item(state, tx, pid, "M"),
        4 => true,
        24 => place_building_from_item(state, tx, pid, "F"),
        26 => place_building_from_item(state, tx, pid, "G"),
        27 => place_building_from_item(state, tx, pid, "N"),
        29 => place_building_from_item(state, tx, pid, "L"),
        5 => use_boom(state, pid),
        6 => use_protector(state, pid),
        7 => use_razryadka(state, pid),
        35 => use_poli(state, pid),
        40 => use_c190(state, pid),
        _ => false,
    };

    if used {
        state.modify_player(pid, |ecs, entity| {
            let mut inv = ecs.get_mut::<PlayerInventory>(entity)?;
            let c = inv.items.entry(sel).or_insert(0);
            *c -= 1;
            if *c <= 0 {
                inv.items.remove(&sel);
                inv.miniq.retain(|&x| x != sel);
            }
            send_inventory(tx, &mut inv);
            Some(())
        });
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
    state.modify_player(pid, |ecs, entity| {
        let mut inv = ecs.get_mut::<PlayerInventory>(entity)?;
        if id != -1 {
            add_choose_miniq(&mut inv.miniq, id);
        }
        inv.selected = id;
        // C# ref: selection == -1 → Choose(-1) without SendInventory()
        if id != -1 {
            send_inventory(tx, &mut inv);
        }
        Some(())
    });
}

/// Item-to-alive-cell mapping for geopack items.
/// Returns `None` for item 10 (no placement), `Some(cell_type)` for all others.
fn geopack_item_to_cell(item: i32) -> Option<u8> {
    match item {
        10 => None,
        11 => Some(cell_type::ALIVE_CYAN),
        12 => Some(cell_type::ALIVE_RED),
        13 => Some(cell_type::ALIVE_VIOL),
        14 => Some(cell_type::ALIVE_BLACK),
        15 => Some(cell_type::ALIVE_WHITE),
        16 => Some(cell_type::ALIVE_BLUE),
        34 => Some(cell_type::HYPNO_ROCK),
        42 => Some(cell_type::BLACK_ROCK),
        43 => Some(cell_type::RED_ROCK),
        46 => Some(cell_type::ALIVE_RAINBOW),
        _ => None,
    }
}

/// D26: Reverse mapping — alive cell to item ID for geopack pickup.
/// C# `World.isAlive` only includes the 7 alive types (not HypnoRock/BlackRock/RedRock).
fn alive_cell_to_item(cell: u8) -> Option<i32> {
    match cell {
        cell_type::ALIVE_CYAN => Some(11),
        cell_type::ALIVE_RED => Some(12),
        cell_type::ALIVE_VIOL => Some(13),
        cell_type::ALIVE_BLACK => Some(14),
        cell_type::ALIVE_WHITE => Some(15),
        cell_type::ALIVE_BLUE => Some(16),
        cell_type::ALIVE_RAINBOW => Some(46),
        cell_type::HYPNO_ROCK => Some(34),
        cell_type::BLACK_ROCK => Some(42),
        cell_type::RED_ROCK => Some(43),
        _ => None,
    }
}

/// D25+D26: Geopack — placement on truly empty cells, pickup of any alive cell.
pub fn use_geopack(
    state: &Arc<GameState>,
    _tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    item_id: i32,
) -> bool {
    let pos = state
        .query_player(pid, |ecs, entity| {
            let p = ecs.get::<PlayerPosition>(entity)?;
            Some((p.x, p.y, p.dir))
        })
        .flatten();
    let Some((px, py, pdir)) = pos else {
        return false;
    };
    let (dx, dy) = dir_offset(pdir);
    let (fx, fy) = (px + dx, py + dy);
    if !state.world.valid_coord(fx, fy) {
        return false;
    }

    let facing_cell = state.world.get_cell(fx, fy);

    // D26: pickup — if facing cell is ANY alive cell, pick it up and map to correct item.
    if let Some(pickup_item) = alive_cell_to_item(facing_cell) {
        state.world.destroy(fx, fy);
        broadcast_cell_update(state, fx, fy);
        // Add the mapped item to inventory (don't consume the used item).
        state.modify_player(pid, |ecs, entity| {
            let mut inv = ecs.get_mut::<PlayerInventory>(entity)?;
            let c = inv.items.entry(pickup_item).or_insert(0);
            *c += 1;
            Some(())
        });
        return false; // don't consume the item used
    }

    // D25: placement — target cell must be truly empty (NOTHING or EMPTY), not can_place_over.
    let is_truly_empty = facing_cell == cell_type::NOTHING || facing_cell == cell_type::EMPTY;
    if !is_truly_empty {
        return false;
    }

    // Item 10 has no placement.
    let Some(alive_cell) = geopack_item_to_cell(item_id) else {
        return false;
    };

    state.world.set_cell(fx, fy, alive_cell);
    broadcast_cell_update(state, fx, fy);
    true
}

/// C# `ShitClass.Poli()` — place `POLYMER_ROAD` at the facing cell.
pub fn use_poli(state: &Arc<GameState>, pid: PlayerId) -> bool {
    let pos = state
        .query_player(pid, |ecs, entity| {
            let p = ecs.get::<PlayerPosition>(entity)?;
            let s = ecs.get::<PlayerStats>(entity)?;
            Some((p.x, p.y, p.dir, s.clan_id.unwrap_or(0)))
        })
        .flatten();
    let Some((px, py, pdir, cid)) = pos else {
        return false;
    };
    let (dx, dy) = dir_offset(pdir);
    let (fx, fy) = (px + dx, py + dy);
    if !state.world.valid_coord(fx, fy) {
        return false;
    }
    if !state.access_gun(fx, fy, cid) {
        return false;
    }
    let c = state.world.get_cell(fx, fy);
    let is_truly_empty = c == cell_type::NOTHING || c == cell_type::EMPTY;
    if !is_truly_empty {
        return false;
    }
    state.world.set_cell(fx, fy, cell_type::POLYMER_ROAD);
    broadcast_cell_update(state, fx, fy);
    false
}

pub fn place_building_from_item(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    code: &str,
) -> bool {
    let Some(pack_type) = PackType::from_str(code) else {
        return false;
    };
    let pos = state
        .query_player(pid, |ecs: &bevy_ecs::prelude::World, entity| {
            let p = ecs.get::<PlayerPosition>(entity)?;
            Some((p.x, p.y, p.dir))
        })
        .flatten();
    let Some((px, py, pdir)) = pos else {
        return false;
    };
    let (dx, dy) = dir_offset(pdir);
    let (bx, by) = (px + dx * 3, py + dy * 3);
    if validate_building_area(state, bx, by, pack_type).is_err() {
        return false;
    }
    let extra = building_extra_for_pack_type(pack_type);
    let id = state.db.insert_building(code, bx, by, pid, 0, &extra).ok();
    if let Some(db_id) = id {
        let entity = state
            .ecs
            .write()
            .spawn((
                BuildingMetadata {
                    id: db_id,
                    pack_type,
                },
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
                    clan_id: 0,
                },
                BuildingCrafting {
                    recipe_id: extra.craft_recipe_id,
                    num: extra.craft_num,
                    end_ts: extra.craft_end_ts,
                },
                BuildingFlags { dirty: false },
            ))
            .id();
        state.building_index.insert((bx, by), entity);
        let view = PackView {
            id: db_id,
            pack_type,
            x: bx,
            y: by,
            owner_id: pid,
            clan_id: 0,
            charge: extra.charge,
            max_charge: extra.max_charge,
            hp: extra.hp,
            max_hp: extra.max_hp,
        };
        place_building_in_world(state, tx, pid, &view, false);
        true
    } else {
        false
    }
}

/// Helper: check if a cell is a building block type (C# `World.isBuildingBlock`).
fn is_building_block(cell: u8) -> bool {
    matches!(
        cell,
        cell_type::GREEN_BLOCK
            | cell_type::YELLOW_BLOCK
            | cell_type::RED_BLOCK
            | cell_type::MILITARY_BLOCK_FRAME
            | cell_type::MILITARY_BLOCK
            | cell_type::SUPPORT
            | cell_type::QUAD_BLOCK
    )
}

/// Helper: check if a cell is alive (C# `World.isAlive`).
fn is_alive_cell(cell: u8) -> bool {
    matches!(
        cell,
        cell_type::ALIVE_BLUE
            | cell_type::ALIVE_CYAN
            | cell_type::ALIVE_RED
            | cell_type::ALIVE_BLACK
            | cell_type::ALIVE_VIOL
            | cell_type::ALIVE_WHITE
            | cell_type::ALIVE_RAINBOW
    )
}

/// Helper: deal damage to nearby players, returning list of killed players.
/// `center`: (cx, cy), `radius`: max Vector2.Distance, `damage`: HP to deal.
fn aoe_damage_players(
    state: &Arc<GameState>,
    cx: i32,
    cy: i32,
    scan_range: i32,
    radius: f32,
    damage: i32,
) -> Vec<(PlayerId, mpsc::UnboundedSender<Vec<u8>>)> {
    let now = std::time::Instant::now();
    let mut killed: Vec<(PlayerId, mpsc::UnboundedSender<Vec<u8>>)> = Vec::new();
    for entry in &state.active_players {
        let opid = *entry.key();
        // Returns Some((survived, px, py)) if player was in range and damaged
        let hit_result = state
            .modify_player(opid, |ecs: &mut bevy_ecs::prelude::World, entity| {
                let (px_o, py_o, h, mh, conn_tx) = {
                    let p = ecs.get::<PlayerPosition>(entity)?;
                    let s = ecs.get::<PlayerStats>(entity)?;
                    let c = ecs.get::<PlayerConnection>(entity)?;
                    (p.x, p.y, s.health, s.max_health, c.tx.clone())
                };
                if (px_o - cx).abs() > scan_range || (py_o - cy).abs() > scan_range {
                    return Some(None);
                }
                let dist = (((px_o - cx) as f32).powi(2) + ((py_o - cy) as f32).powi(2)).sqrt();
                if dist > radius {
                    return Some(None);
                }
                if let Some(cd) = ecs.get::<PlayerCooldowns>(entity) {
                    if cd.protection_until.is_some_and(|u| now < u) {
                        return Some(None);
                    }
                }
                // Health skill exp on every hurt (C# Player.Hurt → SkillType.Health)
                if let Some(mut skills) = ecs.get_mut::<PlayerSkills>(entity) {
                    crate::game::skills::add_skill_exp(&mut skills.states, "l", 1.0);
                    // Always send @S after skill exp (C# Skill.AddExp always sends)
                    let sk = skills_packet(&skill_progress_payload(&skills.states));
                    let _ =
                        conn_tx.send(crate::net::session::wire::make_u_packet_bytes(sk.0, &sk.1));
                }
                let mut s_mut = ecs.get_mut::<PlayerStats>(entity)?;
                if h > damage {
                    s_mut.health = h - damage;
                } else {
                    s_mut.health = 0;
                }
                let survived = s_mut.health > 0;
                let _ = conn_tx.send(crate::net::session::wire::make_u_packet_bytes(
                    "@L",
                    &health(s_mut.health, mh).1,
                ));
                Some(Some((survived, px_o, py_o)))
            })
            .flatten();
        // Broadcast hurt FX for surviving players (C# SendDFToBots(6,0,0,id,0))
        if let Some(Some((true, px_o, py_o))) = hit_result {
            let fx = hb_directed_fx(net_u16_nonneg(opid), 0, 0, 6, 0, 0);
            let (chunk_x, chunk_y) = World::chunk_pos(px_o, py_o);
            state.broadcast_to_nearby(
                chunk_x,
                chunk_y,
                &encode_hb_bundle(&hb_bundle(&[fx]).1),
                None,
            );
        }
        let dead = state
            .query_player(opid, |ecs, entity| {
                let s = ecs.get::<PlayerStats>(entity)?;
                let c = ecs.get::<PlayerConnection>(entity)?;
                (s.health <= 0).then(|| c.tx.clone())
            })
            .flatten();
        if let Some(tx) = dead {
            killed.push((opid, tx));
        }
    }
    killed
}

/// D14: Boom — C# `ShitClass.Boom` parity.
pub fn use_boom(state: &Arc<GameState>, pid: PlayerId) -> bool {
    let pos = state
        .query_player(pid, |ecs: &bevy_ecs::prelude::World, entity| {
            let p = ecs.get::<PlayerPosition>(entity)?;
            let s = ecs.get::<PlayerStats>(entity)?;
            Some((p.x, p.y, p.dir, s.clan_id.unwrap_or(0)))
        })
        .flatten();
    let Some((px, py, pdir, cid)) = pos else {
        return false;
    };
    let (dx, dy) = dir_offset(pdir);
    let (cx, cy) = (px + dx, py + dy);
    if !state.world.valid_coord(cx, cy) {
        return false;
    }

    // AccessGun check on facing cell
    if !state.access_gun(cx, cy, cid) {
        return false;
    }

    // AoE: radius 3.5 centered on facing cell
    for ddx in -4..=4 {
        for ddy in -4..=4 {
            let tx_c = cx + ddx;
            let ty_c = cy + ddy;
            if !state.world.valid_coord(tx_c, ty_c) {
                continue;
            }
            let dist = ((ddx as f32).powi(2) + (ddy as f32).powi(2)).sqrt();
            if dist > 3.5 {
                continue;
            }
            let c = state.world.get_cell(tx_c, ty_c);
            let defs = state.world.cell_defs();
            let prop = defs.get(c);
            if prop.physical.is_destructible && !state.building_index.contains_key(&(tx_c, ty_c)) {
                // Special cell conversions
                if c == cell_type::RED_ROCK && rand::random::<u32>() % 100 >= 98 {
                    // 2% chance: 117 → 118
                    state.world.set_cell(tx_c, ty_c, cell_type::ACID_ROCK);
                } else if c == cell_type::ACID_ROCK {
                    // 118 → 103
                    state.world.set_cell(tx_c, ty_c, cell_type::ROCK);
                } else if c != cell_type::RED_ROCK && c != cell_type::ACID_ROCK {
                    state.world.destroy(tx_c, ty_c);
                }
                broadcast_cell_update(state, tx_c, ty_c);
            }
        }
    }

    // Damage: 40 HP to players in radius
    let killed = aoe_damage_players(state, cx, cy, 4, 3.5, 40);
    for (opid, tx) in killed {
        handle_death(state, &tx, opid);
    }

    let fx = hb_directed_fx(
        net_u16_nonneg(pid),
        net_u16_nonneg(cx),
        net_u16_nonneg(cy),
        1,
        3,
        0,
    );
    state.broadcast_to_nearby(
        World::chunk_pos(cx, cy).0,
        World::chunk_pos(cx, cy).1,
        &encode_hb_bundle(&hb_bundle(&[fx]).1),
        None,
    );
    true
}

/// D15: Protector (item 6) — C# `ShitClass.Prot` — AoE bomb, NOT a shield.
pub fn use_protector(state: &Arc<GameState>, pid: PlayerId) -> bool {
    let pos = state
        .query_player(pid, |ecs: &bevy_ecs::prelude::World, entity| {
            let p = ecs.get::<PlayerPosition>(entity)?;
            let s = ecs.get::<PlayerStats>(entity)?;
            Some((p.x, p.y, p.dir, s.clan_id.unwrap_or(0)))
        })
        .flatten();
    let Some((px, py, pdir, cid)) = pos else {
        return false;
    };
    let (dx, dy) = dir_offset(pdir);
    let (cx, cy) = (px + dx, py + dy);
    if !state.world.valid_coord(cx, cy) {
        return false;
    }

    // AccessGun check on facing cell
    if !state.access_gun(cx, cy, cid) {
        return false;
    }

    // C# iterates -1..=1 with distance check <= 3.5 (always true in that range)
    for ddx in -1..=1 {
        for ddy in -1..=1 {
            let tx_c = cx + ddx;
            let ty_c = cy + ddy;
            if !state.world.valid_coord(tx_c, ty_c) {
                continue;
            }
            // Destroy gates in range
            // (Gate buildings are in building_index; check and remove if it's a Gate)
            if let Some(bld_entity) = state.building_index.get(&(tx_c, ty_c)) {
                let is_gate = state
                    .ecs
                    .read()
                    .get::<BuildingMetadata>(*bld_entity)
                    .is_some_and(|m| m.pack_type == PackType::Gate);
                if is_gate {
                    // Remove gate
                    state.building_index.remove(&(tx_c, ty_c));
                    let ent = *bld_entity;
                    drop(bld_entity);
                    state.ecs.write().despawn(ent);
                    // Clear the gate cell
                    state.world.set_cell(tx_c, ty_c, cell_type::EMPTY);
                    broadcast_cell_update(state, tx_c, ty_c);
                }
            }
            // Destroy destructible non-building cells
            let c = state.world.get_cell(tx_c, ty_c);
            let defs = state.world.cell_defs();
            let prop = defs.get(c);
            if prop.physical.is_destructible && !state.building_index.contains_key(&(tx_c, ty_c)) {
                state.world.destroy(tx_c, ty_c);
                broadcast_cell_update(state, tx_c, ty_c);
            }
        }
    }

    // 50 HP damage to players in range
    let killed = aoe_damage_players(state, cx, cy, 1, 3.5, 50);
    for (opid, tx) in killed {
        handle_death(state, &tx, opid);
    }

    let fx = hb_directed_fx(
        net_u16_nonneg(pid),
        net_u16_nonneg(cx),
        net_u16_nonneg(cy),
        1,
        0,
        1,
    );
    state.broadcast_to_nearby(
        World::chunk_pos(cx, cy).0,
        World::chunk_pos(cx, cy).1,
        &encode_hb_bundle(&hb_bundle(&[fx]).1),
        None,
    );
    true
}

/// D16: Razryadka — C# `ShitClass.Raz` parity.
pub fn use_razryadka(state: &Arc<GameState>, pid: PlayerId) -> bool {
    let pos = state
        .query_player(pid, |ecs: &bevy_ecs::prelude::World, entity| {
            let p = ecs.get::<PlayerPosition>(entity)?;
            Some((p.x, p.y, p.dir))
        })
        .flatten();
    let Some((px, py, pdir)) = pos else {
        return false;
    };
    let (dx, dy) = dir_offset(pdir);
    let (cx, cy) = (px + dx, py + dy);

    // Damage buildings (IDamagable) and zero gun charge in radius 9.5
    {
        let mut ecs = state.ecs.write();
        let mut query = ecs.query::<(&BuildingMetadata, &GridPosition, &mut BuildingStats)>();
        for (_meta, bpos, mut stats) in query.iter_mut(&mut ecs) {
            let ddx = (bpos.x - cx) as f32;
            let ddy = (bpos.y - cy) as f32;
            let dist = (ddx * ddx + ddy * ddy).sqrt();
            if dist <= 9.5 {
                // Zero charge
                stats.charge = 0.0;
                // Damage building 10 HP
                stats.hp -= 10;
            }
        }
    }

    // 500 HP damage to ALL players in radius 9.5
    let killed = aoe_damage_players(state, cx, cy, 10, 9.5, 500);
    for (opid, tx) in killed {
        handle_death(state, &tx, opid);
    }

    let fx = hb_directed_fx(
        net_u16_nonneg(pid),
        net_u16_nonneg(cx),
        net_u16_nonneg(cy),
        1,
        0,
        2,
    );
    state.broadcast_to_nearby(
        World::chunk_pos(cx, cy).0,
        World::chunk_pos(cx, cy).1,
        &encode_hb_bundle(&hb_bundle(&[fx]).1),
        None,
    );
    true
}

/// D17: C190 — C# `ShitClass.C190Shot` parity.
pub fn use_c190(state: &Arc<GameState>, pid: PlayerId) -> bool {
    let data = state
        .query_player(pid, |ecs: &bevy_ecs::prelude::World, entity| {
            let p = ecs.get::<PlayerPosition>(entity)?;
            Some((p.x, p.y, p.dir))
        })
        .flatten();
    let Some((px, py, pdir)) = data else {
        return false;
    };
    let (dx, dy) = dir_offset(pdir);
    // Starting from facing cell, 10 cells total
    let start_x = px + dx;
    let start_y = py + dy;
    // Endpoint for FX (9 more cells past facing = facing + 9*dir)
    let end_x = start_x + dx * 9;
    let end_y = start_y + dy * 9;
    let now = std::time::Instant::now();

    // Iterate all 10 cells — don't stop early
    for i in 0..10 {
        let (tx_c, ty_c) = (start_x + dx * i, start_y + dy * i);
        if !state.world.valid_coord(tx_c, ty_c) {
            break;
        }

        // Damage valid cells: not alive, is_diggable, is_destructible, not building block
        let c = state.world.get_cell(tx_c, ty_c);
        if !is_alive_cell(c) && !is_building_block(c) {
            let defs = state.world.cell_defs();
            let prop = defs.get(c);
            if prop.physical.is_diggable && prop.physical.is_destructible {
                state.world.damage_cell(tx_c, ty_c, 50.0);
                broadcast_cell_update(state, tx_c, ty_c);
            }
        }

        // Damage ALL players on this cell: 20 + 60 * c190stacks, then increment
        for entry in &state.active_players {
            let opid = *entry.key();
            let death_tx = state
                .modify_player(opid, |ecs: &mut bevy_ecs::prelude::World, entity| {
                    let (px_o, py_o) = {
                        let p = ecs.get::<PlayerPosition>(entity)?;
                        (p.x, p.y)
                    };
                    if px_o != tx_c || py_o != ty_c {
                        return None;
                    }
                    // Check protection
                    if let Some(cd) = ecs.get::<PlayerCooldowns>(entity) {
                        if cd.protection_until.is_some_and(|u| now < u) {
                            return None;
                        }
                    }
                    // Health skill exp (C# Player.Hurt → AddExp("l"))
                    let skill_pkt = if let Some(mut skills) = ecs.get_mut::<PlayerSkills>(entity) {
                        crate::game::skills::add_skill_exp(&mut skills.states, "l", 1.0);
                        let sk = skills_packet(&skill_progress_payload(&skills.states));
                        Some(crate::net::session::wire::make_u_packet_bytes(sk.0, &sk.1))
                    } else {
                        None
                    };
                    if let Some(pkt) = skill_pkt {
                        if let Some(c) = ecs.get::<PlayerConnection>(entity) {
                            let _ = c.tx.send(pkt);
                        }
                    }
                    // Get stacks and compute damage
                    let stacks = ecs
                        .get::<PlayerCooldowns>(entity)
                        .map_or(0, |cd| cd.c190_stacks);
                    let dmg = 20 + 60 * stacks;
                    // Apply damage
                    let (h, mh, conn_tx) = {
                        let s = ecs.get::<PlayerStats>(entity)?;
                        let c = ecs.get::<PlayerConnection>(entity)?;
                        (s.health, s.max_health, c.tx.clone())
                    };
                    let new_health = {
                        let mut s_mut = ecs.get_mut::<PlayerStats>(entity)?;
                        s_mut.health = if h > dmg { h - dmg } else { 0 };
                        s_mut.health
                    };
                    let _ = conn_tx.send(crate::net::session::wire::make_u_packet_bytes(
                        "@L",
                        &health(new_health, mh).1,
                    ));
                    // Increment stacks
                    if let Some(mut cd) = ecs.get_mut::<PlayerCooldowns>(entity) {
                        cd.c190_stacks += 1;
                        cd.last_c190_hit = Some(now);
                    }
                    if new_health <= 0 {
                        Some((conn_tx, true)) // died
                    } else {
                        Some((conn_tx, false)) // survived
                    }
                })
                .flatten();
            if let Some((tx, died)) = death_tx {
                if died {
                    handle_death(state, &tx, opid);
                } else {
                    // Hurt FX for survivor: SendDFToBots(6, 0, 0, id, 0)
                    let fx = hb_directed_fx(
                        net_u16_nonneg(opid),
                        net_u16_nonneg(tx_c),
                        net_u16_nonneg(ty_c),
                        6,
                        0,
                        0,
                    );
                    let (cx, cy) = World::chunk_pos(tx_c, ty_c);
                    state.broadcast_to_nearby(cx, cy, &encode_hb_bundle(&hb_bundle(&[fx]).1), None);
                }
            }
        }
    }

    // FX: type 7, at endpoint, dir 1 — clamp to world bounds
    let clamped_end_x = end_x.max(0).min(state.world.cells_width() as i32 - 1);
    let clamped_end_y = end_y.max(0).min(state.world.cells_height() as i32 - 1);
    let fx = hb_directed_fx(
        net_u16_nonneg(pid),
        net_u16_nonneg(clamped_end_x),
        net_u16_nonneg(clamped_end_y),
        7,
        1,
        0,
    );
    state.broadcast_to_nearby(
        World::chunk_pos(px, py).0,
        World::chunk_pos(px, py).1,
        &encode_hb_bundle(&hb_bundle(&[fx]).1),
        None,
    );
    true
}
