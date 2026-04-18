//! Обработка нажатий GUI-кнопок клиента.
use crate::net::session::auth::gui_flow::{
    handle_auth_gui_finish_with_title, handle_auth_gui_registration_with_title,
};
use crate::net::session::auth::login::AuthState;
use crate::net::session::play::chunks::check_chunk_changed;
use crate::net::session::play::packs::{CRYSTAL_PRICES, build_pack_gui};
use crate::net::session::prelude::*;
use crate::net::session::social::buildings::{
    broadcast_pack_update, handle_place_building, handle_remove_building, update_pack_with_db,
};
use crate::net::session::social::clans::{
    handle_clan_accept, handle_clan_decline, handle_clan_join_request, handle_clan_leave,
    handle_clan_menu, handle_clan_preview, handle_clan_requests_view,
};

/// Pack / teleport / building GUI actions for a logged-in player.
pub fn handle_gui_world_buttons(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    button: &str,
) -> bool {
    if let Some(rest) = button.strip_prefix("resp_bind:") {
        if let Some((sx, sy)) = rest.split_once(':') {
            if let (Ok(rx), Ok(ry)) = (sx.parse::<i32>(), sy.parse::<i32>()) {
                let Some(pack) = state.get_pack_at(rx, ry) else {
                    send_u_packet(tx, "OK", &ok_message("Ошибка", "Респ не найден").1);
                    return true;
                };
                if pack.pack_type != PackType::Resp {
                    send_u_packet(tx, "OK", &ok_message("Ошибка", "Неверный тип объекта").1);
                    return true;
                }
                if pack.clan_id != 0 && pack.owner_id != pid {
                    send_u_packet(
                        tx,
                        "OK",
                        &ok_message("Ошибка", "Можно привязать только свой респ").1,
                    );
                    return true;
                }
                let Some(player_pos) = state.active_players.get(&pid).map(|p| (p.data.x, p.data.y))
                else {
                    return true;
                };
                if !pack
                    .pack_type
                    .building_cells()
                    .iter()
                    .any(|(dx, dy, _)| pack.x + dx == player_pos.0 && pack.y + dy == player_pos.1)
                {
                    send_u_packet(tx, "OK", &ok_message("Ошибка", "Вы не у этого респа").1);
                    return true;
                }
                if let Some(mut p) = state.active_players.get_mut(&pid) {
                    p.data.resp_x = Some(rx);
                    p.data.resp_y = Some(ry);
                }
                let _ = state.db.update_player_resp(pid, Some(rx), Some(ry));
                refresh_pack_gui(state, tx, pid, rx, ry);
            }
        }
        return true;
    }

    if let Some(rest) = button.strip_prefix("tp_go:") {
        if let Some((sx, sy)) = rest.split_once(':') {
            if let (Ok(tx_coord), Ok(ty_coord)) = (sx.parse::<i32>(), sy.parse::<i32>()) {
                let dest_x = tx_coord;
                let dest_y = ty_coord + 3;
                if !state.world.valid_coord(dest_x, dest_y) {
                    send_u_packet(tx, "OK", &ok_message("Ошибка", "Некорректные координаты").1);
                    return true;
                }
                let Some(pack) = state.get_pack_at(tx_coord, ty_coord) else {
                    send_u_packet(
                        tx,
                        "OK",
                        &ok_message("Ошибка", "Точка телепортации не найдена").1,
                    );
                    return true;
                };
                if pack.pack_type != PackType::Teleport {
                    send_u_packet(tx, "OK", &ok_message("Ошибка", "Неверный тип объекта").1);
                    return true;
                }
                if pack.owner_id != pid {
                    send_u_packet(tx, "OK", &ok_message("Ошибка", "Нет прав на телепорт").1);
                    return true;
                }
                if let Some(mut p) = state.active_players.get_mut(&pid) {
                    p.data.x = dest_x;
                    p.data.y = dest_y;
                    p.current_window = None;
                }
                send_u_packet(tx, "Gu", &[]);
                send_u_packet(tx, "@T", &tp(dest_x, dest_y).1);
                check_chunk_changed(state, tx, pid);
            }
        }
        return true;
    }

    if let Some(rest) = button.strip_prefix("bld_place:") {
        handle_place_building(state, tx, pid, rest);
        return true;
    }

    if let Some(rest) = button.strip_prefix("pack_remove:") {
        if let Some((sx, sy)) = rest.split_once(':') {
            if let (Ok(bx), Ok(by)) = (sx.parse::<i32>(), sy.parse::<i32>()) {
                let Some(p) = state.active_players.get(&pid) else {
                    send_u_packet(tx, "OK", &ok_message("Ошибка", "Игрок не найден").1);
                    return true;
                };
                let player_pos = (p.data.x, p.data.y);
                let player_clan = p.data.clan_id.unwrap_or(0);
                let Some(pack) = state.get_pack_at(bx, by).map(|pack| pack.clone()) else {
                    send_u_packet(tx, "OK", &ok_message("Ошибка", "Объект не найден").1);
                    return true;
                };
                if let Err(error) = validate_pack_access(&pack, player_pos, player_clan, pid) {
                    let message = match error {
                        PackAccessError::NotAtObject => "Вы не у этого объекта",
                        PackAccessError::NoRights => "Нет прав на объект",
                    };
                    send_u_packet(tx, "OK", &ok_message("Ошибка", message).1);
                    return true;
                }
                handle_remove_building(state, tx, pid, bx, by);
            }
        }
        return true;
    }

    // ─── Clan buttons ────────────────────────────────────────────────────
    if button == "clan_create" {
        let gui = serde_json::json!({
            "title": "СОЗДАТЬ КЛАН",
            "text": "Используйте команду в чате:\n/clan create ИМЯ ТЕГ\n\nПример: /clan create MyClan MC\nСтоимость: 1000 кр.",
            "buttons": ["Назад", "clan_back", "ВЫЙТИ", "exit"],
            "back": false
        });
        send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
        return true;
    }

    if button == "clan_back" {
        handle_clan_menu(state, tx, pid);
        return true;
    }

    if button == "clan_leave" {
        handle_clan_leave(state, tx, pid);
        return true;
    }

    if button == "clan_requests" {
        handle_clan_requests_view(state, tx, pid);
        return true;
    }

    if let Some(id_str) = button.strip_prefix("clan_view:") {
        if let Ok(clan_id) = id_str.parse::<i32>() {
            handle_clan_preview(state, tx, pid, clan_id);
        }
        return true;
    }

    if let Some(id_str) = button.strip_prefix("clan_request:") {
        if let Ok(clan_id) = id_str.parse::<i32>() {
            handle_clan_join_request(state, tx, pid, clan_id);
        }
        return true;
    }

    if let Some(pid_str) = button.strip_prefix("clan_accept:") {
        if let Ok(target_pid) = pid_str.parse::<i32>() {
            handle_clan_accept(state, tx, pid, target_pid);
        }
        return true;
    }

    if let Some(pid_str) = button.strip_prefix("clan_decline:") {
        if let Ok(target_pid) = pid_str.parse::<i32>() {
            handle_clan_decline(state, tx, pid, target_pid);
        }
        return true;
    }

    if let Some(rest) = button.strip_prefix("gun_fill:") {
        // gun_fill:AMOUNT:X:Y
        let parts: Vec<&str> = rest.splitn(3, ':').collect();
        if parts.len() == 3 {
            if let (Ok(bx), Ok(by)) = (parts[1].parse::<i32>(), parts[2].parse::<i32>()) {
                if !validate_pack_interaction(state, tx, pid, bx, by, PackType::Gun, &[]) {
                    return true;
                }
                let amount_str = parts[0];
                // Cost: 10 cyan per 100 charge
                let player_cyan = state
                    .active_players
                    .get(&pid)
                    .map(|p| p.data.crystals[5])
                    .unwrap_or(0);
                let (current_charge, max_charge) = state
                    .packs
                    .get(&(bx, by))
                    .map(|p| (p.charge, p.max_charge))
                    .unwrap_or((0.0, 0.0));
                let space = (max_charge - current_charge).max(0.0);
                let (fill_amount, cyan_cost) = if amount_str == "max" {
                    let fill = space;
                    #[allow(clippy::cast_possible_truncation)]
                    let cost = ((fill / 100.0) * 10.0).ceil() as i64;
                    (fill, cost)
                } else if let Ok(n) = amount_str.parse::<f32>() {
                    let fill = n.min(space);
                    #[allow(clippy::cast_possible_truncation)]
                    let cost = ((fill / 100.0) * 10.0).ceil() as i64;
                    (fill, cost)
                } else {
                    return true;
                };
                if cyan_cost > player_cyan {
                    send_u_packet(
                        tx,
                        "OK",
                        &ok_message("Ошибка", "Недостаточно циановых кристаллов").1,
                    );
                    return true;
                }
                if fill_amount <= 0.0 {
                    send_u_packet(tx, "OK", &ok_message("Пушка", "Заряд уже максимальный").1);
                    return true;
                }
                // Deduct cyan
                if let Some(mut p) = state.active_players.get_mut(&pid) {
                    p.data.crystals[5] -= cyan_cost;
                    let crys = p.data.crystals;
                    send_u_packet(tx, "@B", &basket(&crys, 1000).1);
                }
                if !with_updated_pack(
                    state,
                    tx,
                    bx,
                    by,
                    "обновление пушки",
                    |pack| {
                        pack.charge = (pack.charge + fill_amount).min(pack.max_charge);
                    },
                    |updated_pack| {
                        broadcast_pack_update(state, updated_pack);
                    },
                ) {
                    if let Some(mut p) = state.active_players.get_mut(&pid) {
                        p.data.crystals[5] += cyan_cost;
                        let crys = p.data.crystals;
                        send_u_packet(tx, "@B", &basket(&crys, 1000).1);
                    }
                    return true;
                }
                refresh_pack_gui(state, tx, pid, bx, by);
            }
        }
        return true;
    }

    if let Some(rest) = button.strip_prefix("storage_deposit:") {
        // storage_deposit:INDEX:AMOUNT:X:Y
        if let Some((idx, amount_str, bx, by)) = parse_pack_amount_payload(rest) {
            if idx < 6 {
                if !validate_pack_interaction(state, tx, pid, bx, by, PackType::Storage, &[]) {
                    return true;
                }
                let player_amount = state
                    .active_players
                    .get(&pid)
                    .map(|p| p.data.crystals[idx])
                    .unwrap_or(0);
                let deposit = parse_amount_arg(&amount_str, player_amount);
                if deposit > 0 {
                    if let Some(mut p) = state.active_players.get_mut(&pid) {
                        p.data.crystals[idx] -= deposit;
                        let crys = p.data.crystals;
                        send_u_packet(tx, "@B", &basket(&crys, 1000).1);
                    }
                    if !with_updated_pack(
                        state,
                        tx,
                        bx,
                        by,
                        "добавление кристаллов в хранилище",
                        |pack| {
                            pack.crystals_inside[idx] += deposit;
                        },
                        |updated_pack| {
                            broadcast_pack_update(state, updated_pack);
                        },
                    ) {
                        if let Some(mut p) = state.active_players.get_mut(&pid) {
                            p.data.crystals[idx] += deposit;
                            let crys = p.data.crystals;
                            send_u_packet(tx, "@B", &basket(&crys, 1000).1);
                        }
                        return true;
                    }
                    refresh_pack_gui(state, tx, pid, bx, by);
                }
            }
        }
        return true;
    }

    if let Some(rest) = button.strip_prefix("storage_withdraw:") {
        // storage_withdraw:INDEX:AMOUNT:X:Y
        if let Some((idx, amount_str, bx, by)) = parse_pack_amount_payload(rest) {
            if idx < 6 {
                if !validate_pack_interaction(state, tx, pid, bx, by, PackType::Storage, &[]) {
                    return true;
                }
                let stored = state
                    .packs
                    .get(&(bx, by))
                    .map(|p| p.crystals_inside[idx])
                    .unwrap_or(0);
                let withdraw = parse_amount_arg(&amount_str, stored);
                if withdraw > 0 {
                    if !with_updated_pack(
                        state,
                        tx,
                        bx,
                        by,
                        "изъятие кристаллов из хранилища",
                        |pack| {
                            pack.crystals_inside[idx] -= withdraw;
                        },
                        |updated_pack| {
                            if let Some(mut p) = state.active_players.get_mut(&pid) {
                                p.data.crystals[idx] += withdraw;
                                let crys = p.data.crystals;
                                send_u_packet(tx, "@B", &basket(&crys, 1000).1);
                            }
                            broadcast_pack_update(state, updated_pack);
                        },
                    ) {
                        return true;
                    }
                    refresh_pack_gui(state, tx, pid, bx, by);
                }
            }
        }
        return true;
    }

    if let Some(rest) = button.strip_prefix("market_sell:") {
        // market_sell:INDEX:AMOUNT:X:Y
        if let Some((idx, amount_str, bx, by)) = parse_pack_amount_payload(rest) {
            if idx < 6 {
                if !validate_pack_interaction(state, tx, pid, bx, by, PackType::Market, &[]) {
                    return true;
                }
                let player_amount = state
                    .active_players
                    .get(&pid)
                    .map(|p| p.data.crystals[idx])
                    .unwrap_or(0);
                let sell_amount = parse_amount_arg(&amount_str, player_amount);
                if sell_amount > 0 {
                    let earned = sell_amount * CRYSTAL_PRICES[idx];
                    if let Some(mut p) = state.active_players.get_mut(&pid) {
                        p.data.crystals[idx] -= sell_amount;
                        p.data.money += earned;
                        let crys = p.data.crystals;
                        send_u_packet(tx, "@B", &basket(&crys, 1000).1);
                        send_u_packet(tx, "P$", &money(p.data.money, p.data.creds).1);
                    }
                }
            }
        }
        return true;
    }

    if let Some(rest) = button.strip_prefix("craft_start:") {
        // craft_start:RECIPE_ID:NUM:X:Y
        let mut parts = rest.splitn(4, ':');
        let recipe_id = parts.next().and_then(|s| s.parse::<i32>().ok());
        let num = parts
            .next()
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(1)
            .max(1);
        let bx = parts.next().and_then(|s| s.parse::<i32>().ok());
        let by = parts.next().and_then(|s| s.parse::<i32>().ok());
        if let (Some(recipe_id), Some(bx), Some(by)) = (recipe_id, bx, by) {
            handle_craft_start(state, tx, pid, recipe_id, num, bx, by);
        }
        return true;
    }

    if let Some(rest) = button.strip_prefix("craft_claim:") {
        if let Some((sx, sy)) = rest.split_once(':') {
            if let (Ok(bx), Ok(by)) = (sx.parse::<i32>(), sy.parse::<i32>()) {
                handle_craft_claim(state, tx, pid, bx, by);
            }
        }
        return true;
    }

    if let Some(code) = button.strip_prefix("skill_up:") {
        let code = code.to_string();
        let skill_data = {
            let Some(mut p) = state.active_players.get_mut(&pid) else {
                return true;
            };
            if let Some(ss) = p.data.skills.get_mut(&code) {
                let max_exp = 100.0 * ss.level as f32;
                if ss.exp >= max_exp {
                    ss.level += 1;
                    ss.exp = 0.0;
                }
            }
            skill_progress_payload(&p.data.skills)
        };
        send_u_packet(tx, "SK", &skills_packet(&skill_data).1);
        if let Some(p) = state.active_players.get(&pid) {
            let _ = state.db.save_player(&p.data);
        }
        return true;
    }

    if button == "skill_install" {
        // Show available skills not yet installed
        let installed: std::collections::HashSet<String> = state
            .active_players
            .get(&pid)
            .map(|p| p.data.skills.keys().cloned().collect())
            .unwrap_or_default();
        let all_codes = [
            "d", "M", "m", "l", "A", "L", "Y", "r", "W", "O", "I", "e", "p", "pb", "pc", "pg",
            "pr", "pv", "pw", "g", "F", "RM",
        ];
        let mut btns: Vec<serde_json::Value> = Vec::new();
        for code in all_codes {
            if !installed.contains(code) {
                btns.push(serde_json::json!(format!("Добавить {}", code)));
                btns.push(serde_json::json!(format!("skill_add:{}", code)));
            }
        }
        btns.push(serde_json::json!("НАЗАД"));
        btns.push(serde_json::json!("exit"));
        let gui = serde_json::json!({
            "title": "Навыки",
            "text": "Доступные навыки для установки:",
            "buttons": btns,
            "back": false
        });
        send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
        return true;
    }

    if let Some(code) = button.strip_prefix("skill_add:") {
        let code = code.to_string();
        if SkillType::from_code(&code).is_none() {
            send_u_packet(tx, "OK", &ok_message("Ошибка", "Неверный код навыка").1);
            return true;
        }
        {
            let Some(mut p) = state.active_players.get_mut(&pid) else {
                return true;
            };
            p.data
                .skills
                .entry(code)
                .or_insert(crate::db::SkillState { level: 1, exp: 0.0 });
        }
        let skill_data = {
            let Some(p) = state.active_players.get(&pid) else {
                return true;
            };
            let _ = state.db.save_player(&p.data);
            skill_progress_payload(&p.data.skills)
        };
        send_u_packet(tx, "SK", &skills_packet(&skill_data).1);
        return true;
    }

    false
}

fn validate_pack_interaction(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    bx: i32,
    by: i32,
    expected_type: PackType,
    allowed_types: &[PackType],
) -> bool {
    let Some(pack) = state.get_pack_at(bx, by) else {
        send_u_packet(tx, "OK", &ok_message("Ошибка", "Объект не найден").1);
        return false;
    };
    if pack.pack_type != expected_type && !allowed_types.contains(&pack.pack_type) {
        send_u_packet(tx, "OK", &ok_message("Ошибка", "Неверный объект").1);
        return false;
    }
    let Some(player) = state.active_players.get(&pid) else {
        send_u_packet(tx, "OK", &ok_message("Ошибка", "Игрок не найден").1);
        return false;
    };
    let player_pos = (player.data.x, player.data.y);
    let player_clan = player.data.clan_id.unwrap_or(0);

    match validate_pack_access(&pack, player_pos, player_clan, pid) {
        Ok(()) => {}
        Err(PackAccessError::NotAtObject) => {
            send_u_packet(tx, "OK", &ok_message("Ошибка", "Вы не у этого объекта").1);
            return false;
        }
        Err(PackAccessError::NoRights) => {
            send_u_packet(tx, "OK", &ok_message("Ошибка", "Нет прав на объект").1);
            return false;
        }
    }
    true
}

pub fn handle_gui_auth_early(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    button: &str,
) -> bool {
    if button.starts_with("exit") {
        send_u_packet(tx, "Gu", &[]);
        return true;
    }

    handle_auth_gui_registration_with_title(state, tx, button, "auth")
}

pub fn handle_gui_auth_login(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    _pid: PlayerId,
    button: &str,
) -> bool {
    let mut auth_state = AuthState::Authenticated;
    matches!(
        handle_auth_gui_finish_with_title(state, tx, button, &mut auth_state, "auth"),
        ControlFlow::Break(_)
    )
}

pub fn handle_gui_button(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    button: &str,
) {
    if handle_gui_auth_early(state, tx, button) {
        return;
    }
    if handle_gui_auth_login(state, tx, pid, button) {
        return;
    }

    if handle_gui_world_buttons(state, tx, pid, button) {
        return;
    }

    tracing::debug!("GUI button unknown from pid {pid} len={}", button.len());
}

fn parse_pack_amount_payload(rest: &str) -> Option<(usize, String, i32, i32)> {
    let mut parts = rest.splitn(4, ':');
    Some((
        parts.next()?.parse::<usize>().ok()?,
        parts.next()?.to_string(),
        parts.next()?.parse::<i32>().ok()?,
        parts.next()?.parse::<i32>().ok()?,
    ))
}

fn parse_amount_arg(amount_str: &str, max_available: i64) -> i64 {
    if amount_str == "max" {
        max_available
    } else {
        amount_str
            .parse::<i64>()
            .ok()
            .map(|amount| amount.min(max_available))
            .unwrap_or(0)
    }
}

fn with_updated_pack(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    bx: i32,
    by: i32,
    action: &str,
    mutate: impl FnOnce(&mut PackData),
    on_success: impl FnOnce(&PackData),
) -> bool {
    match update_pack_with_db(state, bx, by, mutate) {
        Ok(updated_pack) => {
            on_success(&updated_pack);
            true
        }
        Err(err) => {
            tracing::error!("Failed to {action} pack at ({}, {}): {err}", bx, by);
            let user_error = if err == "Объект не найден" {
                "Объект не найден"
            } else {
                "Не удалось обновить здание"
            };
            send_u_packet(tx, "OK", &ok_message("Ошибка", user_error).1);
            false
        }
    }
}

fn refresh_pack_gui(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    x: i32,
    y: i32,
) {
    if let Some(pack) = state.get_pack_at(x, y) {
        let pack = pack.clone();
        let json = build_pack_gui(state, pid, &pack);
        send_u_packet(tx, "GU", json.as_bytes());
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(0))
        .unwrap_or(0)
}

fn send_player_inventory(
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    inv: &std::collections::HashMap<i32, i32>,
    selected: i32,
) {
    let items: Vec<(i32, i32)> = inv
        .iter()
        .filter(|(_, v)| **v > 0)
        .map(|(k, v)| (*k, *v))
        .collect();
    let total = i32::try_from(items.len()).unwrap_or(i32::MAX);
    send_u_packet(tx, "IN", &inventory_show(&items, selected, total).1);
}

fn handle_craft_start(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    recipe_id: i32,
    num: i32,
    bx: i32,
    by: i32,
) {
    if !validate_pack_interaction(state, tx, pid, bx, by, PackType::Craft, &[]) {
        return;
    }
    // Крафтер уже занят — сначала забрать текущий крафт.
    if let Some(pack) = state.get_pack_at(bx, by) {
        if pack.craft_recipe_id.is_some() {
            send_u_packet(
                tx,
                "OK",
                &ok_message("Крафтер", "Уже идёт крафт. Сначала заберите результат").1,
            );
            return;
        }
    }
    let Some(recipe) = recipe_by_id(recipe_id) else {
        send_u_packet(tx, "OK", &ok_message("Ошибка", "Неизвестный рецепт").1);
        return;
    };
    // Проверка ресурсов у игрока.
    {
        let Some(p) = state.active_players.get(&pid) else {
            return;
        };
        for c in recipe.cost_crys {
            let idx = usize::try_from(c.id).unwrap_or(0).min(5);
            let need = i64::from(c.num) * i64::from(num);
            if p.data.crystals[idx] < need {
                send_u_packet(
                    tx,
                    "OK",
                    &ok_message("Крафтер", "Недостаточно кристаллов").1,
                );
                return;
            }
        }
        for c in recipe.cost_res {
            let have = p.data.inventory.get(&c.id).copied().unwrap_or(0);
            if have < c.num * num {
                send_u_packet(tx, "OK", &ok_message("Крафтер", "Недостаточно ресурсов").1);
                return;
            }
        }
    }
    // Списываем ресурсы, шлём обновления.
    {
        let Some(mut p) = state.active_players.get_mut(&pid) else {
            return;
        };
        for c in recipe.cost_crys {
            let idx = usize::try_from(c.id).unwrap_or(0).min(5);
            p.data.crystals[idx] -= i64::from(c.num) * i64::from(num);
        }
        for c in recipe.cost_res {
            let have = p.data.inventory.get(&c.id).copied().unwrap_or(0);
            p.data.inventory.insert(c.id, have - c.num * num);
        }
        let crys = p.data.crystals;
        send_u_packet(tx, "@B", &basket(&crys, 1000).1);
        let inv = p.data.inventory.clone();
        let sel = p.inv_selected;
        send_player_inventory(tx, &inv, sel);
    }
    // Обновляем pack.
    let end_ts = now_unix() + i64::from(recipe.time_sec) * i64::from(num);
    let _ = with_updated_pack(
        state,
        tx,
        bx,
        by,
        "запуск крафта",
        |pack| {
            pack.craft_recipe_id = Some(recipe_id);
            pack.craft_num = num;
            pack.craft_end_ts = end_ts;
        },
        |updated_pack| {
            broadcast_pack_update(state, updated_pack);
        },
    );
    refresh_pack_gui(state, tx, pid, bx, by);
}

fn handle_craft_claim(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    bx: i32,
    by: i32,
) {
    if !validate_pack_interaction(state, tx, pid, bx, by, PackType::Craft, &[]) {
        return;
    }
    let (recipe_id, num, end_ts) = {
        let Some(pack) = state.get_pack_at(bx, by) else {
            return;
        };
        (pack.craft_recipe_id, pack.craft_num, pack.craft_end_ts)
    };
    let Some(recipe_id) = recipe_id else {
        send_u_packet(tx, "OK", &ok_message("Крафтер", "Нечего забирать").1);
        return;
    };
    if now_unix() < end_ts {
        send_u_packet(tx, "OK", &ok_message("Крафтер", "Ещё не готово").1);
        return;
    }
    let Some(recipe) = recipe_by_id(recipe_id) else {
        return;
    };
    let total_items = recipe.result.num * num;
    {
        let Some(mut p) = state.active_players.get_mut(&pid) else {
            return;
        };
        let have = p
            .data
            .inventory
            .get(&recipe.result.id)
            .copied()
            .unwrap_or(0);
        p.data
            .inventory
            .insert(recipe.result.id, have + total_items);
        let inv = p.data.inventory.clone();
        let sel = p.inv_selected;
        send_player_inventory(tx, &inv, sel);
    }
    let _ = with_updated_pack(
        state,
        tx,
        bx,
        by,
        "завершение крафта",
        |pack| {
            pack.craft_recipe_id = None;
            pack.craft_num = 0;
            pack.craft_end_ts = 0;
        },
        |updated_pack| {
            broadcast_pack_update(state, updated_pack);
        },
    );
    refresh_pack_gui(state, tx, pid, bx, by);
}
