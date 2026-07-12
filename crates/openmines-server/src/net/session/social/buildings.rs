//! Меню построек и установка здания на карте.
use crate::game::buildings::{BuildingFlags, PackType, PackView, get_building_config};
use crate::game::player::{PlayerFlags, PlayerPosition, PlayerStats, PlayerUI};
use crate::net::session::prelude::*;
use bevy_ecs::prelude::{Entity, World as EcsWorld};
use std::collections::HashMap;

fn send_building_state_error(tx: &Outbox) {
    send_u_packet(
        tx,
        "OK",
        &ok_message("ПОСТРОЙКА", "Состояние игрока недоступно.").1,
    );
}

// ─── Buildings ─────────────────────────────────────────────────────────

/// TY `Pope` → `StaticGUI.OpenGui` в `server_reference/.../StaticGUI.cs` (программатор).
/// Показывает список программ игрока из БД (кликабельный) или кнопку создания.
pub async fn handle_programmator_pope_menu(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId) {
    use crate::net::session::ui::horb::{Button, Horb, ListRow};
    let programs = match state.db.list_programs(pid.into()).await {
        Ok(programs) => programs,
        Err(e) => {
            tracing::error!(player_id = %pid, error = ?e, "Failed to load player programs");
            send_u_packet(tx, "OK", &ok_message("Ошибка", "Ошибка БД").1);
            return;
        }
    };
    tracing::info!(player_id = %pid, count = programs.len(), "PROGDIAG Pope");
    let mut win = Horb::new("ПРОГРАММАТОР");
    if programs.is_empty() {
        win = win
            .text("Нет программ")
            .button(Button::new("СОЗДАТЬ ПРОГРАММУ", "createprog"));
    } else {
        for prog in &programs {
            win = win.list_row(ListRow::new(
                prog.name.clone(),
                "ОТКРЫТЬ",
                format!("openprog:{}", prog.id),
            ));
        }
        win = win.button(Button::new("Создать", "createprog"));
    }
    win.send(state, tx, pid, "prog");
}

/// TY `Blds` → `Player.OpenMyBuildings()` (список построек владельца).
pub async fn handle_my_buildings_list(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId) {
    let mine: Vec<crate::db::buildings::BuildingRow> = match state
        .db
        .load_buildings_by_owner(pid.into())
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!(player_id = %pid, error = ?e, "Failed to load buildings for player");
            Vec::new()
        }
    };
    // Раньше все постройки сваливались в один `text` → окно росло за экран
    // (репорт). Теперь каждая — строка `list` (виджет в ScrollRect → ползунок).
    use crate::net::session::ui::horb::{Horb, ListRow};
    let mut win = Horb::new("Мои здания");
    if mine.is_empty() {
        win = win.text("(нет построек)");
    } else {
        for r in &mine {
            // subtitle="" → клиент скрывает кнопку строки (не-кликабельно),
            // вся инфа в title (`list[3n]`).
            win = win.list_row(ListRow::new(
                format!("{} {}:{}", r.type_code, r.x, r.y),
                String::new(),
                String::new(),
            ));
        }
    }
    win.close_button().send(state, tx, pid, "blds");
}

/// TY `DPBX` → `Basket.OpenBoxGui` (упрощённо: показать кристаллы).
pub fn handle_dpbx_crystal_box(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId) {
    use crate::net::session::ui::horb::{Button, Horb, ListRow};

    let Some(cry) =
        state.query_player_opt(pid, |ecs, e| ecs.get::<PlayerStats>(e).map(|s| s.crystals))
    else {
        return;
    };
    let mut win = Horb::new("Создание бокса").text("Кристаллы");
    for (i, n) in cry.iter().enumerate() {
        win = win.list_row(ListRow::new(
            format!("тип {i}: {n}"),
            String::new(),
            String::new(),
        ));
    }
    win.button(Button::new("ВЫЙТИ", "exit"))
        .send(state, tx, pid, "open_box");
}

pub fn handle_buildings_menu(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId) {
    use crate::net::session::ui::horb::{Button, Horb};

    Horb::new("ПОСТРОЙКИ")
        .text("Выберите здание")
        .button(Button::new("Респ (5000$)", "bld_place:R"))
        .button(Button::new("Телепорт (3000$)", "bld_place:T"))
        .button(Button::new("Пушка (8000$)", "bld_place:G"))
        .button(Button::new("UP (4000$)", "bld_place:U"))
        .button(Button::new("Склад (4000$)", "bld_place:L"))
        .button(Button::new("Крафтер (5000$)", "bld_place:F"))
        .button(Button::new("Спот (3000$)", "bld_place:O"))
        .button(Button::new("Маркет (6000$)", "bld_place:M"))
        .button(Button::new("Ворота (2000$)", "bld_place:N"))
        .close_button()
        .send(state, tx, pid, "blds");
}

pub async fn handle_place_building(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    type_code: &str,
) {
    let Some(pack_type) = PackType::from_str(type_code) else {
        send_building_error(tx, "Некорректное здание");
        return;
    };
    let Some(cfg) = get_building_config(pack_type) else {
        send_building_error(tx, "Конфиг здания не найден");
        return;
    };
    let cost = cfg.cost;

    let p_data = state.query_player(pid, |ecs, entity| {
        let pstats = ecs.get::<PlayerStats>(entity);
        let pos = ecs.get::<PlayerPosition>(entity);
        let flags = ecs.get::<PlayerFlags>(entity);
        match (pstats, pos, flags) {
            (Some(pstats), Some(pos), Some(_)) => {
                Ok((pstats.clan_id.unwrap_or(0), pos.x, pos.y, pos.dir))
            }
            _ => Err(()),
        }
    });

    let Some(Ok((player_clan, px, py, pdir))) = p_data else {
        send_building_state_error(tx);
        return;
    };

    if pack_type == PackType::Gate && player_clan == 0 {
        send_u_packet(tx, "OK", &ok_message("Ошибка", "Для ворот нужен клан").1);
        return;
    }

    let (dx, dy) = dir_offset(pdir);
    let (bx, by) = (px + dx * 3, py + dy * 3);

    if let Err(msg) = validate_building_area(state, bx, by, pack_type) {
        send_building_error(tx, msg);
        return;
    }

    let extra = match building_extra_for_pack_type(pack_type) {
        Ok(extra) => extra,
        Err(e) => {
            tracing::error!(?pack_type, error = ?e, "Missing building config for placement");
            send_u_packet(tx, "OK", &ok_message("Ошибка", "Конфиг здания не найден").1);
            return;
        }
    };
    let result = state.modify_player(pid, |ecs, entity| {
        if ecs.get::<PlayerStats>(entity).is_none() || ecs.get::<PlayerFlags>(entity).is_none() {
            send_building_state_error(tx);
            return None;
        }
        let mut s = ecs.get_mut::<PlayerStats>(entity)?;
        if s.money < cost {
            return None;
        }
        s.money -= cost;
        let m = s.money;
        let c = s.creds;
        let owner_clan = s.clan_id.unwrap_or(0);
        let mut flags = ecs.get_mut::<PlayerFlags>(entity)?;
        flags.dirty = true;
        send_u_packet(tx, "P$", &money(m, c).1);
        Some(owner_clan)
    });

    let Some(result) = result else {
        send_building_state_error(tx);
        return;
    };

    let Some(owner_clan) = result else {
        send_u_packet(tx, "OK", &ok_message("Ошибка", "Недостаточно денег").1);
        return;
    };

    let initial_clan = if pack_type == PackType::Gate {
        owner_clan
    } else {
        0
    };
    let insert_spec = crate::game::BuildingInsertSpec {
        type_code,
        pack_type,
        x: bx,
        y: by,
        owner_id: pid,
        clan_id: initial_clan,
        extra: &extra,
    };
    let (db_id, entity) = match state.insert_building_runtime(&insert_spec).await {
        Ok(created) => created,
        Err(_) => {
            let refunded = state
                .modify_player(pid, |ecs, entity| {
                    if ecs.get::<PlayerStats>(entity).is_none()
                        || ecs.get::<PlayerFlags>(entity).is_none()
                    {
                        send_building_state_error(tx);
                        return None;
                    }
                    let mut s = ecs.get_mut::<PlayerStats>(entity)?;
                    s.money += cost;
                    let m = s.money;
                    let c = s.creds;
                    let mut flags = ecs.get_mut::<PlayerFlags>(entity)?;
                    flags.dirty = true;
                    Some((m, c))
                })
                .flatten();
            if let Some((m, c)) = refunded {
                send_u_packet(tx, "P$", &money(m, c).1);
            }
            send_u_packet(tx, "OK", &ok_message("Ошибка", "Ошибка БД").1);
            return;
        }
    };

    // Spawn BotSpot entity for Spot buildings (1:1 with C# Spot.Build → new BotSpot).
    if pack_type == PackType::Spot {
        spawn_botspot(state, pid, bx, by, owner_clan, entity);
    }

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
    broadcast_building_placed(state, tx, pid, &view, true);
}

pub fn prepare_paid_building_placement(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    type_code: &str,
) -> Option<crate::game::logic::contracts::PaidBuildingPlacement> {
    let Some(pack_type) = PackType::from_str(type_code) else {
        send_building_error(tx, "Некорректное здание");
        return None;
    };
    let Some(cfg) = get_building_config(pack_type) else {
        send_building_error(tx, "Конфиг здания не найден");
        return None;
    };
    let cost = cfg.cost;

    let p_data = state.query_player(pid, |ecs, entity| {
        let pstats = ecs.get::<PlayerStats>(entity);
        let pos = ecs.get::<PlayerPosition>(entity);
        let flags = ecs.get::<PlayerFlags>(entity);
        match (pstats, pos, flags) {
            (Some(pstats), Some(pos), Some(_)) => {
                Ok((pstats.clan_id.unwrap_or(0), pos.x, pos.y, pos.dir))
            }
            _ => Err(()),
        }
    });

    let Some(Ok((player_clan, px, py, pdir))) = p_data else {
        send_building_state_error(tx);
        return None;
    };

    if pack_type == PackType::Gate && player_clan == 0 {
        send_u_packet(tx, "OK", &ok_message("Ошибка", "Для ворот нужен клан").1);
        return None;
    }

    let (dx, dy) = dir_offset(pdir);
    let (bx, by) = (px + dx * 3, py + dy * 3);

    if let Err(msg) = validate_building_area(state, bx, by, pack_type) {
        send_building_error(tx, msg);
        return None;
    }

    let extra = match building_extra_for_pack_type(pack_type) {
        Ok(extra) => extra,
        Err(e) => {
            tracing::error!(?pack_type, error = ?e, "Missing building config for placement");
            send_u_packet(tx, "OK", &ok_message("Ошибка", "Конфиг здания не найден").1);
            return None;
        }
    };
    let result = state.modify_player(pid, |ecs, entity| {
        if ecs.get::<PlayerStats>(entity).is_none() || ecs.get::<PlayerFlags>(entity).is_none() {
            send_building_state_error(tx);
            return None;
        }
        let mut s = ecs.get_mut::<PlayerStats>(entity)?;
        if s.money < cost {
            return None;
        }
        s.money -= cost;
        let m = s.money;
        let c = s.creds;
        let owner_clan = s.clan_id.unwrap_or(0);
        let mut flags = ecs.get_mut::<PlayerFlags>(entity)?;
        flags.dirty = true;
        send_u_packet(tx, "P$", &money(m, c).1);
        Some(owner_clan)
    });

    let Some(result) = result else {
        send_building_state_error(tx);
        return None;
    };

    let Some(owner_clan) = result else {
        send_u_packet(tx, "OK", &ok_message("Ошибка", "Недостаточно денег").1);
        return None;
    };

    let initial_clan = if pack_type == PackType::Gate {
        owner_clan
    } else {
        0
    };
    Some(crate::game::logic::contracts::PaidBuildingPlacement {
        type_code: type_code.to_owned(),
        pack_type,
        x: bx,
        y: by,
        owner_id: pid,
        owner_clan_id: owner_clan,
        building_clan_id: initial_clan,
        cost,
        extra,
    })
}

pub fn apply_paid_building_placed(
    state: &Arc<GameState>,
    tx: &Outbox,
    placement: &crate::game::logic::contracts::PaidBuildingPlacement,
    db_id: i32,
) {
    let spawn_spec = crate::game::BuildingSpawnSpec {
        id: db_id,
        pack_type: placement.pack_type,
        x: placement.x,
        y: placement.y,
        owner_id: placement.owner_id,
        clan_id: placement.building_clan_id,
        extra: &placement.extra,
    };
    let entity = state.spawn_building_runtime(&spawn_spec);
    if placement.pack_type == PackType::Spot {
        state.spawn_botspot_runtime(
            placement.owner_id,
            placement.x,
            placement.y,
            placement.owner_clan_id,
            entity,
        );
    }

    let view = PackView {
        id: db_id,
        pack_type: placement.pack_type,
        x: placement.x,
        y: placement.y,
        owner_id: placement.owner_id,
        clan_id: placement.building_clan_id,
        charge: placement.extra.charge,
        max_charge: placement.extra.max_charge,
        hp: placement.extra.hp,
        max_hp: placement.extra.max_hp,
    };
    broadcast_building_placed(state, tx, placement.owner_id, &view, true);
}

pub fn refund_paid_building_placement(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    cost: i64,
) {
    let refunded = state
        .modify_player(pid, |ecs, entity| {
            if ecs.get::<PlayerStats>(entity).is_none() || ecs.get::<PlayerFlags>(entity).is_none()
            {
                send_building_state_error(tx);
                return None;
            }
            let mut s = ecs.get_mut::<PlayerStats>(entity)?;
            s.money += cost;
            let m = s.money;
            let c = s.creds;
            let mut flags = ecs.get_mut::<PlayerFlags>(entity)?;
            flags.dirty = true;
            Some((m, c))
        })
        .flatten();
    if let Some((m, c)) = refunded {
        send_u_packet(tx, "P$", &money(m, c).1);
    }
    send_u_packet(tx, "OK", &ok_message("Ошибка", "Ошибка БД").1);
}

pub fn broadcast_building_placed(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    view: &PackView,
    close_gui: bool,
) {
    broadcast_pack_to_nearby(state, view);
    if close_gui {
        state.modify_player(pid, |ecs, entity| {
            if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) {
                ui.current_window = None;
            }
        });
        let g = gu_close();
        send_u_packet(tx, g.0, &g.1);
    }
    tracing::info!(
        player_id = %pid,
        building_type = view.pack_type.code(),
        x = view.x,
        y = view.y,
        "Player placed building"
    );
}

pub fn handle_remove_building(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    bx: i32,
    by: i32,
) {
    let Some(session_id) = state.sessions.session_for_player(pid) else {
        send_building_error(tx, "Сессия устарела");
        return;
    };
    state.enqueue_command(crate::game::PlayerCommand::RemovePack {
        remove: crate::game::RemovePack {
            x: bx,
            y: by,
            cause: crate::game::BuildingDeleteCause::PlayerRequest(
                crate::game::BuildingDeleteOrigin {
                    session_id,
                    player_id: pid,
                },
            ),
        },
    });
}

pub fn building_extra_for_pack_type(pack_type: PackType) -> anyhow::Result<BuildingExtra> {
    let cfg = get_building_config(pack_type)
        .ok_or_else(|| anyhow::anyhow!("missing building config for {pack_type:?}"))?;
    Ok(BuildingExtra {
        charge: cfg.charge,
        items_inside: HashMap::new(),
        max_charge: cfg.max_charge,
        cost: i32::try_from(cfg.cost)
            .map_err(|_| anyhow::anyhow!("building config cost overflow for {pack_type:?}"))?,
        hp: cfg.hp,
        max_hp: cfg.max_hp,
        money_inside: 0,
        crystals_inside: [0; 6],
        craft_recipe_id: None,
        craft_num: 0,
        craft_end_ts: 0,
        craft_ready: false,
        clanzone: 0,
    })
}

pub fn validate_building_area(
    state: &Arc<GameState>,
    bx: i32,
    by: i32,
    pack_type: PackType,
) -> Result<(), &'static str> {
    let cells = pack_type
        .building_cells()
        .map_err(|_| "Конфиг здания не найден")?;
    for (cdx, cdy, _) in cells {
        let cx = bx + cdx;
        let cy = by + cdy;
        if !state.world.valid_coord(cx, cy) || !state.world.is_empty(cx, cy) {
            return Err("Нет места");
        }
        if state.find_pack_covering(cx, cy).is_some() {
            return Err("Место занято");
        }
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
    if chunk_x < 0 || chunk_y < 0 {
        return vec![];
    }
    let mut out = Vec::new();
    for po in state.get_packs_in_chunk_area(chunk_x as u32, chunk_y as u32) {
        if let Some(bp) = state.pack_block_pos(i32::from(po.x), i32::from(po.y)) {
            if bp == block_pos {
                out.push((po.code, po.x, po.y, po.clan, po.off));
            }
        }
    }
    // Активные расходники-спрайты (boom/prot/raz) того же блока. ОБЯЗАТЕЛЬНО:
    // клиентский `O` чистит весь block_pos, поэтому пакет должен нести и здания,
    // и расходники вместе — иначе апдейт здания стирает бумы, а бум — здания.
    for (cx, cy, typ, off) in state.consumable_packs_in_block(block_pos) {
        out.push((typ, net_u16_nonneg(cx), net_u16_nonneg(cy), 0, off));
    }
    out
}

pub fn broadcast_pack_update(state: &Arc<GameState>, view: &PackView) {
    broadcast_block_at(state, view.x, view.y);
}

/// Ре-бродкаст ВСЕГО `block_pos` клетки `(x,y)`: здания + активные расходники
/// (см. `gather_block_packs`). Единственный путь эмиссии `O` вне `check_chunk_changed`
/// — держит инвариант «один `O` несёт весь блок».
pub fn broadcast_block_at(state: &Arc<GameState>, x: i32, y: i32) {
    if let Some(block_pos) = state.pack_block_pos(x, y) {
        let packs = gather_block_packs(state, block_pos);
        let sub = hb_packs(block_pos, &packs);
        let data = encode_hb_bundle(&hb_bundle(&[sub]).1);
        let (cx, cy) = World::chunk_pos(x, y);
        state.broadcast_to_nearby(cx, cy, &data, None);
    }
}

pub fn broadcast_pack_clear(state: &Arc<GameState>, view: &PackView) {
    broadcast_pack_update(state, view);
}

pub fn modify_pack_with_db<F, R>(
    state: &Arc<GameState>,
    pack_x: i32,
    pack_y: i32,
    f: F,
) -> Result<R, String>
where
    F: FnOnce(&mut EcsWorld, Entity) -> R,
{
    let entity = state
        .building_entity_at(pack_x, pack_y)
        .ok_or_else(|| "Объект не найден".to_string())?;
    let mut ecs = state.ecs_write_profiled("buildings.modify_pack_with_db");
    if ecs.get::<BuildingFlags>(entity).is_none() {
        return Err("Состояние здания недоступно".to_string());
    }
    let res = f(&mut ecs, entity);

    // Помечаем dirty для периодического snapshot flush (каждые 45с).
    // Флаг снимается только после admission в bounded persistence owner; принятые
    // команды не теряются и повторяются при transient DB error.
    let mut flags = ecs
        .get_mut::<BuildingFlags>(entity)
        .ok_or_else(|| "Состояние здания недоступно".to_string())?;
    flags.dirty = true;

    Ok(res)
}

fn pack_has_cell(
    _state: &Arc<GameState>,
    bx: i32,
    by: i32,
    pack_type: PackType,
    cx: i32,
    cy: i32,
) -> bool {
    pack_type
        .building_cells()
        .expect("loaded building pack type must have config")
        .iter()
        .any(|(dx, dy, _)| bx + dx == cx && by + dy == cy)
}

pub fn validate_pack_footprint(
    state: &Arc<GameState>,
    old_view: &PackView,
    new_x: i32,
    new_y: i32,
    new_type: PackType,
) -> Result<(), &'static str> {
    let cells = new_type
        .building_cells()
        .map_err(|_| "Конфиг здания не найден")?;
    for (dx, dy, _) in cells {
        let tx = new_x + dx;
        let ty = new_y + dy;
        if !state.world.valid_coord(tx, ty) {
            return Err("Нет места");
        }
        if !state.world.is_empty(tx, ty)
            && !pack_has_cell(state, old_view.x, old_view.y, old_view.pack_type, tx, ty)
        {
            return Err("Нет места");
        }
        if let Some((px, py)) = state.find_pack_covering(tx, ty) {
            if px != old_view.x || py != old_view.y {
                return Err("Место занято");
            }
        }
    }
    Ok(())
}

fn send_building_error(tx: &Outbox, text: &str) {
    send_u_packet(tx, "OK", &ok_message("Ошибка", text).1);
}

/// Перенос футпринта здания на новую позицию — ЗЕРКАЛО remove+place, чтобы все
/// слои совпали: клетки мира И O-оверлей пака (иконка). Без оверлей-броадкаста
/// иконка пака осталась бы на старом месте, хотя клетки/индекс/ECS/БД — на новом.
pub fn move_pack_cells(state: &Arc<GameState>, old_view: &PackView, nx: i32, ny: i32) {
    // Старая позиция: очистить клетки + снять O-оверлей.
    state.clear_building_footprint(old_view);
    broadcast_pack_clear(state, old_view);
    // Новая позиция: поставить клетки + O-оверлей (как initial runtime commit).
    let mut new_view = old_view.clone();
    new_view.x = nx;
    new_view.y = ny;
    state.place_building_footprint(nx, ny, new_view.pack_type);
    broadcast_pack_to_nearby(state, &new_view);
}

// ─── BotSpot spawning/despawning ────────────────────────────────────────────

/// Spawn a `BotSpot` entity associated with a Spot building.
/// 1:1 with C# `new BotSpot(x, y, owner)` called when Spot is placed.
pub fn spawn_botspot(
    state: &Arc<GameState>,
    owner_id: PlayerId,
    x: i32,
    y: i32,
    clan_id: i32,
    building_entity: Entity,
) {
    state.spawn_botspot_runtime(owner_id, x, y, clan_id, building_entity);
}

/// Уничтожить `IDamagable` здание (C# `Destroy(Player p)`): убрать из мира/ECS/DB, Gun-специфика.
/// Возвращает true при успехе.
pub fn destroy_damagable_building(
    state: &Arc<GameState>,
    trigger_pid: Option<PlayerId>,
    bx: i32,
    by: i32,
) -> bool {
    if state.building_entity_at(bx, by).is_none() {
        return false;
    }
    state.enqueue_command(crate::game::PlayerCommand::RemovePack {
        remove: crate::game::RemovePack {
            x: bx,
            y: by,
            cause: crate::game::BuildingDeleteCause::Damage {
                trigger_player_id: trigger_pid,
                origin_session_id: trigger_pid
                    .and_then(|player_id| state.sessions.session_for_player(player_id)),
            },
        },
    });
    true
}

/// Положить Box с кристаллами на месте снесённого здания (C# `Box.BuildBox(x,y,cry,null)`).
/// Проверка размещения 1:1: `isEmpty && can_place_over && !PackPart`.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::buildings::{BuildingMetadata, BuildingOwnership, GridPosition};
    use crate::game::player::PlayerMetadata;
    use crate::test_support::{ServerTestHarness, drain_events};

    async fn make_building_test_state(label: &str) -> ServerTestHarness {
        ServerTestHarness::new(&format!("buildings_{label}"), "building-user").await
    }

    fn player_money(game_state: &Arc<GameState>, pid: PlayerId) -> i64 {
        game_state
            .query_player_opt(pid, |ecs, entity| {
                let stats = ecs.get::<PlayerStats>(entity)?;
                Some(stats.money)
            })
            .unwrap()
    }

    fn admit_building_delete(
        state: &Arc<GameState>,
        command: crate::game::PlayerCommand,
    ) -> crate::game::BuildingDeleteRequest {
        let mut effects = crate::game::logic::commands::apply_player_command(state, command);
        assert!(effects.events.is_empty());
        assert!(effects.broadcasts.is_empty());
        assert_eq!(effects.saves.len(), 1);
        match effects.saves.pop().expect("building-delete save") {
            crate::game::SaveCommand::BuildingDelete { request } => request,
            _ => panic!("remove-pack must produce one typed building-delete save"),
        }
    }

    async fn persist_and_complete_building_delete(
        state: &Arc<GameState>,
        request: crate::game::BuildingDeleteRequest,
    ) -> crate::game::CommandEffects {
        let outcome = state
            .db
            .apply_building_delete(&crate::db::BuildingDeleteWrite {
                building_id: request.expected.building_id,
                x: request.expected.x,
                y: request.expected.y,
                clear_resp_bindings: request.view.pack_type == PackType::Resp,
                box_write: request.box_write.clone(),
            })
            .await
            .unwrap();
        let result = match outcome {
            crate::db::BuildingDeleteOutcome::Deleted {
                cleared_resp_bindings,
            } => crate::game::BuildingDeleteResult::Deleted {
                cleared_resp_bindings,
            },
            crate::db::BuildingDeleteOutcome::IdentityMismatch => {
                crate::game::BuildingDeleteResult::IdentityMismatch
            }
        };
        crate::game::logic::commands::apply_persistence_completion(
            state,
            crate::game::PersistenceCompletion::BuildingDeleted { request, result },
        )
    }

    #[tokio::test]
    async fn place_building_missing_flags_is_explicit_error_without_money_mutation() {
        let test = make_building_test_state("place_missing_flags").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut stats = ecs.get_mut::<PlayerStats>(entity).unwrap();
            stats.money = 10_000;
            ecs.entity_mut(entity).remove::<PlayerFlags>();
        }

        handle_place_building(&test.state, &tx, pid, "R").await;

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));
        assert_eq!(player_money(&test.state, pid), 10_000);
    }

    #[tokio::test]
    async fn modify_pack_missing_flags_rejects_without_mutating_building() {
        let test = make_building_test_state("pack_missing_flags").await;
        let building_entity = test
            .state
            .ecs
            .write()
            .spawn((
                BuildingMetadata {
                    id: 1,
                    pack_type: PackType::Resp,
                },
                GridPosition { x: 8, y: 8 },
                BuildingOwnership {
                    owner_id: PlayerId(1),
                    clan_id: 0,
                },
            ))
            .id();
        test.state.register_building_entity(8, 8, building_entity);

        let result = modify_pack_with_db(&test.state, 8, 8, |ecs, entity| {
            let mut ownership = ecs.get_mut::<BuildingOwnership>(entity).unwrap();
            ownership.owner_id = PlayerId(99);
        });

        assert_eq!(result, Err("Состояние здания недоступно".to_string()));
        let owner_id = {
            let ecs = test.state.ecs.read();
            ecs.get::<BuildingOwnership>(building_entity)
                .unwrap()
                .owner_id
        };
        assert_eq!(owner_id, PlayerId(1));
    }

    #[tokio::test]
    async fn destroying_resp_clears_matching_online_and_offline_resp_bindings() {
        let test = make_building_test_state("resp_destroy_clears_bindings").await;
        let (_tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let extra = crate::db::buildings::BuildingExtra {
            charge: 100,
            max_charge: 1000,
            cost: 10,
            hp: 1000,
            max_hp: 1000,
            money_inside: 0,
            crystals_inside: [0; 6],
            items_inside: HashMap::new(),
            craft_recipe_id: None,
            craft_num: 0,
            craft_end_ts: 0,
            craft_ready: false,
            clanzone: 0,
        };
        let spec = crate::game::BuildingInsertSpec {
            type_code: "R",
            pack_type: PackType::Resp,
            x: 10,
            y: 10,
            owner_id: PlayerId(test.player.id),
            clan_id: 0,
            extra: &extra,
        };
        let (building_id, _) = test.state.insert_building_runtime(&spec).await.unwrap();
        let offline = test.create_player("resp-offline-bound").await;
        test.state
            .db
            .update_player_resp(test.player.id, Some(10), Some(10))
            .await
            .unwrap();
        test.state
            .db
            .update_player_resp(offline.id, Some(10), Some(10))
            .await
            .unwrap();
        let pid = PlayerId(test.player.id);
        test.state.modify_player(pid, |ecs, entity| {
            let mut meta = ecs.get_mut::<PlayerMetadata>(entity)?;
            meta.resp_x = Some(10);
            meta.resp_y = Some(10);
            let mut flags = ecs.get_mut::<PlayerFlags>(entity)?;
            flags.dirty = false;
            Some(())
        });

        assert!(destroy_damagable_building(&test.state, None, 10, 10));
        let queued = test
            .state
            .commands_rx
            .lock()
            .as_mut()
            .expect("test command receiver")
            .try_recv()
            .expect("building delete request");
        assert!(matches!(
            queued.command,
            crate::game::PlayerCommand::RemovePack { .. }
        ));
        let request = admit_building_delete(&test.state, queued.command);
        assert!(test.state.get_pack_at(10, 10).is_none());
        assert!(
            test.state
                .db
                .load_all_buildings()
                .await
                .unwrap()
                .iter()
                .any(|building| building.id == building_id)
        );
        let _effects = persist_and_complete_building_delete(&test.state, request).await;

        assert!(test.state.get_pack_at(10, 10).is_none());
        assert!(
            test.state
                .db
                .load_all_buildings()
                .await
                .unwrap()
                .iter()
                .all(|b| b.id != building_id)
        );
        let online_db = test
            .state
            .db
            .get_player_by_id(test.player.id)
            .await
            .unwrap()
            .unwrap();
        let offline_db = test
            .state
            .db
            .get_player_by_id(offline.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!((online_db.resp_x, online_db.resp_y), (None, None));
        assert_eq!((offline_db.resp_x, offline_db.resp_y), (None, None));
        let online_ecs = test
            .state
            .query_player_opt(pid, |ecs, entity| {
                let meta = ecs.get::<PlayerMetadata>(entity)?;
                let flags = ecs.get::<PlayerFlags>(entity)?;
                Some((meta.resp_x, meta.resp_y, flags.dirty))
            })
            .unwrap();
        assert_eq!(online_ecs, (None, None, true));
    }

    #[tokio::test]
    async fn removed_teleport_persists_box_in_same_delete_transaction() {
        let test = make_building_test_state("teleport_box_effect").await;
        let extra = crate::db::buildings::BuildingExtra {
            charge: 7,
            max_charge: 100,
            cost: 0,
            hp: 100,
            max_hp: 100,
            money_inside: 0,
            crystals_inside: [0; 6],
            items_inside: HashMap::new(),
            craft_recipe_id: None,
            craft_num: 0,
            craft_end_ts: 0,
            craft_ready: false,
            clanzone: 0,
        };
        let spec = crate::game::BuildingInsertSpec {
            type_code: "T",
            pack_type: PackType::Teleport,
            x: 10,
            y: 10,
            owner_id: PlayerId(test.player.id),
            clan_id: 0,
            extra: &extra,
        };
        test.state.insert_building_runtime(&spec).await.unwrap();
        let request = admit_building_delete(
            &test.state,
            crate::game::PlayerCommand::RemovePack {
                remove: crate::game::RemovePack {
                    x: 10,
                    y: 10,
                    cause: crate::game::BuildingDeleteCause::Damage {
                        trigger_player_id: None,
                        origin_session_id: None,
                    },
                },
            },
        );

        assert!(request.box_write.as_ref().is_some_and(|write| {
            write.x == 10 && write.y == 10 && write.crystals == Some([0, 0, 0, 0, 7, 0])
        }));
        assert_ne!(
            test.state.world.get_cell_typed(10, 10),
            crate::world::CellType(crate::world::cells::cell_type::BOX)
        );
        let _effects = persist_and_complete_building_delete(&test.state, request).await;
        assert_eq!(
            test.state.world.get_cell_typed(10, 10),
            crate::world::CellType(crate::world::cells::cell_type::BOX)
        );
        assert_eq!(
            test.state.db.load_all_boxes().await.unwrap(),
            vec![(10, 10, [0, 0, 0, 0, 7, 0])]
        );
    }
}
