//! Лечение и инвентарь.
use crate::game::player::{
    PlayerCooldowns, PlayerInventory, PlayerPosition, PlayerSkillsComp, PlayerStats,
};
use crate::net::session::outbound::inventory_sync::send_inventory;
use crate::net::session::play::death::request_death;
use crate::net::session::play::dig_build::broadcast_cell_update;
use crate::net::session::prelude::*;
use crate::net::session::social::buildings::{
    broadcast_building_placed, building_extra_for_pack_type, validate_building_area,
};

// ─── Inventory ──────────────────────────────────────────────────────────────

/// Helper: check if an item is exempt from facing-cell placement checks.
/// C# `Inventory.Use:208`: `selected is 40 or (>=10 and <17) or 34 or 42 or 43 or 46`
const fn is_exempt_item(sel: i32) -> bool {
    sel == 40 || is_geopack_item(sel)
}

fn send_inventory_state_error(tx: &Outbox) {
    send_u_packet(
        tx,
        "OK",
        &ok_message("ИНВЕНТАРЬ", "Состояние инвентаря недоступно.").1,
    );
}

#[cfg(test)]
pub async fn handle_inventory_use(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId) {
    // C# Inventory.Use:204 — гейт 400ms: если с прошлого использования прошло
    // меньше, Use() игнорируется целиком. Таймер обновляется при прохождении
    // гейта, даже если предмет в итоге не использован (как C# `time = DateTime.Now`).
    // ДЕВИАЦИЯ: building-items {0,1,2,3,24,26,29} в C# не входят в typeditems и
    // ставятся отдельным путём (без Inventory.time); здесь они идут через тот же
    // диспетчер, поэтому делят этот таймер. Benign (предотвращает дабл-плейсмент).
    let now = std::time::Instant::now();
    let gate_passed = state.modify_player(pid, |ecs, entity| {
        let mut cd = ecs.get_mut::<PlayerCooldowns>(entity)?;
        if now.duration_since(cd.last_inventory_use) >= std::time::Duration::from_millis(400) {
            cd.last_inventory_use = now;
            Some(true)
        } else {
            Some(false)
        }
    });
    let Some(gate_passed) = gate_passed.flatten() else {
        tracing::error!(player_id = %pid, "Player cooldowns missing for inventory use");
        send_inventory_state_error(tx);
        return;
    };
    if !gate_passed {
        return;
    }

    let selected = state.query_player_opt(pid, |ecs, entity| {
        let inv = ecs.get::<PlayerInventory>(entity)?;
        Some((inv.selected, *inv.items.get(&inv.selected).unwrap_or(&0)))
    });
    let Some((sel, count)) = selected else {
        tracing::error!(player_id = %pid, "Player inventory missing for inventory use");
        send_inventory_state_error(tx);
        return;
    };

    if sel < 0 || count <= 0 {
        return;
    }

    // D27+D28: C# Inventory.Use:206-208 — проверки facing-клетки перед использованием.
    // ContainsPack(facing) применяется ВСЕГДА (здание на facing блокирует ЛЮБОЙ
    // предмет; невалидные координаты C# трактует как занятые → return true).
    // Exemption ({40, 10-16, 34, 42, 43, 46}) обходит только can_place_over.
    let position = state.query_player_opt(pid, |ecs, entity| {
        let p = ecs.get::<PlayerPosition>(entity)?;
        Some((p.x, p.y, p.dir))
    });
    let Some((px, py, pdir)) = position else {
        tracing::error!(player_id = %pid, "Player position missing for inventory use");
        send_inventory_state_error(tx);
        return;
    };
    let (dx, dy) = dir_offset(pdir);
    let (fx, fy) = (px + dx, py + dy);
    // C# `ContainsPack` → `Chunk.GetPack`: блок ТОЛЬКО если facing = ORIGIN
    // здания (SetPack регистрирует Pack лишь в origin-клетке), НЕ footprint-aware.
    // Раньше был `find_pack_covering` (весь футпринт) → строже C#/клиента.
    if !state.world.valid_coord(fx, fy) || state.has_building_origin(fx, fy) {
        return;
    }
    // can_place_over: обходится только exempt-предметами.
    if !is_exempt_item(sel) {
        let cell = state.world.get_cell_typed(fx, fy);
        let cell_defs = state.world.cell_defs();
        if !cell_defs.get_typed(cell).can_place_over() {
            return;
        }
    }

    let used = if is_geopack_item(sel) {
        use_geopack(state, tx, pid, sel)
    } else if let Some(spec) = crate::game::logic::items::building_item(sel) {
        if spec.pack_type == PackType::Gate {
            use_gate_item(state, tx, pid).await
        } else {
            place_building_from_item(state, tx, pid, spec.pack_type).await
        }
    } else {
        match sel {
            35 => use_poli(state, pid),
            40 => use_c190(state, pid),
            _ => false,
        }
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

pub fn handle_inventory_use_sync_nonbuilding(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    session_id: crate::game::SessionId,
    due_actions: &mut crate::game::logic::due::DueActionQueue,
    broadcasts: &mut Vec<crate::game::BroadcastEffect>,
) -> bool {
    let selected = state.query_player_opt(pid, |ecs, entity| {
        let inv = ecs.get::<PlayerInventory>(entity)?;
        Some((inv.selected, *inv.items.get(&inv.selected).unwrap_or(&0)))
    });
    let Some((sel, count)) = selected else {
        tracing::error!(player_id = %pid, "Player inventory missing for inventory use");
        send_inventory_state_error(tx);
        return true;
    };
    if crate::game::logic::items::building_item(sel).is_some() {
        return false;
    }

    let delayed_kind = crate::game::logic::consumables::DelayedConsumableKind::from_item(sel);
    let mut due_reservation = if let Some(kind) = delayed_kind {
        let Some(reservation) = due_actions.try_reserve() else {
            crate::metrics::DUE_ACTIONS_TOTAL
                .with_label_values(&[kind.metric_label(), "saturated"])
                .inc();
            return true;
        };
        Some(reservation)
    } else {
        None
    };

    let now = std::time::Instant::now();
    let gate_passed = state.modify_player(pid, |ecs, entity| {
        let mut cd = ecs.get_mut::<PlayerCooldowns>(entity)?;
        if now.duration_since(cd.last_inventory_use) >= std::time::Duration::from_millis(400) {
            cd.last_inventory_use = now;
            Some(true)
        } else {
            Some(false)
        }
    });
    let Some(gate_passed) = gate_passed.flatten() else {
        tracing::error!(player_id = %pid, "Player cooldowns missing for inventory use");
        send_inventory_state_error(tx);
        return true;
    };
    if !gate_passed || sel < 0 || count <= 0 {
        return true;
    }

    let position = state.query_player_opt(pid, |ecs, entity| {
        let p = ecs.get::<PlayerPosition>(entity)?;
        Some((p.x, p.y, p.dir))
    });
    let Some((px, py, pdir)) = position else {
        tracing::error!(player_id = %pid, "Player position missing for inventory use");
        send_inventory_state_error(tx);
        return true;
    };
    let (dx, dy) = dir_offset(pdir);
    let (fx, fy) = (px + dx, py + dy);
    if !state.world.valid_coord(fx, fy) || state.has_building_origin(fx, fy) {
        return true;
    }
    if !is_exempt_item(sel) {
        let cell = state.world.get_cell_typed(fx, fy);
        let cell_defs = state.world.cell_defs();
        if !cell_defs.get_typed(cell).can_place_over() {
            return true;
        }
    }

    if let Some(kind) = delayed_kind {
        if kind.requires_gun_access() {
            let Some(cid) = state.query_player_opt(pid, |ecs, entity| {
                let player_stats = ecs.get::<PlayerStats>(entity)?;
                Some(player_stats.clan_id.unwrap_or(0))
            }) else {
                return true;
            };
            if !state.access_gun(fx, fy, cid) {
                return true;
            }
        }

        let Some(inventory_effect) = consume_inventory_item_effect(state, pid, session_id, sel)
        else {
            send_inventory_state_error(tx);
            return true;
        };
        let reservation = due_reservation
            .take()
            .expect("delayed consumable reservation must exist after admission");
        let sequence = reservation.sequence();
        let center = crate::game::WorldPos(fx, fy);
        let action = match kind {
            crate::game::logic::consumables::DelayedConsumableKind::Boom => {
                crate::game::logic::due::DueAction::Boom(
                    crate::game::logic::consumables::BoomDueAction {
                        center,
                        rng_seed: sequence,
                    },
                )
            }
            crate::game::logic::consumables::DelayedConsumableKind::Protector => {
                crate::game::logic::due::DueAction::Protector(
                    crate::game::logic::consumables::ProtectorDueAction {
                        center,
                        trigger_player_id: pid,
                    },
                )
            }
            crate::game::logic::consumables::DelayedConsumableKind::Raz => {
                crate::game::logic::due::DueAction::Raz(
                    crate::game::logic::consumables::RazDueAction {
                        center,
                        trigger_player_id: pid,
                    },
                )
            }
        };
        state.put_consumable_pack(fx, fy, b'B', kind.pack_offset());
        reservation.publish(std::time::Instant::now() + kind.delay(), action);
        broadcasts.push(crate::game::BroadcastEffect::BlockUpdate(center));
        broadcasts.push(inventory_effect);
        crate::metrics::DUE_ACTIONS_TOTAL
            .with_label_values(&[kind.metric_label(), "admitted"])
            .inc();
        drop(due_reservation);
        crate::metrics::DUE_ACTION_DEPTH.set(i64::try_from(due_actions.len()).unwrap_or(i64::MAX));
        return true;
    }

    let used = match sel {
        item if is_geopack_item(item) => use_geopack(state, tx, pid, item),
        35 => use_poli(state, pid),
        40 => use_c190(state, pid),
        _ => false,
    };

    if used {
        consume_selected_inventory_item(state, tx, pid, sel);
    }
    true
}

pub fn prepare_inventory_building_use(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
) -> Option<crate::game::logic::contracts::InventoryBuildingPlacement> {
    let selected = state.query_player_opt(pid, |ecs, entity| {
        let inv = ecs.get::<PlayerInventory>(entity)?;
        Some((inv.selected, *inv.items.get(&inv.selected).unwrap_or(&0)))
    });
    let Some((sel, count)) = selected else {
        tracing::error!(player_id = %pid, "Player inventory missing for inventory building use");
        send_inventory_state_error(tx);
        return None;
    };
    if sel < 0 || count <= 0 {
        return None;
    }

    let now = std::time::Instant::now();
    let gate_passed = state.modify_player(pid, |ecs, entity| {
        let mut cd = ecs.get_mut::<PlayerCooldowns>(entity)?;
        if now.duration_since(cd.last_inventory_use) >= std::time::Duration::from_millis(400) {
            cd.last_inventory_use = now;
            Some(true)
        } else {
            Some(false)
        }
    });
    let Some(gate_passed) = gate_passed.flatten() else {
        tracing::error!(player_id = %pid, "Player cooldowns missing for inventory building use");
        send_inventory_state_error(tx);
        return None;
    };
    if !gate_passed {
        return None;
    }

    let item_spec = crate::game::logic::items::building_item(sel)?;
    let pack_type = item_spec.pack_type;
    let offset_cells = item_spec.placement_offset;
    let pos = state.query_player_opt(pid, |ecs: &bevy_ecs::prelude::World, entity| {
        let p = ecs.get::<PlayerPosition>(entity)?;
        let s = ecs.get::<PlayerStats>(entity)?;
        Some((p.x, p.y, p.dir, s.clan_id.unwrap_or(0)))
    });
    let Some((px, py, pdir, player_clan)) = pos else {
        tracing::error!(player_id = %pid, "Player position/stats missing for inventory building use");
        send_inventory_state_error(tx);
        return None;
    };
    if pack_type == PackType::Gun && player_clan == 0 {
        return None;
    }
    let (dx, dy) = dir_offset(pdir);
    let (fx, fy) = (px + dx, py + dy);
    if !state.world.valid_coord(fx, fy) || state.has_building_origin(fx, fy) {
        return None;
    }
    let cell = state.world.get_cell_typed(fx, fy);
    let cell_defs = state.world.cell_defs();
    if !cell_defs.get_typed(cell).can_place_over() {
        return None;
    }

    let building_clan = if matches!(pack_type, PackType::Gate | PackType::Gun) {
        player_clan
    } else {
        0
    };
    let (bx, by) = (px + dx * offset_cells, py + dy * offset_cells);
    if pack_type == PackType::Gate {
        let (access, anygun) = state.access_gun_full(bx, by, building_clan);
        if building_clan == 0 || !access || !anygun {
            return None;
        }
    }
    if validate_building_area(state, bx, by, pack_type).is_err() {
        return None;
    }
    let extra = match building_extra_for_pack_type(pack_type) {
        Ok(extra) => extra,
        Err(e) => {
            tracing::error!(?pack_type, error = ?e, "Missing building config for inventory placement");
            return None;
        }
    };

    Some(crate::game::logic::contracts::InventoryBuildingPlacement {
        selected_item: sel,
        type_code: item_spec.database_code.to_owned(),
        pack_type,
        x: bx,
        y: by,
        owner_id: pid,
        clan_id: building_clan,
        extra,
    })
}

pub fn apply_inventory_building_placed(
    state: &Arc<GameState>,
    tx: &Outbox,
    placement: &crate::game::logic::contracts::InventoryBuildingPlacement,
    db_id: i32,
) {
    let spawn_spec = crate::game::BuildingSpawnSpec {
        id: db_id,
        pack_type: placement.pack_type,
        x: placement.x,
        y: placement.y,
        owner_id: placement.owner_id,
        clan_id: placement.clan_id,
        extra: &placement.extra,
    };
    state.spawn_building_runtime(&spawn_spec);
    let view = PackView {
        id: db_id,
        pack_type: placement.pack_type,
        x: placement.x,
        y: placement.y,
        owner_id: placement.owner_id,
        clan_id: placement.clan_id,
        charge: placement.extra.charge,
        max_charge: placement.extra.max_charge,
        hp: placement.extra.hp,
        max_hp: placement.extra.max_hp,
    };
    broadcast_building_placed(state, tx, placement.owner_id, &view, false);
    consume_selected_inventory_item(state, tx, placement.owner_id, placement.selected_item);
}

fn consume_selected_inventory_item(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    selected: i32,
) {
    state.modify_player(pid, |ecs, entity| {
        let mut inv = ecs.get_mut::<PlayerInventory>(entity)?;
        let c = inv.items.entry(selected).or_insert(0);
        *c -= 1;
        if *c <= 0 {
            inv.items.remove(&selected);
            inv.miniq.retain(|&x| x != selected);
        }
        send_inventory(tx, &mut inv);
        Some(())
    });
}

fn consume_inventory_item_effect(
    state: &Arc<GameState>,
    pid: PlayerId,
    session_id: crate::game::SessionId,
    selected: i32,
) -> Option<crate::game::BroadcastEffect> {
    let packet = state
        .modify_player(pid, |ecs, entity| {
            ecs.get::<crate::game::PlayerFlags>(entity)?;
            let packet = {
                let mut inventory = ecs.get_mut::<PlayerInventory>(entity)?;
                let count = inventory.items.get_mut(&selected)?;
                if *count <= 0 {
                    return None;
                }
                *count -= 1;
                if *count == 0 {
                    inventory.items.remove(&selected);
                    inventory.miniq.retain(|&item| item != selected);
                }
                crate::game::logic::inventory::inventory_packet(&mut inventory)
            };
            ecs.get_mut::<crate::game::PlayerFlags>(entity)?.dirty = true;
            Some(packet)
        })
        .flatten()?;
    Some(crate::game::BroadcastEffect::Direct {
        session_id,
        data: crate::net::session::wire::make_u_packet_bytes(packet.0, &packet.1),
    })
}

/// Item-to-alive-cell mapping for geopack items.
/// Returns `None` for item 10 (no placement), `Some(cell_type)` for all others.
const fn geopack_item_to_cell(item: i32) -> Option<crate::world::CellType> {
    match item {
        // 10 → None (как и любой неизвестный item, см. wildcard).
        11 => Some(crate::world::CellType(cell_type::ALIVE_CYAN)),
        12 => Some(crate::world::CellType(cell_type::ALIVE_RED)),
        13 => Some(crate::world::CellType(cell_type::ALIVE_VIOL)),
        14 => Some(crate::world::CellType(cell_type::ALIVE_BLACK)),
        15 => Some(crate::world::CellType(cell_type::ALIVE_WHITE)),
        16 => Some(crate::world::CellType(cell_type::ALIVE_BLUE)),
        34 => Some(crate::world::CellType(cell_type::HYPNO_ROCK)),
        // item 43 кладёт BlackRock, не RedRock: C# Inventory item 43 вызывает
        // Geopack(42) (Inventory.cs:170), а ветка 43=>RedRock в C# switch — мёртвый
        // код (ни один item не зовёт Geopack(43)).
        42 | 43 => Some(crate::world::CellType(cell_type::BLACK_ROCK)),
        46 => Some(crate::world::CellType(cell_type::ALIVE_RAINBOW)),
        _ => None,
    }
}

const fn is_geopack_item(item: i32) -> bool {
    matches!(item, 10..=16 | 34 | 42 | 43 | 46)
}

/// D26: Reverse mapping — alive cell to item ID for geopack pickup.
/// C# `World.isAlive` only includes the 7 alive types (not HypnoRock/BlackRock/RedRock).
const fn alive_cell_to_item(cell: crate::world::CellType) -> Option<i32> {
    match cell.0 {
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

/// C# `World.TrueEmpty(x,y)` (World.cs:251): клетка пригодна для размещения
/// geopack/poli, если её свойство `isEmpty` И на ней нет здания (`PackPart`) И тип
/// клетки не в {36,37,0,39}. `isEmpty`-набор: {30,31,32,33,34,35,36,37,39,83}, так
/// что эффективно разрешено {30,31,32,33,34,35,83}. `find_pack_covering` покрывает
/// весь footprint многоклеточных зданий (эквивалент chunk.packsprop).
fn true_empty(state: &Arc<GameState>, x: i32, y: i32) -> bool {
    let c = state.world.get_cell_typed(x, y);
    state.world.is_empty(x, y)
        && state.find_pack_covering(x, y).is_none()
        && !matches!(
            c.0,
            cell_type::NOTHING
                | cell_type::GOLDEN_ROAD
                | cell_type::BUILDING_DOOR
                | cell_type::POLYMER_ROAD
        )
}

/// D25+D26: Geopack — placement on truly empty cells, pickup of any alive cell.
pub fn use_geopack(state: &Arc<GameState>, _tx: &Outbox, pid: PlayerId, item_id: i32) -> bool {
    let pos = state.query_player_opt(pid, |ecs, entity| {
        let p = ecs.get::<PlayerPosition>(entity)?;
        Some((p.x, p.y, p.dir))
    });
    let Some((px, py, pdir)) = pos else {
        return false;
    };
    let (dx, dy) = dir_offset(pdir);
    let (fx, fy) = (px + dx, py + dy);
    if !state.world.valid_coord(fx, fy) {
        return false;
    }

    let facing_cell = state.world.get_cell_typed(fx, fy);

    // D26: pickup — if facing cell is ANY alive cell, pick it up and map to correct item.
    if let Some(pickup_item) = alive_cell_to_item(facing_cell) {
        state.world.destroy(fx, fy);
        broadcast_cell_update(state, fx, fy);
        // C# `ShitClass.Geopack` (ShitClass.cs): pickup делает `p.inventory[id]++`
        // и возвращает `true`, поэтому `Inventory.Use` декрементит использованный
        // geopack (`this[selected]--`). Добавляем поднятый тип и возвращаем true,
        // чтобы вызывающий код израсходовал geopack — паритет 1:1 с референсом.
        state.modify_player(pid, |ecs, entity| {
            let mut inv = ecs.get_mut::<PlayerInventory>(entity)?;
            let c = inv.items.entry(pickup_item).or_insert(0);
            *c += 1;
            Some(())
        });
        return true;
    }

    // D25: placement — клетка должна проходить C# World.TrueEmpty.
    if !true_empty(state, fx, fy) {
        return false;
    }

    // Item 10 has no placement.
    let Some(alive_cell) = geopack_item_to_cell(item_id) else {
        return false;
    };

    state.world.set_cell_typed(fx, fy, alive_cell);
    broadcast_cell_update(state, fx, fy);
    true
}

/// C# `ShitClass.Poli()` — place `POLYMER_ROAD` at the facing cell.
pub fn use_poli(state: &Arc<GameState>, pid: PlayerId) -> bool {
    let pos = state.query_player_opt(pid, |ecs, entity| {
        let p = ecs.get::<PlayerPosition>(entity)?;
        let s = ecs.get::<PlayerStats>(entity)?;
        Some((p.x, p.y, p.dir, s.clan_id.unwrap_or(0)))
    });
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
    // C# Poli кладёт PolymerRoad только при World.TrueEmpty; возвращает false
    // в любом случае (предмет не тратится — паритет с референсом).
    if !true_empty(state, fx, fy) {
        return false;
    }
    state
        .world
        .set_cell_typed(fx, fy, crate::world::CellType(cell_type::POLYMER_ROAD));
    broadcast_cell_update(state, fx, fy);
    false
}

/// C# Inventory item 27 (Gate): `if (p.clan != null && c.access && c.anygun)`.
/// Только в клане, без вражеской заряженной пушки рядом (access) и при наличии
/// любой пушки в радиусе (anygun). Иначе ворота не ставятся.
#[cfg(test)]
async fn use_gate_item(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId) -> bool {
    let info = state.query_player_opt(pid, |ecs, entity| {
        let p = ecs.get::<PlayerPosition>(entity)?;
        let s = ecs.get::<PlayerStats>(entity)?;
        Some((p.x, p.y, p.dir, s.clan_id.unwrap_or(0)))
    });
    let Some((px, py, pdir, cid)) = info else {
        return false;
    };
    if cid == 0 {
        return false; // C# p.clan != null
    }
    let (dx, dy) = dir_offset(pdir);
    let (bx, by) = (px + dx, py + dy);
    let (access, anygun) = state.access_gun_full(bx, by, cid);
    if !access || !anygun {
        return false;
    }
    place_building_from_item_with(state, tx, pid, PackType::Gate, 1, Some(cid)).await
}

#[cfg(test)]
pub async fn place_building_from_item(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    pack_type: PackType,
) -> bool {
    place_building_from_item_with(state, tx, pid, pack_type, 2, None).await
}

#[cfg(test)]
async fn place_building_from_item_with(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    pack_type: PackType,
    offset_cells: i32,
    clan_override: Option<i32>,
) -> bool {
    let pos = state.query_player_opt(pid, |ecs: &bevy_ecs::prelude::World, entity| {
        let p = ecs.get::<PlayerPosition>(entity)?;
        let s = ecs.get::<PlayerStats>(entity)?;
        Some((p.x, p.y, p.dir, s.clan_id.unwrap_or(0)))
    });
    let Some((px, py, pdir, player_clan)) = pos else {
        return false;
    };
    // C# `Inventory`: ТОЛЬКО Gun ставится с кланом и ТОЛЬКО `if p.clan != null`
    // (`new Gun(x,y,p.id,p.cid)`); прочие здания clanless. Без клана пушку не
    // ставим (и предмет не тратим — caller списывает лишь при `true`).
    if pack_type == PackType::Gun && player_clan == 0 {
        return false;
    }
    // Клан здания: пушке и Gate-item — клан игрока; прочим item-пакам — 0.
    let building_clan = clan_override.unwrap_or(if pack_type == PackType::Gun {
        player_clan
    } else {
        0
    });
    let (dx, dy) = dir_offset(pdir);
    let (bx, by) = (px + dx * offset_cells, py + dy * offset_cells);
    if validate_building_area(state, bx, by, pack_type).is_err() {
        return false;
    }
    let extra = match building_extra_for_pack_type(pack_type) {
        Ok(extra) => extra,
        Err(e) => {
            tracing::error!(?pack_type, error = ?e, "Missing building config for inventory placement");
            return false;
        }
    };
    let Some(item_spec) = crate::game::logic::items::building_item_for_pack(pack_type) else {
        return false;
    };
    let code = item_spec.database_code;
    let insert_spec = crate::game::BuildingInsertSpec {
        type_code: code,
        pack_type,
        x: bx,
        y: by,
        owner_id: pid,
        clan_id: building_clan,
        extra: &extra,
    };
    let created = state.insert_building_runtime(&insert_spec).await;
    if let Ok((db_id, _)) = created {
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
        broadcast_building_placed(state, tx, pid, &view, false);
        true
    } else {
        false
    }
}

/// Helper: check if a cell is a building block type (C# `World.isBuildingBlock`).
const fn is_building_block(cell: crate::world::CellType) -> bool {
    matches!(
        cell.0,
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
const fn is_alive_cell(cell: crate::world::CellType) -> bool {
    cell.is_living_crystal()
}

/// D17: C190 — C# `ShitClass.C190Shot` parity.
pub fn use_c190(state: &Arc<GameState>, pid: PlayerId) -> bool {
    let ctx = crate::game::ExpContext::from_state(state);
    let data = state.query_player_opt(pid, |ecs: &bevy_ecs::prelude::World, entity| {
        let p = ecs.get::<PlayerPosition>(entity)?;
        Some((p.x, p.y, p.dir))
    });
    let Some((px, py, pdir)) = data else {
        return false;
    };
    let (dx, dy) = dir_offset(pdir);
    // Starting from facing cell, 10 cells total
    let start_x = px + dx;
    let start_y = py + dy;
    // Endpoint = facing + 9*dir.
    let end_x = start_x + dx * 9;
    let end_y = start_y + dy * 9;
    // C# C190Shot пре-проверяет ValidCoord(endpoint): если конец за краем мира —
    // выстрел не происходит и предмет НЕ тратится (return false до FX и урона).
    if !state.world.valid_coord(end_x, end_y) {
        return false;
    }
    let now = std::time::Instant::now();

    // FX (type 7) шлётся ДО цикла урона, на endpoint: C# p.SendDFToBots(7, end,
    // p.id, dir=1) перед for-циклом. Для fx=7 клиент AddGunShot(x,y,bid,col) → bid
    // = стрелок (pid).
    let shot_fx = hb_gun_shot_fx(
        net_u16_nonneg(pid),
        net_u16_nonneg(end_x),
        net_u16_nonneg(end_y),
    );
    state.broadcast_hb_at(px, py, &[shot_fx], None);

    // Все 10 клеток валидны (endpoint проверен выше) — без early-break.
    let cell_defs = state.world.cell_defs();
    for i in 0..10 {
        let (tgt_x, tgt_y) = (start_x + dx * i, start_y + dy * i);

        // Damage valid cells: not alive, is_diggable, is_destructible, not building block
        let c = state.world.get_cell_typed(tgt_x, tgt_y);
        if !is_alive_cell(c) && !is_building_block(c) {
            let prop = cell_defs.get_typed(c);
            if prop.physical.is_diggable && prop.physical.is_destructible {
                state.world.damage_cell(tgt_x, tgt_y, 50.0);
                broadcast_cell_update(state, tgt_x, tgt_y);
            }
        }

        // Damage ALL players on this cell: 20 + 60 * c190stacks, then increment
        for opid in state.active_player_ids() {
            // Can't shoot self or clan members
            let c_info = state.query_player_opt(pid, |ecs, entity| {
                let shooter_stats = ecs.get::<PlayerStats>(entity)?;
                Some(shooter_stats.clan_id.unwrap_or(0))
            });
            let o_info = state.query_player_opt(opid, |ecs, entity| {
                let target_stats = ecs.get::<PlayerStats>(entity)?;
                Some(target_stats.clan_id.unwrap_or(0))
            });
            if opid == pid || (c_info.is_some() && c_info == o_info) {
                continue;
            }

            let Some(conn_tx) = state.player_sender(opid) else {
                continue;
            };

            let death_tx = state
                .modify_player(opid, |ecs: &mut bevy_ecs::prelude::World, entity| {
                    let (prev_x, prev_y) = {
                        let p = ecs.get::<PlayerPosition>(entity)?;
                        (p.x, p.y)
                    };
                    if prev_x != tgt_x || prev_y != tgt_y {
                        return None;
                    }
                    // Check protection
                    if let Some(cd) = ecs.get::<PlayerCooldowns>(entity) {
                        if cd.protection_until.is_some_and(|u| now < u) {
                            return None;
                        }
                    }
                    // Health skill exp (C# Player.Hurt → AddExp("l"))
                    let skill_pkt =
                        if let Some(mut skills) = ecs.get_mut::<PlayerSkillsComp>(entity) {
                            if let Some(sk) = ctx.add_skill_exp(&mut skills.states, "l", 1.0) {
                                Some(crate::net::session::wire::make_u_packet_bytes(sk.0, &sk.1))
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                    if let Some(pkt) = skill_pkt {
                        let _ = conn_tx.send(pkt);
                    }
                    // Get stacks and compute damage
                    let stacks = ecs
                        .get::<PlayerCooldowns>(entity)
                        .map_or(0, |cd| cd.c190_stacks);
                    let dmg = 20 + 60 * stacks;
                    // Apply damage
                    let (h, mh) = {
                        let s = ecs.get::<PlayerStats>(entity)?;
                        (s.health, s.max_health)
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
                        Some(true) // died
                    } else {
                        Some(false) // survived
                    }
                })
                .flatten();
            if let Some(died) = death_tx {
                if died {
                    request_death(state, opid);
                } else {
                    // Hurt FX for survivor: SendDFToBots(6, 0, 0, id, 0)
                    let fx = hb_hurt_fx(net_u16_nonneg(opid));
                    state.broadcast_hb_at(tgt_x, tgt_y, &[fx], None);
                }
            }
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::buildings::{BuildingMetadata, BuildingOwnership};
    use crate::test_support::{ServerTestHarness, drain_events};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    async fn make_test_state(label: &str) -> ServerTestHarness {
        ServerTestHarness::new(label, "inventory-user").await
    }

    fn building_at(state: &Arc<GameState>, x: i32, y: i32) -> Option<(PackType, i32)> {
        let entity = state.building_entity_at(x, y)?;
        let ecs = state.ecs.read();
        let meta = ecs.get::<BuildingMetadata>(entity)?;
        let own = ecs.get::<BuildingOwnership>(entity)?;
        Some((meta.pack_type, own.clan_id))
    }

    fn clear_building_footprint(state: &Arc<GameState>, x: i32, y: i32, pack_type: PackType) {
        for (dx, dy, _) in pack_type.building_cells().unwrap() {
            state.world.destroy(x + dx, y + dy);
        }
    }

    fn apply_heal_command(state: &Arc<GameState>, pid: PlayerId) {
        crate::game::logic::commands::apply_player_command(
            state,
            crate::game::PlayerCommand::Heal {
                player_id: pid,
                programmatic: false,
            },
        );
    }

    #[tokio::test]
    async fn heal_missing_stats_is_explicit_error_not_full_health_fallback() {
        let test = make_test_state("heal_missing_stats").await;
        let (_tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity).remove::<PlayerStats>();
        }

        apply_heal_command(&test.state, pid);

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));
    }

    #[tokio::test]
    async fn heal_missing_skills_is_explicit_error_not_no_repair_fallback() {
        let test = make_test_state("heal_missing_skills").await;
        let (_tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut stats = ecs.get_mut::<PlayerStats>(entity).unwrap();
            stats.health = 50;
            stats.max_health = 100;
            stats.crystals[2] = 1;
            ecs.entity_mut(entity).remove::<PlayerSkillsComp>();
        }

        apply_heal_command(&test.state, pid);

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));
    }

    #[tokio::test]
    async fn heal_without_repair_skill_stays_quiet_noop() {
        let test = make_test_state("heal_no_repair_skill").await;
        let (_tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut stats = ecs.get_mut::<PlayerStats>(entity).unwrap();
            stats.health = 50;
            stats.max_health = 100;
            stats.crystals[2] = 1;
        }

        apply_heal_command(&test.state, pid);

        assert!(drain_events(&mut rx).is_empty());
    }

    #[tokio::test]
    async fn inventory_use_missing_cooldowns_is_explicit_error_not_cooldown_fallback() {
        let test = make_test_state("inventory_missing_cooldowns").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity).remove::<PlayerCooldowns>();
        }

        handle_inventory_use(&test.state, &tx, pid).await;

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние инвентаря недоступно."));
    }

    #[tokio::test]
    async fn inventory_use_cooldown_gates_before_inventory_lookup() {
        let test = make_test_state("inventory_cooldown_before_inventory").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut cd = ecs.get_mut::<PlayerCooldowns>(entity).unwrap();
            cd.last_inventory_use -= Duration::from_millis(500);
        }

        handle_inventory_use(&test.state, &tx, pid).await;
        assert!(drain_events(&mut rx).is_empty());

        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity).remove::<PlayerInventory>();
        }

        handle_inventory_use(&test.state, &tx, pid).await;

        assert!(
            drain_events(&mut rx).is_empty(),
            "second INUS inside 400ms gate must be ignored before inventory lookup"
        );
    }

    #[tokio::test]
    async fn inventory_use_missing_inventory_is_explicit_error_not_unselected_fallback() {
        let test = make_test_state("inventory_missing_inventory").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut cd = ecs.get_mut::<PlayerCooldowns>(entity).unwrap();
            cd.last_inventory_use -= Duration::from_millis(500);
            ecs.entity_mut(entity).remove::<PlayerInventory>();
        }

        handle_inventory_use(&test.state, &tx, pid).await;

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние инвентаря недоступно."));
    }

    #[tokio::test]
    async fn inventory_use_missing_position_is_explicit_error_not_skipped_facing_checks() {
        let test = make_test_state("inventory_missing_position").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut cd = ecs.get_mut::<PlayerCooldowns>(entity).unwrap();
            cd.last_inventory_use -= Duration::from_millis(500);
            let mut inv = ecs.get_mut::<PlayerInventory>(entity).unwrap();
            inv.selected = 10;
            inv.items.insert(10, 1);
            ecs.entity_mut(entity).remove::<PlayerPosition>();
        }

        handle_inventory_use(&test.state, &tx, pid).await;

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние инвентаря недоступно."));
    }

    #[tokio::test]
    async fn saturated_due_queue_leaves_delayed_consumable_admission_state_unchanged() {
        for item_id in [5, 6, 7] {
            let label = format!("delayed_due_saturation_{item_id}");
            let test = make_test_state(&label).await;
            let (tx, mut rx) = test.connect_with_outbox(1);
            drain_events(&mut rx);

            let pid = PlayerId(test.player.id);
            let entity = test.state.get_player_entity(pid).unwrap();
            let original_cooldown = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
            {
                let mut ecs = test.state.ecs.write();
                ecs.get_mut::<PlayerCooldowns>(entity)
                    .unwrap()
                    .last_inventory_use = original_cooldown;
                let mut inventory = ecs.get_mut::<PlayerInventory>(entity).unwrap();
                inventory.selected = item_id;
                inventory.items.insert(item_id, 2);
            }
            let original_pack_count = test.state.consumable_packs.len();
            let mut due_actions = crate::game::logic::due::DueActionQueue::new(1);
            due_actions.try_reserve().unwrap().publish(
                Instant::now() + Duration::from_mins(1),
                crate::game::logic::due::DueAction::Boom(
                    crate::game::logic::consumables::BoomDueAction {
                        center: crate::game::WorldPos(20, 20),
                        rng_seed: 1,
                    },
                ),
            );
            let mut broadcasts = Vec::new();

            assert!(handle_inventory_use_sync_nonbuilding(
                &test.state,
                &tx,
                pid,
                crate::game::SessionId::new(1),
                &mut due_actions,
                &mut broadcasts,
            ));

            assert_eq!(due_actions.len(), 1);
            assert!(broadcasts.is_empty());
            assert_eq!(test.state.consumable_packs.len(), original_pack_count);
            let (cooldown, item_count) = test
                .state
                .query_player_opt(pid, |ecs, player_entity| {
                    Some((
                        ecs.get::<PlayerCooldowns>(player_entity)?
                            .last_inventory_use,
                        *ecs.get::<PlayerInventory>(player_entity)?
                            .items
                            .get(&item_id)?,
                    ))
                })
                .expect("player admission state");
            assert_eq!(cooldown, original_cooldown);
            assert_eq!(item_count, 2);
            assert!(drain_events(&mut rx).is_empty());
        }
    }

    #[tokio::test]
    async fn boom_effects_stay_on_active_session_during_reconnect_bind_race() {
        let test = make_test_state("boom_reconnect_generation").await;
        let (_old_tx, mut old_rx) = test.connect_with_outbox(1);
        drain_events(&mut old_rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut position = ecs.get_mut::<PlayerPosition>(entity).unwrap();
            position.x = 10;
            position.y = 10;
            position.dir = 0;
            ecs.get_mut::<PlayerCooldowns>(entity)
                .unwrap()
                .last_inventory_use = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
            let mut inventory = ecs.get_mut::<PlayerInventory>(entity).unwrap();
            inventory.selected = 5;
            inventory.items.insert(5, 2);
        }
        test.state.world.destroy(10, 11);

        let new_session = crate::game::SessionId::new(2);
        let (new_tx, mut new_rx) = crate::net::session::outbox::channel();
        test.state
            .sessions
            .register_test_outbox(new_session, new_tx);
        assert!(test.state.sessions.bind_player(new_session, pid));

        let mut due_actions = crate::game::logic::due::DueActionQueue::new(1);
        let effects = crate::game::logic::commands::apply_player_command_with_due(
            &test.state,
            crate::game::PlayerCommand::InventoryUse {
                session_id: crate::game::SessionId::new(1),
                player_id: pid,
            },
            &mut due_actions,
        );

        assert_eq!(due_actions.len(), 1);
        assert!(effects.broadcasts.iter().any(|effect| matches!(
            effect,
            crate::game::BroadcastEffect::Direct { session_id, .. }
                if *session_id == crate::game::SessionId::new(1)
        )));
        assert!(effects.broadcasts.iter().any(|effect| matches!(
            effect,
            crate::game::BroadcastEffect::BlockUpdate(position)
                if *position == crate::game::WorldPos(10, 11)
        )));

        crate::net::session::social::buildings::broadcast_block_at(&test.state, 10, 11);
        assert!(
            drain_events(&mut old_rx)
                .iter()
                .any(|(event, _)| event == "HB"),
            "active session must receive nearby gameplay output"
        );
        assert!(
            drain_events(&mut new_rx).is_empty(),
            "authenticated but not active reconnect must not receive gameplay output before init"
        );
    }

    #[tokio::test]
    async fn item_building_uses_pack_offset_two_not_three() {
        let test = make_test_state("inventory_build_offset").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut pos = ecs.get_mut::<PlayerPosition>(entity).unwrap();
            pos.x = 10;
            pos.y = 10;
            pos.dir = 0;
        }
        clear_building_footprint(&test.state, 10, 12, PackType::Teleport);

        assert!(place_building_from_item(&test.state, &tx, pid, PackType::Teleport).await);
        assert_eq!(
            building_at(&test.state, 10, 12),
            Some((PackType::Teleport, 0))
        );
        assert!(building_at(&test.state, 10, 13).is_none());
    }

    #[tokio::test]
    async fn gate_item_uses_offset_one_and_player_clan() {
        let test = make_test_state("inventory_gate_offset_clan").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut pos = ecs.get_mut::<PlayerPosition>(entity).unwrap();
            pos.x = 10;
            pos.y = 10;
            pos.dir = 0;
            ecs.get_mut::<PlayerStats>(entity).unwrap().clan_id = Some(7);
        }
        clear_building_footprint(&test.state, 10, 11, PackType::Gate);

        assert!(
            place_building_from_item_with(&test.state, &tx, pid, PackType::Gate, 1, Some(7)).await
        );
        assert_eq!(building_at(&test.state, 10, 11), Some((PackType::Gate, 7)));
        assert!(building_at(&test.state, 10, 13).is_none());
    }
}
