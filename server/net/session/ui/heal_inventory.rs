//! Лечение и инвентарь.
#![allow(clippy::cast_possible_truncation)]
use crate::game::buildings::{
    BuildingCrafting, BuildingFlags, BuildingMetadata, BuildingOwnership, BuildingStats,
    BuildingStorage, GridPosition, can_destroy, damage_building, is_damagable,
};
use crate::game::player::{
    PlayerConnection, PlayerCooldowns, PlayerInventory, PlayerPosition, PlayerSkills, PlayerStats,
};
use crate::net::session::outbound::inventory_sync::{add_choose_miniq, send_inventory};
use crate::net::session::play::death::handle_death;
use crate::net::session::play::dig_build::broadcast_cell_update;
use crate::net::session::prelude::*;
use crate::net::session::social::buildings::{
    broadcast_pack_update, building_extra_for_pack_type, destroy_damagable_building,
    place_building_in_world, validate_building_area,
};

// ─── Healing ────────────────────────────────────────────────────────────────

pub fn handle_heal(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId) {
    let result = state
        .modify_player(pid, |ecs, entity| {
            let (h, mh, cry2, hx, hy) = {
                let pstats = ecs.get::<PlayerStats>(entity)?;
                let pos = ecs.get::<PlayerPosition>(entity)?;
                (
                    pstats.health,
                    pstats.max_health,
                    pstats.crystals[2],
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
                let mut pstats_mut = ecs.get_mut::<PlayerStats>(entity)?;
                pstats_mut.crystals[2] -= 1;
                pstats_mut.health = (h + heal_amount).min(mh);
                (pstats_mut.health, pstats_mut.crystals, hx)
            };
            let skill_payload = if let Some(mut skills_mut) = ecs.get_mut::<PlayerSkills>(entity) {
                add_skill_exp(&mut skills_mut.states, "e", 1.0);
                Some(skill_progress_payload(&skills_mut.states))
            } else {
                None
            };
            Some((new_health, mh, new_crys, pid_val, hy, skill_payload))
        })
        .flatten();

    if let Some((h, mh, crys, hx, hy, skill_payload)) = result {
        send_u_packet(tx, "@L", &health(h, mh).1);
        send_u_packet(tx, "@B", &basket(&crys, 1).1);
        // Always send @S after skill exp (C# Skill.AddExp always sends)
        if let Some(sp) = skill_payload {
            let sk = skills_packet(&sp);
            send_u_packet(tx, sk.0, &sk.1);
        }
        // D20: C# sends coordinates (0, 0) for heal FX
        let fx = hb_directed_fx(net_u16_nonneg(pid), 0, 0, 5, 0, 0);
        state.broadcast_hb_at(hx, hy, &[fx], None);
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

pub async fn handle_inventory_use(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    // C# Inventory.Use:204 — гейт 400ms: если с прошлого использования прошло
    // меньше, Use() игнорируется целиком. Таймер обновляется при прохождении
    // гейта, даже если предмет в итоге не использован (как C# `time = DateTime.Now`).
    // ДЕВИАЦИЯ: building-items {0,1,2,3,24,26,29} в C# не входят в typeditems и
    // ставятся отдельным путём (без Inventory.time); здесь они идут через тот же
    // диспетчер, поэтому делят этот таймер. Benign (предотвращает дабл-плейсмент).
    let now = std::time::Instant::now();
    let gate_passed = state
        .modify_player(pid, |ecs, entity| {
            let mut cd = ecs.get_mut::<PlayerCooldowns>(entity)?;
            if now.duration_since(cd.last_inventory_use) >= std::time::Duration::from_millis(400) {
                cd.last_inventory_use = now;
                Some(true)
            } else {
                Some(false)
            }
        })
        .flatten()
        .unwrap_or(false);
    if !gate_passed {
        return;
    }

    let (sel, count) = state
        .query_player_opt(pid, |ecs, entity| {
            let inv = ecs.get::<PlayerInventory>(entity)?;
            Some((inv.selected, *inv.items.get(&inv.selected).unwrap_or(&0)))
        })
        .unwrap_or((-1, 0));

    if sel < 0 || count <= 0 {
        return;
    }

    // D27+D28: C# Inventory.Use:206-208 — проверки facing-клетки перед использованием.
    // ContainsPack(facing) применяется ВСЕГДА (здание на facing блокирует ЛЮБОЙ
    // предмет; невалидные координаты C# трактует как занятые → return true).
    // Exemption ({40, 10-16, 34, 42, 43, 46}) обходит только can_place_over.
    let check = state.query_player_opt(pid, |ecs, entity| {
        let p = ecs.get::<PlayerPosition>(entity)?;
        Some((p.x, p.y, p.dir))
    });
    if let Some((px, py, pdir)) = check {
        let (dx, dy) = dir_offset(pdir);
        let (fx, fy) = (px + dx, py + dy);
        // C# `ContainsPack` → `Chunk.GetPack`: блок ТОЛЬКО если facing = ORIGIN
        // здания (SetPack регистрирует Pack лишь в origin-клетке), НЕ footprint-aware.
        // Раньше был `find_pack_covering` (весь футпринт) → строже C#/клиента.
        if !state.world.valid_coord(fx, fy) || state.building_index.contains_key(&(fx, fy)) {
            return;
        }
        // can_place_over: обходится только exempt-предметами.
        if !is_exempt_item(sel) {
            let cell = state.world.get_cell(fx, fy);
            if !state.world.cell_defs().get(cell).can_place_over() {
                return;
            }
        }
    }

    let used = match sel {
        10..=16 | 34 | 42 | 43 | 46 => use_geopack(state, tx, pid, sel),
        0 => place_building_from_item(state, tx, pid, "T").await,
        1 => place_building_from_item(state, tx, pid, "R").await,
        2 => place_building_from_item(state, tx, pid, "U").await,
        3 => place_building_from_item(state, tx, pid, "M").await,
        // Предмет 4 = «пак кланс» (тип D, конфиг "Clans"). В C#-референсе был обрубок
        // `4 => (p) => true` (потреблялся, ничего не делал → «списывается но не ставится»).
        // Оригинал ставил здесь пак кланс. Восстановлено по поведению (клиент предмет
        // локально не списывает → раз «списывается», сервер его потреблял в этой ветке).
        4 => place_building_from_item(state, tx, pid, "D").await,
        24 => place_building_from_item(state, tx, pid, "F").await,
        26 => place_building_from_item(state, tx, pid, "G").await,
        27 => use_gate_item(state, tx, pid).await,
        29 => place_building_from_item(state, tx, pid, "L").await,
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
    // C# `Inventory.Choose` (Inventory.cs): AddChoose(id) — внутри no-op для -1 —
    // selected = id, затем ВСЕГДА SendU(InvToSend()), затем SendU(choose) при
    // id != -1 либо SendU(close) при id == -1. Второй пакет обязателен: только
    // он арм'ит предмет на клиенте (inventoryItem != -1) и включает Enter→INUS.
    let updated = state
        .modify_player(pid, |ecs, entity| {
            let mut inv = ecs.get_mut::<PlayerInventory>(entity)?;
            add_choose_miniq(&mut inv.miniq, id);
            inv.selected = id;
            send_inventory(tx, &mut inv);
            Some(())
        })
        .is_some();
    if !updated {
        return;
    }
    let (ev, payload) = if id == -1 {
        inventory_close()
    } else {
        inventory_choose()
    };
    send_u_packet(tx, ev, &payload);
}

/// Item-to-alive-cell mapping for geopack items.
/// Returns `None` for item 10 (no placement), `Some(cell_type)` for all others.
const fn geopack_item_to_cell(item: i32) -> Option<u8> {
    match item {
        // 10 → None (как и любой неизвестный item, см. wildcard).
        11 => Some(cell_type::ALIVE_CYAN),
        12 => Some(cell_type::ALIVE_RED),
        13 => Some(cell_type::ALIVE_VIOL),
        14 => Some(cell_type::ALIVE_BLACK),
        15 => Some(cell_type::ALIVE_WHITE),
        16 => Some(cell_type::ALIVE_BLUE),
        34 => Some(cell_type::HYPNO_ROCK),
        // item 43 кладёт BlackRock, не RedRock: C# Inventory item 43 вызывает
        // Geopack(42) (Inventory.cs:170), а ветка 43=>RedRock в C# switch — мёртвый
        // код (ни один item не зовёт Geopack(43)).
        42 | 43 => Some(cell_type::BLACK_ROCK),
        46 => Some(cell_type::ALIVE_RAINBOW),
        _ => None,
    }
}

/// D26: Reverse mapping — alive cell to item ID for geopack pickup.
/// C# `World.isAlive` only includes the 7 alive types (not HypnoRock/BlackRock/RedRock).
const fn alive_cell_to_item(cell: u8) -> Option<i32> {
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

/// C# `World.TrueEmpty(x,y)` (World.cs:251): клетка пригодна для размещения
/// geopack/poli, если её свойство `isEmpty` И на ней нет здания (`PackPart`) И тип
/// клетки не в {36,37,0,39}. `isEmpty`-набор: {30,31,32,33,34,35,36,37,39,83}, так
/// что эффективно разрешено {30,31,32,33,34,35,83}. `find_pack_covering` покрывает
/// весь footprint многоклеточных зданий (эквивалент chunk.packsprop).
fn true_empty(state: &Arc<GameState>, x: i32, y: i32) -> bool {
    let c = state.world.get_cell(x, y);
    state.world.is_empty(x, y)
        && state.find_pack_covering(x, y).is_none()
        && !matches!(c, 0 | 36 | 37 | 39)
}

/// D25+D26: Geopack — placement on truly empty cells, pickup of any alive cell.
pub fn use_geopack(
    state: &Arc<GameState>,
    _tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    item_id: i32,
) -> bool {
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

    let facing_cell = state.world.get_cell(fx, fy);

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

    state.world.set_cell(fx, fy, alive_cell);
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
    state.world.set_cell(fx, fy, cell_type::POLYMER_ROAD);
    broadcast_cell_update(state, fx, fy);
    false
}

/// C# Inventory item 27 (Gate): `if (p.clan != null && c.access && c.anygun)`.
/// Только в клане, без вражеской заряженной пушки рядом (access) и при наличии
/// любой пушки в радиусе (anygun). Иначе ворота не ставятся.
async fn use_gate_item(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) -> bool {
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
    let (bx, by) = (px + dx * 3, py + dy * 3);
    let (access, anygun) = state.access_gun_full(bx, by, cid);
    if !access || !anygun {
        return false;
    }
    place_building_from_item(state, tx, pid, "N").await
}

pub async fn place_building_from_item(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    code: &str,
) -> bool {
    let Some(pack_type) = PackType::from_str(code) else {
        return false;
    };
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
    // Клан здания: пушке — клан игрока (p.cid), остальным — 0 (1:1 C#).
    let building_clan = if pack_type == PackType::Gun {
        player_clan
    } else {
        0
    };
    let (dx, dy) = dir_offset(pdir);
    let (bx, by) = (px + dx * 3, py + dy * 3);
    if validate_building_area(state, bx, by, pack_type).is_err() {
        return false;
    }
    let extra = building_extra_for_pack_type(pack_type);
    let id = state
        .db
        .insert_building(code, bx, by, pid, building_clan, &extra)
        .await
        .ok();
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
                    clanzone: extra.clanzone,
                    broken_timer: None,
                },
                BuildingStorage {
                    money: extra.money_inside,
                    crystals: extra.crystals_inside,
                    items: extra.items_inside.clone(),
                },
                BuildingOwnership {
                    owner_id: pid,
                    clan_id: building_clan,
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
        let (cx, cy) = crate::world::World::chunk_pos(bx, by);
        state
            .chunk_buildings
            .entry((cx, cy))
            .or_default()
            .push(entity);
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
const fn is_building_block(cell: u8) -> bool {
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
const fn is_alive_cell(cell: u8) -> bool {
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
                let (prev_x, prev_y, h, mh, conn_tx) = {
                    let p = ecs.get::<PlayerPosition>(entity)?;
                    let s = ecs.get::<PlayerStats>(entity)?;
                    let c = ecs.get::<PlayerConnection>(entity)?;
                    (p.x, p.y, s.health, s.max_health, c.tx.clone())
                };
                if (prev_x - cx).abs() > scan_range || (prev_y - cy).abs() > scan_range {
                    return Some(None);
                }
                let dist = ((prev_x - cx) as f32).hypot((prev_y - cy) as f32);
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
                    if crate::game::skills::add_skill_exp(&mut skills.states, "l", 1.0) {
                        let sk = skills_packet(&skill_progress_payload(&skills.states));
                        let _ = conn_tx
                            .send(crate::net::session::wire::make_u_packet_bytes(sk.0, &sk.1));
                    }
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
                Some(Some((survived, prev_x, prev_y)))
            })
            .flatten();
        // Broadcast hurt FX for surviving players (C# SendDFToBots(6,0,0,id,0))
        if let Some(Some((true, prev_x, prev_y))) = hit_result {
            let fx = hb_directed_fx(net_u16_nonneg(opid), 0, 0, 6, 0, 0);
            state.broadcast_hb_at(prev_x, prev_y, &[fx], None);
        }
        let dead = state.query_player_opt(opid, |ecs, entity| {
            let s = ecs.get::<PlayerStats>(entity)?;
            let c = ecs.get::<PlayerConnection>(entity)?;
            (s.health <= 0).then(|| c.tx.clone())
        });
        if let Some(tx) = dead {
            killed.push((opid, tx));
        }
    }
    killed
}

/// Поставить transient pack-спрайт расходника (off 0=Boom/1=Prot/2=Raz, тип `B`).
/// C# `Chunk.SendPack('B', …)` шлёт ОДИН пак с cell-based PACKPOS; здесь `block_pos`
/// чанковый (клиент-truth, 77033c5) — клиентский `O` чистит весь блок, поэтому
/// регистрируем расходник и ре-бродкастим ВЕСЬ блок (здания + все расходники),
/// иначе спрайт стирает соседние здания и другие бумы.
fn send_consumable_pack(state: &Arc<GameState>, x: i32, y: i32, off: u8) {
    if state.pack_block_pos(x, y).is_none() {
        return;
    }
    state.consumable_packs.insert((x, y), (b'B', off));
    crate::net::session::social::buildings::broadcast_block_at(state, x, y);
}

/// Снять спрайт расходника: удалить из реестра и ре-бродкастить остаток блока.
fn clear_consumable_pack(state: &Arc<GameState>, x: i32, y: i32) {
    state.consumable_packs.remove(&(x, y));
    crate::net::session::social::buildings::broadcast_block_at(state, x, y);
}

/// D14: Boom — C# `ShitClass.Boom` parity.
/// C#: `AccessGun`-гейт → `SendPack`('B', off=0) → `AsyncAction`(1s) → детонация + `ClearPack`.
/// Предмет тратится сразу (return true), урон отложен на 1 секунду.
pub fn use_boom(state: &Arc<GameState>, pid: PlayerId) -> bool {
    let pos = state.query_player_opt(pid, |ecs: &bevy_ecs::prelude::World, entity| {
        let p = ecs.get::<PlayerPosition>(entity)?;
        let s = ecs.get::<PlayerStats>(entity)?;
        Some((p.x, p.y, p.dir, s.clan_id.unwrap_or(0)))
    });
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

    send_consumable_pack(state, cx, cy, 0);
    let st = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        boom_detonate(&st, pid, cx, cy);
        clear_consumable_pack(&st, cx, cy);
    });
    true
}

/// Детонация Boom через 1с (тело C# `AsyncAction`): `AoE` r=3.5 — разрушение клеток
/// + 40 HP игрокам + FX.
fn boom_detonate(state: &Arc<GameState>, pid: PlayerId, cx: i32, cy: i32) {
    // AoE: radius 3.5 centered on facing cell
    for ddx in -4..=4 {
        for ddy in -4..=4 {
            let tgt_x = cx + ddx;
            let tgt_y = cy + ddy;
            if !state.world.valid_coord(tgt_x, tgt_y) {
                continue;
            }
            let dist = (ddx as f32).hypot(ddy as f32);
            if dist > 3.5 {
                continue;
            }
            let c = state.world.get_cell(tgt_x, tgt_y);
            let defs = state.world.cell_defs();
            let prop = defs.get(c);
            // C# `!World.PackPart(...)`: footprint-aware (не только origin). Иначе
            // не-origin клетки многоклеточных зданий ошибочно рушатся AoE.
            if prop.physical.is_destructible && state.find_pack_covering(tgt_x, tgt_y).is_none() {
                // Special cell conversions
                if c == cell_type::RED_ROCK && rand::random::<u32>() % 100 >= 98 {
                    // 2% chance: 117 → 118
                    state.world.set_cell(tgt_x, tgt_y, cell_type::ACID_ROCK);
                } else if c == cell_type::ACID_ROCK {
                    // 118 → 103
                    state.world.set_cell(tgt_x, tgt_y, cell_type::ROCK);
                } else if c != cell_type::RED_ROCK && c != cell_type::ACID_ROCK {
                    state.world.destroy(tgt_x, tgt_y);
                }
                broadcast_cell_update(state, tgt_x, tgt_y);
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
    state.broadcast_hb_at(cx, cy, &[fx], None);
}

/// D15: Protector (item 6) — C# `ShitClass.Prot` — `AoE` bomb, NOT a shield.
/// C#: `AccessGun`-гейт → `SendPack`('B', off=1) → `AsyncAction`(2s) → детонация + `ClearPack`.
pub fn use_protector(state: &Arc<GameState>, pid: PlayerId) -> bool {
    let pos = state.query_player_opt(pid, |ecs: &bevy_ecs::prelude::World, entity| {
        let p = ecs.get::<PlayerPosition>(entity)?;
        let s = ecs.get::<PlayerStats>(entity)?;
        Some((p.x, p.y, p.dir, s.clan_id.unwrap_or(0)))
    });
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

    send_consumable_pack(state, cx, cy, 1);
    let st = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        prot_detonate(&st, pid, cx, cy);
        clear_consumable_pack(&st, cx, cy);
    });
    true
}

/// Детонация Protector через 2с (тело C# `AsyncAction`): `AoE` 3x3 — снос гейтов +
/// разрушение клеток + 50 HP игрокам + FX.
fn prot_detonate(state: &Arc<GameState>, pid: PlayerId, cx: i32, cy: i32) {
    // C# iterates -1..=1 with distance check <= 3.5 (always true in that range)
    for ddx in -1..=1 {
        for ddy in -1..=1 {
            let tgt_x = cx + ddx;
            let tgt_y = cy + ddy;
            if !state.world.valid_coord(tgt_x, tgt_y) {
                continue;
            }
            // Destroy gates in range
            // (Gate buildings are in building_index; check and remove if it's a Gate)
            // Считываем entity + DB-id + is_gate, ОТПУСКАЯ DashMap-ref ДО remove:
            // раньше `remove` звался при живом `get`-ref на том же ключе → потенциальный
            // same-shard дедлок. Берём (entity, id) и выходим из ref.
            let gate = state.building_index.get(&(tgt_x, tgt_y)).and_then(|e| {
                let ent = *e;
                let ecs = state.ecs.read();
                let meta = ecs.get::<BuildingMetadata>(ent)?;
                (meta.pack_type == PackType::Gate).then_some((ent, meta.id))
            });
            if let Some((ent, gate_id)) = gate {
                state.building_index.remove(&(tgt_x, tgt_y));
                let (gcx, gcy) = crate::world::World::chunk_pos(tgt_x, tgt_y);
                if let Some(mut e) = state.chunk_buildings.get_mut(&(gcx, gcy)) {
                    e.retain(|&x| x != ent);
                }
                state.ecs.write().despawn(ent);
                state.world.set_cell(tgt_x, tgt_y, cell_type::EMPTY);
                broadcast_cell_update(state, tgt_x, tgt_y);
                // Удалить из БД — иначе гейт ВОСКРЕСАЕТ при рестарте (рассинхрон
                // слоёв: world/ECS чисты, а DB-строка осталась). Async detached.
                let db = state.db.clone();
                tokio::spawn(async move {
                    if let Err(e) = db.delete_building(gate_id).await {
                        tracing::error!("gate DB delete failed: {e}");
                    }
                });
            }
            // Destroy destructible non-building cells
            let c = state.world.get_cell(tgt_x, tgt_y);
            let defs = state.world.cell_defs();
            let prop = defs.get(c);
            // C# `!World.PackPart(...)`: footprint-aware (не только origin). Иначе
            // не-origin клетки многоклеточных зданий ошибочно рушатся AoE.
            if prop.physical.is_destructible && state.find_pack_covering(tgt_x, tgt_y).is_none() {
                state.world.destroy(tgt_x, tgt_y);
                broadcast_cell_update(state, tgt_x, tgt_y);
            }
        }
    }

    // 50 HP damage to players in range
    let killed = aoe_damage_players(state, cx, cy, 1, 3.5, 50);
    for (opid, tx) in killed {
        handle_death(state, &tx, opid);
    }

    // C# ShitClass.Prot: SendDirectedFx(fx=1, x, y, dir=1, bid=0, color=1).
    let fx = hb_directed_fx(
        net_u16_nonneg(pid),
        net_u16_nonneg(cx),
        net_u16_nonneg(cy),
        1,
        1,
        1,
    );
    state.broadcast_hb_at(cx, cy, &[fx], None);
}

/// D16: Razryadka — C# `ShitClass.Raz` parity.
/// C#: `SendPack`('B', off=2) → `AsyncAction`(5s) → детонация + `ClearPack`. Без `AccessGun`.
pub fn use_razryadka(state: &Arc<GameState>, pid: PlayerId) -> bool {
    let pos = state.query_player_opt(pid, |ecs: &bevy_ecs::prelude::World, entity| {
        let p = ecs.get::<PlayerPosition>(entity)?;
        Some((p.x, p.y, p.dir))
    });
    let Some((px, py, pdir)) = pos else {
        return false;
    };
    let (dx, dy) = dir_offset(pdir);
    let (cx, cy) = (px + dx, py + dy);

    send_consumable_pack(state, cx, cy, 2);
    let st = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        raz_detonate(&st, pid, cx, cy).await;
        clear_consumable_pack(&st, cx, cy);
    });
    true
}

/// Детонация Razryadka через 5с (тело C# `AsyncAction`): `IDamagable`-здания в r=9.5
/// (Destroy при `CanDestroy`, иначе Damage 10) + 500 HP всем игрокам + FX.
async fn raz_detonate(state: &Arc<GameState>, pid: PlayerId, cx: i32, cy: i32) {
    // Collect IDamagable buildings in radius 9.5 and apply damage (C# ShitClass.Raz).
    let mut to_destroy: Vec<(i32, i32)> = Vec::new();
    let mut resend_charge_zero: Vec<(i32, i32)> = Vec::new();
    {
        let mut ecs = state.ecs.write();
        let mut query = ecs.query::<(
            &BuildingMetadata,
            &GridPosition,
            &BuildingOwnership,
            &mut BuildingStats,
        )>();
        for (meta, bpos, ownership, mut pstats) in query.iter_mut(&mut ecs) {
            if !is_damagable(meta.pack_type) || ownership.owner_id == 0 {
                continue;
            }
            let ddx = (bpos.x - cx) as f32;
            let ddy = (bpos.y - cy) as f32;
            if ddx.hypot(ddy) > 9.5 {
                continue;
            }
            if can_destroy(&pstats) {
                to_destroy.push((bpos.x, bpos.y));
            } else {
                damage_building(&mut pstats, 10);
                // C#: if (pack.charge == 0) ResendPack
                if pstats.charge == 0.0 {
                    resend_charge_zero.push((bpos.x, bpos.y));
                }
            }
        }
    }

    // Destroy buildings that reached CanDestroy (release ECS lock first to avoid deadlock).
    for (bx, by) in to_destroy {
        destroy_damagable_building(state, Some(pid), bx, by).await;
    }

    // Resend HB O for buildings whose charge hit 0 from damage.
    for (bx, by) in resend_charge_zero {
        if let Some(view) = state.get_pack_at(bx, by) {
            broadcast_pack_update(state, &view);
        }
    }

    // 500 HP damage to ALL players in radius 9.5
    let killed = aoe_damage_players(state, cx, cy, 10, 9.5, 500);
    for (opid, tx) in killed {
        handle_death(state, &tx, opid);
    }

    // C# ShitClass.Raz: SendDirectedFx(fx=1, x, y, dir=9, bid=0, color=2).
    let fx = hb_directed_fx(
        net_u16_nonneg(pid),
        net_u16_nonneg(cx),
        net_u16_nonneg(cy),
        1,
        9,
        2,
    );
    state.broadcast_hb_at(cx, cy, &[fx], None);
}

/// D17: C190 — C# `ShitClass.C190Shot` parity.
pub fn use_c190(state: &Arc<GameState>, pid: PlayerId) -> bool {
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
    let shot_fx = hb_directed_fx(
        net_u16_nonneg(pid),
        net_u16_nonneg(end_x),
        net_u16_nonneg(end_y),
        7,
        1,
        0,
    );
    state.broadcast_hb_at(px, py, &[shot_fx], None);

    // Все 10 клеток валидны (endpoint проверен выше) — без early-break.
    for i in 0..10 {
        let (tgt_x, tgt_y) = (start_x + dx * i, start_y + dy * i);

        // Damage valid cells: not alive, is_diggable, is_destructible, not building block
        let c = state.world.get_cell(tgt_x, tgt_y);
        if !is_alive_cell(c) && !is_building_block(c) {
            let defs = state.world.cell_defs();
            let prop = defs.get(c);
            if prop.physical.is_diggable && prop.physical.is_destructible {
                state.world.damage_cell(tgt_x, tgt_y, 50.0);
                broadcast_cell_update(state, tgt_x, tgt_y);
            }
        }

        // Damage ALL players on this cell: 20 + 60 * c190stacks, then increment
        for entry in &state.active_players {
            let opid = *entry.key();
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
                    let skill_pkt = if let Some(mut skills) = ecs.get_mut::<PlayerSkills>(entity) {
                        if crate::game::skills::add_skill_exp(&mut skills.states, "l", 1.0) {
                            let sk = skills_packet(&skill_progress_payload(&skills.states));
                            Some(crate::net::session::wire::make_u_packet_bytes(sk.0, &sk.1))
                        } else {
                            None
                        }
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
                        net_u16_nonneg(tgt_x),
                        net_u16_nonneg(tgt_y),
                        6,
                        0,
                        0,
                    );
                    state.broadcast_hb_at(tgt_x, tgt_y, &[fx], None);
                }
            }
        }
    }

    true
}
