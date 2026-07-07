//! Меню построек и установка здания на карте.
use crate::game::broadcast_cell_update;
use crate::game::buildings::{
    BuildingFlags, BuildingStorage, PackType, PackView, get_building_config,
};
use crate::game::player::{
    PlayerConnection, PlayerFlags, PlayerInventory, PlayerPosition, PlayerStats, PlayerUI,
};
use crate::net::session::prelude::*;
use bevy_ecs::prelude::{Entity, World as EcsWorld};
use std::collections::HashMap;

/// Шанс дропа предмета-размещения при сносе здания (C# `Building.Destroy`: 40%).
const SHPAAK_DROP_PCT: u32 = 40;

fn send_building_state_error(tx: &mpsc::UnboundedSender<Vec<u8>>) {
    send_u_packet(
        tx,
        "OK",
        &ok_message("ПОСТРОЙКА", "Состояние игрока недоступно.").1,
    );
}

// ─── Buildings ─────────────────────────────────────────────────────────

/// TY `Pope` → `StaticGUI.OpenGui` в `server_reference/.../StaticGUI.cs` (программатор).
/// Показывает список программ игрока из БД (кликабельный) или кнопку создания.
pub async fn handle_programmator_pope_menu(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
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
pub async fn handle_my_buildings_list(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
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
pub fn handle_dpbx_crystal_box(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
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

pub fn handle_buildings_menu(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
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
    tx: &mpsc::UnboundedSender<Vec<u8>>,
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

pub fn broadcast_building_placed(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    view: &PackView,
    close_gui: bool,
) {
    broadcast_pack_to_nearby(state, view);
    if close_gui {
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

pub async fn handle_remove_building(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    bx: i32,
    by: i32,
) {
    let actor = state.query_player_opt(pid, |ecs, entity| {
        let p = ecs.get::<PlayerPosition>(entity)?;
        let s = ecs.get::<PlayerStats>(entity)?;
        Some((p.x, p.y, s.clan_id.unwrap_or(0)))
    });

    let Some(actor) = actor else {
        return;
    };
    let Some(view) = state.get_pack_at(bx, by) else {
        send_building_error(tx, "Объект не найден");
        return;
    };

    if view.owner_id != pid && !(view.clan_id != 0 && view.clan_id == actor.2) {
        send_building_error(tx, "Нет прав");
        return;
    }

    let cells = match view.pack_type.building_cells() {
        Ok(cells) => cells,
        Err(e) => {
            tracing::error!(pack_type = ?view.pack_type, error = ?e, "Missing building config for remove");
            send_building_error(tx, "Конфиг здания не найден");
            return;
        }
    };
    if !cells
        .iter()
        .any(|(dx, dy, _)| view.x + dx == actor.0 && view.y + dy == actor.1)
    {
        send_building_error(tx, "Вы не у объекта");
        return;
    }

    // Снос через C# `Destroy(p)`-семантику: `destroy_damagable_building` делает
    // DB-delete, despawn (+ `BotSpot` для Spot), очистку клеток, broadcast и
    // `close_pack_windows` (закрывает GUI у всех зрителей, включая сносящего),
    // плюс 40%-дроп предмета-размещения в инвентарь сносящего и Box-дроп
    // кристаллов (Storage) / charge (Teleport). Без C#-эталона для GUI-сноса —
    // унифицировано с боевым разрушением по решению (иначе терялись кристаллы).
    if !destroy_damagable_building(state, Some(pid), view.x, view.y).await {
        send_u_packet(tx, "OK", &ok_message("Ошибка", "Ошибка БД").1);
    }
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
                out.push((po.code, po.x, po.y, po.clan, po.charged));
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
    let mut ecs = state.ecs.write();
    if ecs.get::<BuildingFlags>(entity).is_none() {
        return Err("Состояние здания недоступно".to_string());
    }
    let res = f(&mut ecs, entity);

    // Помечаем dirty — периодический spawn_building_dirty_flush_loop (каждые 45с)
    // подхватит и сохранит. Снимает флаг только после успешного save, поэтому
    // ошибка БД не теряет изменения тихо. Немедленный tokio::spawn(save) убран:
    // он создавал два конкурирующих UPSERT к одной строке (dirty-flush + spawn).
    let mut flags = ecs
        .get_mut::<BuildingFlags>(entity)
        .ok_or_else(|| "Состояние здания недоступно".to_string())?;
    flags.dirty = true;

    Ok(res)
}

// TODO: will be used when pack footprint validation is fully connected
#[allow(dead_code)]
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

// TODO: will be used when building move/type change admin commands are fully wired
#[allow(dead_code)]
fn validate_pack_footprint(
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

fn send_building_error(tx: &mpsc::UnboundedSender<Vec<u8>>, text: &str) {
    send_u_packet(tx, "OK", &ok_message("Ошибка", text).1);
}

fn close_pack_windows(state: &Arc<GameState>, view: &PackView) {
    let window_key = format!("pack:{}:{}", view.x, view.y);
    let (pcx, pcy) = World::chunk_pos(view.x, view.y);
    for (cx, cy) in state.visible_chunks_around(pcx, pcy) {
        for pid in state.players_in_chunk(cx, cy) {
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
pub async fn destroy_damagable_building(
    state: &Arc<GameState>,
    trigger_pid: Option<PlayerId>,
    bx: i32,
    by: i32,
) -> bool {
    let Some(view) = state.get_pack_at(bx, by) else {
        return false;
    };
    // Захват crysinside до despawn (для Box-дропа Storage, C# `Storage.Destroy`).
    let crysinside: Option<[i64; 6]> = if view.pack_type == PackType::Storage {
        let ecs = state.ecs.read();
        state
            .building_entity_at(bx, by)
            .and_then(|entity| ecs.get::<BuildingStorage>(entity).map(|s| s.crystals))
    } else {
        None
    };

    if state.delete_building_runtime(&view).await.is_err() {
        return false;
    }
    broadcast_pack_clear(state, &view);
    close_pack_windows(state, &view);

    // C# `<Building>.Destroy()`: дроп кристаллов в Box.
    // Teleport — White по charge (`[0,0,0,0,charge,0]`); Storage — crysinside.
    match view.pack_type {
        PackType::Teleport if view.charge > 0 => {
            drop_destroy_box(state, bx, by, [0, 0, 0, 0, charge_to_crys(view.charge), 0]);
        }
        PackType::Storage => {
            if let Some(crys) = crysinside {
                if crys.iter().sum::<i64>() > 0 {
                    drop_destroy_box(state, bx, by, crys);
                }
            }
        }
        _ => {}
    }

    // C# `<Building>.Destroy()`: 40% шанс вернуть предмет-размещения в инвентарь сносящего
    // + HB bubble "ШПАААК ВЫПАЛ". Индекс = item-код здания (см. `shpaak_item_index`).
    if let (Some(pid), Some(item_idx)) = (trigger_pid, shpaak_item_index(view.pack_type)) {
        use rand::Rng as _;
        if rand::rng().random_range(1u32..=100) < SHPAAK_DROP_PCT {
            let tx = state.query_player_opt(pid, |ecs, entity| {
                ecs.get::<PlayerConnection>(entity).map(|c| c.tx.clone())
            });
            if let Some(tx) = tx {
                let chat_sub = hb_chat(
                    0,
                    u16::try_from(bx.rem_euclid(65536)).unwrap_or(0),
                    u16::try_from(by.rem_euclid(65536)).unwrap_or(0),
                    "ШПАААК ВЫПАЛ",
                );
                let _ = tx.send(encode_hb_bundle(&hb_bundle(&[chat_sub]).1));
                state.modify_player(pid, |ecs, entity| {
                    if ecs.get::<PlayerInventory>(entity).is_none()
                        || ecs.get::<PlayerFlags>(entity).is_none()
                    {
                        send_building_state_error(&tx);
                        return Some(());
                    }
                    let mut inv = ecs.get_mut::<PlayerInventory>(entity)?;
                    *inv.items.entry(item_idx).or_insert(0) += 1;
                    let mut flags = ecs.get_mut::<PlayerFlags>(entity)?;
                    flags.dirty = true;
                    Some(())
                });
            }
        }
    }
    true
}

/// Индекс предмета-размещения здания для 40%-дропа при `Destroy` (C# `p.inventory[N]++`).
/// `None` — здания без дропа (Gate/Spot и прочие).
const fn shpaak_item_index(pt: PackType) -> Option<i32> {
    match pt {
        PackType::Teleport => Some(0),
        PackType::Resp => Some(1),
        PackType::Up => Some(2),
        PackType::Market => Some(3),
        PackType::Craft => Some(24),
        PackType::Gun => Some(26),
        PackType::Storage => Some(29),
        _ => None,
    }
}

/// `charge` → кол-во кристаллов для Box.
/// 1:1 с C# `(long)charge`.
const fn charge_to_crys(charge: i32) -> i64 {
    if charge <= 0 { 0 } else { charge as i64 }
}

/// Положить Box с кристаллами на месте снесённого здания (C# `Box.BuildBox(x,y,cry,null)`).
/// Проверка размещения 1:1: `isEmpty && can_place_over && !PackPart`.
fn drop_destroy_box(state: &Arc<GameState>, x: i32, y: i32, crystals: [i64; 6]) {
    if !state.world.valid_coord(x, y) {
        return;
    }
    let cell = state.world.get_cell_typed(x, y);
    if !state.world.is_empty(x, y) || !state.world.cell_defs().get_typed(cell).can_place_over() {
        return;
    }
    if state.get_pack_at(x, y).is_some() {
        return;
    }
    state.put_box_cell(x, y, crystals);
    broadcast_cell_update(state, x, y);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::players::PlayerRow;
    use crate::game::buildings::{BuildingMetadata, BuildingOwnership, GridPosition};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::mpsc::UnboundedReceiver;

    struct BuildingTestState {
        state: Arc<GameState>,
        player: PlayerRow,
        db_path: PathBuf,
        world_name: String,
        dir: PathBuf,
    }

    impl BuildingTestState {
        fn cleanup(&self) {
            let _ = std::fs::remove_file(&self.db_path);
            let _ = std::fs::remove_file(self.dir.join(format!("{}_v2.map", self.world_name)));
            let _ =
                std::fs::remove_file(self.dir.join(format!("{}_durability.map", self.world_name)));
        }
    }

    async fn make_building_test_state(label: &str) -> BuildingTestState {
        let _ = crate::game::buildings::load_buildings_config(crate::test_config_path(
            "configs/buildings.json",
        ));
        let dir = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_path = dir.join(format!(
            "buildings_{label}_{}_{}.db",
            std::process::id(),
            nonce
        ));
        let _ = std::fs::remove_file(&db_path);

        let database = crate::db::Database::open(&db_path).await.unwrap();
        let player = database
            .create_player("building-user", "p", "h")
            .await
            .unwrap();

        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();
        let world_name = format!("buildings_world_{label}_{}_{}", std::process::id(), nonce);
        let world = crate::world::World::new(&world_name, 2, 2, cell_defs, &dir).unwrap();
        let config = crate::config::Config {
            world_name: world_name.clone(),
            port: 8090,
            world_chunks_w: 2,
            world_chunks_h: 2,
            data_dir: dir.to_string_lossy().to_string(),
            logging: crate::config::LoggingConfig::default(),
            cron: crate::config::CronConfig::default(),
            gameplay: crate::config::GameplayConfig::default(),
        };
        let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config)
            .await
            .unwrap();

        BuildingTestState {
            state,
            player,
            db_path,
            world_name,
            dir,
        }
    }

    fn drain_events(rx: &mut UnboundedReceiver<Vec<u8>>) -> Vec<(String, Vec<u8>)> {
        let mut events = Vec::new();
        while let Ok(frame) = rx.try_recv() {
            let mut buf = bytes::BytesMut::from(&frame[..]);
            let packet = crate::protocol::Packet::try_decode(&mut buf)
                .expect("valid packet")
                .expect("decoded packet");
            events.push((packet.event_str().to_owned(), packet.payload.to_vec()));
        }
        events
    }

    fn player_money(game_state: &Arc<GameState>, pid: PlayerId) -> i64 {
        game_state
            .query_player_opt(pid, |ecs, entity| {
                let stats = ecs.get::<PlayerStats>(entity)?;
                Some(stats.money)
            })
            .unwrap()
    }

    #[test]
    fn shpaak_destroy_item_indices_match_reference_inventory_slots() {
        assert_eq!(shpaak_item_index(PackType::Teleport), Some(0));
        assert_eq!(shpaak_item_index(PackType::Resp), Some(1));
        assert_eq!(shpaak_item_index(PackType::Up), Some(2));
        assert_eq!(shpaak_item_index(PackType::Market), Some(3));
        assert_eq!(shpaak_item_index(PackType::Craft), Some(24));
        assert_eq!(shpaak_item_index(PackType::Gun), Some(26));
        assert_eq!(shpaak_item_index(PackType::Storage), Some(29));
        assert_eq!(shpaak_item_index(PackType::Gate), None);
        assert_eq!(shpaak_item_index(PackType::Spot), None);
    }

    #[tokio::test]
    async fn place_building_missing_flags_is_explicit_error_without_money_mutation() {
        let test = make_building_test_state("place_missing_flags").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
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

        test.cleanup();
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

        test.cleanup();
    }
}
