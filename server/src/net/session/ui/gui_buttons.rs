//! Обработка нажатий GUI-кнопок игроком.
use crate::game::buildings::{
    BuildingCrafting, BuildingFlags, BuildingMetadata, BuildingOwnership, BuildingStats,
    BuildingStorage, GridPosition,
};
use crate::game::crafting;
use crate::game::market;
use crate::game::player::{PlayerFlags, PlayerInventory, PlayerPosition, PlayerStats, PlayerUI};
use crate::net::session::outbound::inventory_sync::send_inventory;
use crate::net::session::prelude::*;
use crate::net::session::social::buildings::{broadcast_pack_update, modify_pack_with_db};

fn parse_six_i64_fields(data: &str) -> Option<[i64; 6]> {
    let mut out = [0_i64; 6];
    let mut parts = data.split(':');
    for slot in &mut out {
        let part = parts.next()?;
        if part.is_empty() {
            return None;
        }
        *slot = part.parse().ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(out)
}

fn parse_rich_key_values(data: &str) -> Option<std::collections::HashMap<&str, &str>> {
    let mut fields = std::collections::HashMap::new();
    if data.is_empty() {
        return Some(fields);
    }
    for pair in data.split('#') {
        let (key, value) = pair.split_once(':')?;
        if key.is_empty() || value.is_empty() {
            return None;
        }
        fields.insert(key, value);
    }
    Some(fields)
}

fn parse_settings_pairs(data: &str) -> Option<std::collections::HashMap<&str, &str>> {
    let mut fields = std::collections::HashMap::new();
    let trimmed = data.strip_suffix('#').unwrap_or(data);
    if trimmed.is_empty() {
        return None;
    }
    for pair in trimmed.split('#') {
        let (key, value) = pair.split_once(':')?;
        if key.is_empty() || value.is_empty() {
            return None;
        }
        fields.insert(key, value);
    }
    Some(fields)
}

fn parse_rich_bool(value: &str) -> Option<bool> {
    match value {
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
        _ => None,
    }
}

pub async fn handle_gui_button(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    button: &str,
) {
    // ref `Session.GUI`: `"exit"` or `"exit:0"` => CloseWindow()
    if button == "exit" || button == "exit:0" || button == "close" {
        state.modify_player(pid, |ecs, entity| {
            if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) {
                ui.current_window = None;
            }
            // ref `CloseWindow`: сброс выбранного слота (у нас это `inventory.selected`).
            if let Some(mut inv) = ecs.get_mut::<PlayerInventory>(entity) {
                inv.selected = -1;
            }
            Some(())
        });
        let g = gu_close();
        send_u_packet(tx, g.0, &g.1);
        return;
    }

    // C# ref: CallWinAction — if win is null, send Gu close and return.
    let has_window = state
        .query_player_opt(pid, |ecs, entity| {
            ecs.get::<PlayerUI>(entity)
                .map(|ui| ui.current_window.is_some())
        })
        .unwrap_or(false);
    if !has_window {
        let g = gu_close();
        send_u_packet(tx, g.0, &g.1);
        return;
    }

    if let Some(rest) = button.strip_prefix("clan_view:") {
        if let Ok(id) = rest.parse::<i32>() {
            crate::net::session::social::clans::handle_clan_preview(state, tx, pid, id).await;
        }
        return;
    }

    match button {
        "open_buildings" => {
            crate::net::session::social::buildings::handle_buildings_menu(state, tx, pid);
        }
        "createprog" => open_create_prog_dialog(state, tx, pid),
        "prog" => {
            crate::net::session::social::buildings::handle_programmator_pope_menu(state, tx, pid)
                .await;
        }
        "clan_menu" | "clan_back" => {
            crate::net::session::social::clans::handle_clan_menu(state, tx, pid).await;
        }
        "clan_create_view" => handle_clan_create_view(state, tx, pid),
        "clan_requests" => {
            crate::net::session::social::clans::handle_clan_requests_view(state, tx, pid).await;
        }
        "clan_members" => {
            crate::net::session::social::clans::handle_clan_members_view(state, tx, pid).await;
        }
        "clan_invite_list" => {
            crate::net::session::social::clans::handle_clan_invite_list(state, tx, pid).await;
        }
        "clan_invites_view" => {
            crate::net::session::social::clans::handle_clan_invites_view(state, tx, pid).await;
        }
        "clan_leave" => crate::net::session::social::clans::handle_clan_leave(state, tx, pid).await,
        // Market tab switching (C# tabs have action strings)
        "sellcrys" => handle_market_tab_switch(state, tx, pid, "sellcrys").await,
        "buycrys" => handle_market_tab_switch(state, tx, pid, "buycrys").await,
        "auc" => handle_market_tab_switch(state, tx, pid, "auc").await,
        "sellall" => handle_market_sellall(state, tx, pid),
        "getprofit" => handle_market_getprofit(state, tx, pid),
        "clancreate" | "clan_create" => {
            handle_clan_create_view(state, tx, pid);
        }
        "clan_create_input" => {
            crate::net::session::social::commands::send_ok(
                tx,
                "КЛАНЫ",
                "Введите /clan create НАЗВАНИЕ ТЕГ в чате",
            );
        }
        _ => handle_complex_button(state, tx, pid, button).await,
    }

    // C# ref: after CallWinAction, SendWindow() re-sends the window or closes if null.
    // Safety net: if no handler sent a response and window was cleared, send Gu close.
    let still_has_window = state
        .query_player_opt(pid, |ecs, entity| {
            ecs.get::<PlayerUI>(entity)
                .map(|ui| ui.current_window.is_some())
        })
        .unwrap_or(false);
    if !still_has_window {
        let g = gu_close();
        send_u_packet(tx, g.0, &g.1);
    }
}

fn handle_clan_create_view(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    use super::horb::{Button, Horb};
    // exit добавится builder-гарантией последним → Escape закроет окно.
    Horb::new("СОЗДАНИЕ КЛАНА")
        .text("Введите название и тег (3 симв.) через пробел в чат после нажатия кнопки 'ВВОД'")
        .button(Button::new("ВВОД", "clan_create_input"))
        .button(Button::new("Назад", "clan_back"))
        .send(state, tx, pid, "clan");
}

/// Закрыть текущее GUI-окно игрока (сбросить `current_window` + `Gu`).
fn close_player_window(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId) {
    state.modify_player(pid, |ecs, e| {
        if let Some(mut ui) = ecs.get_mut::<PlayerUI>(e) {
            ui.current_window = None;
        }
        Some(())
    });
    let g = gu_close();
    send_u_packet(tx, g.0, &g.1);
}

async fn handle_complex_button(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    button: &str,
) {
    if let Some(rest) = button.strip_prefix("clan_request:") {
        if let Ok(id) = rest.parse::<i32>() {
            crate::net::session::social::clans::handle_clan_join_request(state, tx, pid, id).await;
        }
    } else if let Some(rest) = button.strip_prefix("clan_accept:") {
        if let Ok(id) = rest.parse::<i32>() {
            crate::net::session::social::clans::handle_clan_accept(state, tx, pid, id).await;
        }
    } else if let Some(rest) = button.strip_prefix("clan_invite_send:") {
        if let Ok(id) = rest.parse::<i32>() {
            crate::net::session::social::clans::handle_clan_invite_send(state, tx, pid, id).await;
        }
    } else if let Some(rest) = button.strip_prefix("clan_invite_accept:") {
        if let Ok(id) = rest.parse::<i32>() {
            crate::net::session::social::clans::handle_clan_invite_accept(state, tx, pid, id).await;
        }
    } else if let Some(rest) = button.strip_prefix("clan_promote:") {
        if let Ok(id) = rest.parse::<i32>() {
            crate::net::session::social::clans::handle_clan_promote(state, tx, pid, id).await;
        }
    } else if let Some(rest) = button.strip_prefix("clan_kick_id:") {
        if let Ok(id) = rest.parse::<i32>() {
            crate::net::session::social::clans::handle_clan_kick(state, tx, pid, id).await;
        }
    } else if let Some(rest) = button.strip_prefix("clan_decline:") {
        if let Ok(id) = rest.parse::<i32>() {
            crate::net::session::social::clans::handle_clan_decline(state, tx, pid, id).await;
        }
    } else if let Some(rest) = button.strip_prefix("clan_invite_decline:") {
        if let Ok(id) = rest.parse::<i32>() {
            crate::net::session::social::clans::handle_clan_invite_decline(state, tx, pid, id)
                .await;
        }
    } else if let Some(rest) = button.strip_prefix("bld_place:") {
        crate::net::session::social::buildings::handle_place_building(state, tx, pid, rest).await;
    } else if let Some(rest) = button.strip_prefix("pack_op:") {
        handle_pack_operation(state, tx, pid, rest).await;
    } else if let Some(rest) = button.strip_prefix("transfer:") {
        handle_storage_transfer(state, tx, pid, rest);
    } else if let Some(rest) = button.strip_prefix("craft_recipe:") {
        handle_craft_recipe_view(state, tx, pid, rest);
    } else if let Some(rest) = button.strip_prefix("craft_start:") {
        handle_craft_start(state, tx, pid, rest);
    } else if let Some(rest) = button.strip_prefix("craft_claim:") {
        handle_craft_claim(state, tx, pid, rest);
    } else if let Some(rest) = button.strip_prefix("tp:") {
        handle_teleport_action(state, tx, pid, rest);
    } else if let Some(rest) = button.strip_prefix("resp_bind:") {
        let parts: Vec<&str> = rest.split(':').collect();
        if parts.len() == 2 {
            if let (Ok(x), Ok(y)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>()) {
                crate::net::session::play::packs::handle_resp_bind(state, tx, pid, x, y);
            }
        }
    } else if let Some(rest) = button.strip_prefix("resp_fill:") {
        let parts: Vec<&str> = rest.split(':').collect();
        if parts.len() == 3 {
            if let (Ok(x), Ok(y)) = (parts[1].parse::<i32>(), parts[2].parse::<i32>()) {
                crate::net::session::play::packs::handle_resp_fill(state, tx, pid, parts[0], x, y);
            }
        }
    } else if let Some(rest) = button.strip_prefix("gun_fill:") {
        let parts: Vec<&str> = rest.split(':').collect();
        if parts.len() == 3 {
            if let (Ok(x), Ok(y)) = (parts[1].parse::<i32>(), parts[2].parse::<i32>()) {
                crate::net::session::play::packs::handle_gun_fill(state, tx, pid, parts[0], x, y);
            }
        }
    } else if let Some(rest) = button.strip_prefix("resp_profit:") {
        let parts: Vec<&str> = rest.split(':').collect();
        if parts.len() == 2 {
            if let (Ok(x), Ok(y)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>()) {
                crate::net::session::play::packs::handle_resp_profit(state, tx, pid, x, y);
            }
        }
    } else if let Some(rest) = button.strip_prefix("resp_save:") {
        // RichList data, coordinates resolved from current_window
        crate::net::session::play::packs::handle_resp_save(state, tx, pid, rest);
    } else if let Some(rest) = button.strip_prefix("pack_save:") {
        // Единая админ-панель пака: сохранить cost/clan из %R%.
        handle_pack_save(state, tx, pid, rest);
    } else if let Some(rest) = button.strip_prefix("sell:") {
        handle_market_sell(state, tx, pid, rest);
    } else if let Some(rest) = button.strip_prefix("buy:") {
        handle_market_buy(state, tx, pid, rest);
    } else if let Some(rest) = button.strip_prefix("save:") {
        handle_settings_save(state, tx, pid, rest);
    } else if let Some(rest) = button.strip_prefix("choose:") {
        // Клик item-грида аукциона (клиент хардкодит InvButton="choose").
        match rest.parse::<i32>() {
            Ok(item) => super::auction_gui::open_item_auc(state, tx, pid, item).await,
            Err(e) => {
                tracing::warn!(player_id = %pid, action = button, error = ?e, "Invalid auction choose action");
                send_market_action_error(tx);
            }
        }
    } else if let Some(rest) = button.strip_prefix("openorder:") {
        match rest.parse::<i32>() {
            Ok(id) => super::auction_gui::open_order(state, tx, pid, id).await,
            Err(e) => {
                tracing::warn!(player_id = %pid, action = button, error = ?e, "Invalid auction openorder action");
                send_market_action_error(tx);
            }
        }
    } else if let Some(rest) = button.strip_prefix("auccreate:") {
        match rest.parse::<i32>() {
            Ok(item) => super::auction_gui::open_order_creation(state, tx, pid, item),
            Err(e) => {
                tracing::warn!(player_id = %pid, action = button, error = ?e, "Invalid auction create action");
                send_market_action_error(tx);
            }
        }
    } else if let Some(rest) = button.strip_prefix("aucsetcost:") {
        // aucsetcost:{item}:{cost}; невалидный cost → закрыть окно (1:1 C#).
        let parts: Vec<&str> = rest.splitn(2, ':').collect();
        if let [item, cost] = parts.as_slice() {
            match (item.parse::<i32>(), cost.parse::<i64>()) {
                (Ok(item), Ok(cost)) => {
                    super::auction_gui::open_order_creation_num(state, tx, pid, item, cost);
                }
                _ => close_player_window(state, tx, pid),
            }
        } else {
            tracing::warn!(player_id = %pid, action = button, "Invalid auction setcost action");
            send_market_action_error(tx);
        }
    } else if let Some(rest) = button.strip_prefix("aucsetnum:") {
        // aucsetnum:{item}:{cost}:{num}; невалидный num → закрыть окно (1:1 C#).
        let parts: Vec<&str> = rest.split(':').collect();
        if let [item, cost, num] = parts.as_slice() {
            match (item.parse::<i32>(), cost.parse::<i64>(), num.parse::<i32>()) {
                (Ok(item), Ok(cost), Ok(num)) => {
                    super::auction_gui::create_order(state, tx, pid, item, num, cost).await;
                }
                _ => close_player_window(state, tx, pid),
            }
        } else {
            tracing::warn!(player_id = %pid, action = button, "Invalid auction setnum action");
            send_market_action_error(tx);
        }
    } else if let Some(rest) = button.strip_prefix("aucminbet:") {
        match rest.parse::<i32>() {
            Ok(id) => super::auction_gui::place_minimal_bet(state, tx, pid, id).await,
            Err(e) => {
                tracing::warn!(player_id = %pid, action = button, error = ?e, "Invalid auction minbet action");
                send_market_action_error(tx);
            }
        }
    } else if let Some(rest) = button.strip_prefix("aucbet:") {
        // aucbet:{id}:{amount}; невалидная сумма → просто переоткрыть ордер (1:1 C#).
        let parts: Vec<&str> = rest.splitn(2, ':').collect();
        if let [id, amount] = parts.as_slice() {
            match (id.parse::<i32>(), amount.parse::<i64>()) {
                (Ok(id), Ok(amount)) => {
                    super::auction_gui::place_bet(state, tx, pid, id, amount).await;
                }
                (Ok(id), Err(_)) => super::auction_gui::open_order(state, tx, pid, id).await,
                (Err(e), _) => {
                    tracing::warn!(player_id = %pid, action = button, error = ?e, "Invalid auction bet action");
                    send_market_action_error(tx);
                }
            }
        } else {
            tracing::warn!(player_id = %pid, action = button, "Invalid auction bet action");
            send_market_action_error(tx);
        }
    } else if let Some(rest) = button.strip_prefix("openprog:") {
        if let Ok(id) = rest.parse::<i32>() {
            handle_open_prog(state, tx, pid, id).await;
        }
    } else if let Some(name) = button.strip_prefix("createprog:") {
        handle_create_prog(state, tx, pid, name).await;
    } else if let Some(rest) = button.strip_prefix("rename:") {
        // format: "<id>:<name>" (сервер кодирует как `rename:{id}:%I%`, клиент подставляет ввод)
        let parts: Vec<&str> = rest.splitn(2, ':').collect();
        if let [id_str, name] = parts.as_slice() {
            if let Ok(id) = id_str.parse::<i32>() {
                handle_rename_prog(state, tx, pid, id, name).await;
            }
        }
    } else {
        // Up building buttons (skill:N, upgrade, delete:N, install:code#N, buyslot)
        super::up_building::handle_up_button(state, tx, pid, button);
    }
}

async fn handle_pack_operation(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    op: &str,
) {
    let parts: Vec<&str> = op.split(':').collect();
    if parts.len() < 3 {
        send_pack_action_error(tx);
        return;
    }
    let cmd = parts[0];
    let (x, y) = match (parts[1].parse::<i32>(), parts[2].parse::<i32>()) {
        (Ok(x), Ok(y)) => (x, y),
        (Err(e), _) | (_, Err(e)) => {
            tracing::warn!(player_id = %pid, action = op, error = ?e, "Invalid pack operation coordinates");
            send_pack_action_error(tx);
            return;
        }
    };

    let Some(view) = state.get_pack_at(x, y) else {
        return;
    };

    let p_info = state.query_player_opt(pid, |ecs, entity| {
        let pos = ecs.get::<PlayerPosition>(entity)?;
        let pstats = ecs.get::<PlayerStats>(entity)?;
        Some((pos.x, pos.y, pstats.clan_id.unwrap_or(0)))
    });

    let Some((px, py, p_clan)) = p_info else {
        return;
    };

    // Market allows anyone standing on it to buy/sell (like Resp).
    // Only admin operations require ownership.
    if view.pack_type == PackType::Market && cmd == "open" {
        // Only proximity check for Market open
        let Ok(cells) = view.pack_type.building_cells() else {
            tracing::error!(pack_type = ?view.pack_type, "Missing building config for pack GUI");
            return;
        };
        if !cells
            .iter()
            .any(|(dx, dy, _)| view.x + dx == px && view.y + dy == py)
        {
            return;
        }
    } else if validate_pack_access(&view, (px, py), p_clan, pid).is_err() {
        return;
    }

    match cmd {
        "open" => open_pack_gui(state, tx, pid, &view),
        "take_money" => handle_pack_take_money(state, tx, pid, &view),
        "take_crys" => handle_pack_take_crystals(state, tx, pid, &view),
        "remove" => {
            crate::net::session::social::buildings::handle_remove_building(state, tx, pid, x, y)
                .await;
        }
        _ => {}
    }
}

pub fn open_pack_gui(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    view: &PackView,
) {
    // C# ref: Gate.GUIWin() returns null — no window opens
    if view.pack_type == PackType::Gate {
        close_player_window(state, tx, pid);
        return;
    }
    if view.pack_type == PackType::Storage {
        open_storage_gui(state, tx, pid, view);
        return;
    }
    if view.pack_type == PackType::Teleport {
        open_teleport_gui(state, tx, pid, view);
        return;
    }
    if view.pack_type == PackType::Craft {
        open_crafter_gui(state, tx, pid, view);
        return;
    }
    if view.pack_type == PackType::Market {
        open_market_gui(state, tx, pid, view, "sellcrys");
        return;
    }
    if view.pack_type == PackType::Spot {
        open_spot_gui(state, tx, pid, view);
        return;
    }
    if view.pack_type == PackType::Up {
        super::up_building::open_up_gui(state, tx, pid, view);
        return;
    }
    if view.pack_type == PackType::Resp {
        // Респ: визитёрский GUI с кнопкой «ПРИВЯЗАТЬ» (1:1 C# `Resp.GUIWin`).
        // Без этой ветки респ падал в generic GUI без bind → «невозможно
        // привязаться» (репорт). `handle_pack_action` (был dead code) этот тип
        // обрабатывал, но реальный путь открытия — `open_pack_gui`.
        crate::net::session::play::packs::open_resp_gui(state, tx, pid, view);
        return;
    }
    if view.pack_type == PackType::Gun {
        crate::net::session::play::packs::open_gun_gui(state, tx, pid, view.x, view.y);
        return;
    }
    if view.pack_type == PackType::Clans {
        // Пак кланс открывает меню кланов (вкладка топов и пр.). Меню асинхронное (DB),
        // а `open_pack_gui` синхронный (зовётся в т.ч. из sync `handle_move`) — спавним
        // таску; `state`/`tx` клонируемы, `pid` Copy (идиома как в heal_inventory.rs).
        let (st, txc) = (state.clone(), tx.clone());
        tokio::spawn(async move {
            crate::net::session::social::clans::handle_clan_menu(&st, &txc, pid).await;
        });
        return;
    }

    let title = view.pack_type.name();

    // Fetch detailed pstats from ECS for GUI
    let pstats_info = state
        .building_index
        .get(&((view.x, view.y).into()))
        .and_then(|ent| {
            let ecs = state.ecs.read();
            let pstats = ecs.get::<BuildingStats>(*ent)?;
            Some((pstats.hp, pstats.max_hp))
        });
    let Some((hp, mhp)) = pstats_info else {
        tracing::error!(
            x = view.x,
            y = view.y,
            "Building stats missing for pack GUI"
        );
        send_pack_action_error(tx);
        return;
    };

    let text = format!(
        "Здание: {}\nЗаряд: {:.1}\nПрочность: {}/{}",
        title, view.charge, hp, mhp
    );
    use super::horb::{Button, Horb};
    Horb::new(title)
        .text(text)
        .button(Button::new(
            "Забрать деньги",
            format!("pack_op:take_money:{}:{}", view.x, view.y),
        ))
        .button(Button::new(
            "Забрать кристаллы",
            format!("pack_op:take_crys:{}:{}", view.x, view.y),
        ))
        .button(Button::new(
            "Удалить",
            format!("pack_op:remove:{}:{}", view.x, view.y),
        ))
        .admin(view.owner_id == pid) // шестерёнка → open_pack_admin_gui
        .close_button()
        .send(state, tx, pid, format!("pack:{}:{}", view.x, view.y));
}

/// Единая админ-панель пака (шестерёнка): прочность/заряд/стоимость/закланить/
/// прибыль. Открывается по `ADMN` на окне `pack:{x}:{y}`. Сохранение — `pack_save`.
pub fn open_pack_admin_gui(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    pack_x: i32,
    pack_y: i32,
) {
    use super::horb::{Button, Horb, RichRow};
    let Some(view) = state.get_pack_at(pack_x, pack_y) else {
        return;
    };
    if view.owner_id != pid {
        return;
    }
    let details = state
        .building_index
        .get(&((pack_x, pack_y).into()))
        .and_then(|ent| {
            let ecs = state.ecs.read();
            let st = ecs.get::<BuildingStats>(*ent)?;
            let storage = ecs.get::<BuildingStorage>(*ent)?;
            let own = ecs.get::<BuildingOwnership>(*ent)?;
            Some((
                st.hp,
                st.max_hp,
                st.charge,
                st.max_charge,
                st.cost,
                storage.money,
                own.clan_id,
            ))
        });
    let Some((hp, max_hp, charge, max_charge, cost, money, clan_id)) = details else {
        return;
    };

    let (profit_btn, profit_act) = if money > 0 {
        (
            "Получить".to_string(),
            format!("pack_op:take_money:{pack_x}:{pack_y}"),
        )
    } else {
        (String::new(), String::new())
    };

    Horb::new("Управление")
        .rich_row(RichRow::text(format!("Прочность: {hp}/{max_hp}")))
        .rich_row(RichRow::text(format!("Заряд: {charge:.0}/{max_charge:.0}")))
        .rich_row(RichRow::uint("Стоимость", "cost", i64::from(cost)))
        .rich_row(RichRow::toggle("Закланить", "clan", clan_id != 0))
        .rich_row(RichRow::button(
            format!("Прибыль: {money}$"),
            profit_btn,
            profit_act,
        ))
        .button(Button::new("Сохранить", "pack_save:%R%"))
        .send(state, tx, pid, format!("pack:{pack_x}:{pack_y}"));
}

/// `pack_save:{key:value#…}` из админ-панели (`%R%`). Ставит cost/clan,
/// перерисовывает панель. Зеркало `handle_resp_save`, но для окна `pack:`.
pub fn handle_pack_save(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    richlist_data: &str,
) {
    let coords = state.query_player_opt(pid, |ecs, entity| {
        let ui = ecs.get::<PlayerUI>(entity)?;
        let rest = ui.current_window.as_deref()?.strip_prefix("pack:")?;
        let parts: Vec<&str> = rest.split(':').collect();
        if parts.len() == 2 {
            Some((parts[0].parse::<i32>().ok()?, parts[1].parse::<i32>().ok()?))
        } else {
            None
        }
    });
    let Some((pack_x, pack_y)) = coords else {
        return;
    };
    let Some(view) = state.get_pack_at(pack_x, pack_y) else {
        return;
    };
    if view.owner_id != pid {
        return;
    }

    let Some(fields) = parse_rich_key_values(richlist_data) else {
        send_pack_action_error(tx);
        return;
    };
    let cost = match fields.get("cost") {
        Some(raw) => match raw.parse::<i32>() {
            Ok(cost) if (0..=5000).contains(&cost) => Some(cost),
            _ => {
                send_pack_action_error(tx);
                return;
            }
        },
        None => None,
    };
    let clan_enabled = match fields.get("clan") {
        Some(raw) => match parse_rich_bool(raw) {
            Some(value) => Some(value),
            None => {
                send_pack_action_error(tx);
                return;
            }
        },
        None => None,
    };
    let owner_clan = state
        .query_player_opt(pid, |ecs, e| {
            ecs.get::<PlayerStats>(e).and_then(|s| s.clan_id)
        })
        .unwrap_or(0);

    let updated = match modify_pack_with_db(state, pack_x, pack_y, |ecs, entity| {
        let mut updated = false;
        if let Some(mut st) = ecs.get_mut::<BuildingStats>(entity) {
            if let Some(cost) = cost {
                st.cost = cost;
                updated = true;
            }
        } else if cost.is_some() {
            return false;
        }
        if let Some(mut own) = ecs.get_mut::<BuildingOwnership>(entity) {
            if let Some(clan_enabled) = clan_enabled {
                own.clan_id = if clan_enabled { owner_clan } else { 0 };
                updated = true;
            }
        } else if clan_enabled.is_some() {
            return false;
        }
        updated
    }) {
        Ok(updated) => updated,
        Err(e) => {
            tracing::error!(x = pack_x, y = pack_y, error = %e, "Pack save failed");
            false
        }
    };
    if !updated {
        send_pack_action_error(tx);
        return;
    }

    open_pack_admin_gui(state, tx, pid, pack_x, pack_y);
}

fn handle_pack_take_money(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    view: &PackView,
) {
    if !pack_withdraw_state_ready(state, pid, view.x, view.y) {
        send_pack_state_error(tx);
        return;
    }

    let mut amount = 0i64;
    let updated = match modify_pack_with_db(state, view.x, view.y, |ecs, entity| {
        let mut storage = ecs
            .get_mut::<BuildingStorage>(entity)
            .expect("BuildingStorage checked before pack money withdrawal");
        amount = storage.money;
        storage.money = 0;
        true
    }) {
        Ok(updated) => updated,
        Err(e) => {
            tracing::error!(x = view.x, y = view.y, error = %e, "Pack money withdrawal failed");
            send_pack_state_error(tx);
            return;
        }
    };
    if !updated {
        send_pack_action_error(tx);
        return;
    }

    if amount > 0 {
        state.modify_player(pid, |ecs, entity| {
            // B2: пометить dirty (см. do_market_sell) — pack take тоже мутирует деньги.
            let (money_now, creds_now) = {
                let mut s = ecs
                    .get_mut::<PlayerStats>(entity)
                    .expect("PlayerStats checked before pack money withdrawal");
                s.money += amount;
                (s.money, s.creds)
            };
            let mut f = ecs
                .get_mut::<PlayerFlags>(entity)
                .expect("PlayerFlags checked before pack money withdrawal");
            f.dirty = true;
            send_u_packet(tx, "P$", &money(money_now, creds_now).1);
            Some(())
        });
    }
}

fn handle_pack_take_crystals(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    view: &PackView,
) {
    if !pack_withdraw_state_ready(state, pid, view.x, view.y) {
        send_pack_state_error(tx);
        return;
    }

    let mut amount = [0i64; 6];
    let updated = match modify_pack_with_db(state, view.x, view.y, |ecs, entity| {
        let mut storage = ecs
            .get_mut::<BuildingStorage>(entity)
            .expect("BuildingStorage checked before pack crystal withdrawal");
        amount = storage.crystals;
        storage.crystals = [0; 6];
        true
    }) {
        Ok(updated) => updated,
        Err(e) => {
            tracing::error!(x = view.x, y = view.y, error = %e, "Pack crystal withdrawal failed");
            send_pack_state_error(tx);
            return;
        }
    };
    if !updated {
        send_pack_action_error(tx);
        return;
    }

    if amount.iter().sum::<i64>() > 0 {
        state.modify_player(pid, |ecs, entity| {
            // B2: пометить dirty (см. do_market_sell) — pack take кристаллов.
            let crystals_now = {
                let mut s = ecs
                    .get_mut::<PlayerStats>(entity)
                    .expect("PlayerStats checked before pack crystal withdrawal");
                for i in 0..6 {
                    s.crystals[i] += amount[i];
                }
                s.crystals
            };
            let mut f = ecs
                .get_mut::<PlayerFlags>(entity)
                .expect("PlayerFlags checked before pack crystal withdrawal");
            f.dirty = true;
            send_u_packet(tx, "@B", &basket(&crystals_now, 1).1);
            Some(())
        });
    }
}

fn pack_withdraw_state_ready(state: &Arc<GameState>, pid: PlayerId, x: i32, y: i32) -> bool {
    let player_ready = state
        .query_player(pid, |ecs, entity| {
            ecs.get::<PlayerStats>(entity).is_some() && ecs.get::<PlayerFlags>(entity).is_some()
        })
        .unwrap_or(false);
    let building_ready = state
        .building_index
        .get(&((x, y).into()))
        .map(|ent| {
            let ecs = state.ecs.read();
            ecs.get::<BuildingStorage>(*ent).is_some() && ecs.get::<BuildingFlags>(*ent).is_some()
        })
        .unwrap_or(false);
    player_ready && building_ready
}

/// Open Storage-specific GUI with crystal sliders (1:1 with C# `Storage.MainPage`).
fn open_storage_gui(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    view: &PackView,
) {
    // Fetch storage crystals from ECS
    let storage_crys = state
        .building_index
        .get(&((view.x, view.y).into()))
        .and_then(|ent| {
            let ecs = state.ecs.read();
            let s = ecs.get::<BuildingStorage>(*ent)?;
            Some(s.crystals)
        });
    let Some(storage_crys) = storage_crys else {
        tracing::error!(
            x = view.x,
            y = view.y,
            "Building storage missing for storage GUI"
        );
        send_pack_action_error(tx);
        return;
    };

    // Fetch player crystals
    let player_crys = state.query_player_opt(pid, |ecs, entity| {
        ecs.get::<PlayerStats>(entity).map(|s| s.crystals)
    });
    let Some(player_crys) = player_crys else {
        tracing::error!(player_id = %pid, "Player stats missing for storage GUI");
        send_pack_action_error(tx);
        return;
    };

    // Build crys_lines: each line is "LeftMin:RightMin:Denominator:CurrentValue:Label"
    // C# ref: CrysLine("", 0, 0, p.crys.cry[id] + cry, (int)(cry))
    // Serialized as: "{LeftMin}:{RightMin}:{Denominator}:{CurrentValue}:{Label}"
    let crys_lines: Vec<String> = (0..6)
        .map(|i| {
            let denominator = player_crys[i] + storage_crys[i];
            let current_value = storage_crys[i];
            format!("0:0:{denominator}:{current_value}:")
        })
        .collect();

    use super::horb::{Button, Horb};
    Horb::new("Склад")
        .crystals(" ", " ", false, crys_lines)
        .button(Button::new("Передать", "transfer:%M%"))
        .button(Button::new(
            "Удалить",
            format!("pack_op:remove:{}:{}", view.x, view.y),
        ))
        .close_button()
        .send(state, tx, pid, format!("pack:{}:{}", view.x, view.y));
}

/// Handle `transfer:v0:v1:v2:v3:v4:v5` button from Storage GUI sliders.
/// `sliders[i]` = desired amount to keep IN the storage.
/// C# ref: `Storage.StockTransfer`.
fn handle_storage_transfer(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    slider_data: &str,
) {
    let Some(sliders) = parse_six_i64_fields(slider_data) else {
        return;
    };

    // Resolve storage coordinates from current_window ("pack:{x}:{y}")
    let coords = state.query_player_opt(pid, |ecs, entity| {
        let ui = ecs.get::<PlayerUI>(entity)?;
        let window = ui.current_window.as_deref()?;
        let parts: Vec<&str> = window.strip_prefix("pack:")?.split(':').collect();
        if parts.len() == 2 {
            Some((parts[0].parse::<i32>().ok()?, parts[1].parse::<i32>().ok()?))
        } else {
            None
        }
    });

    let Some((bx, by)) = coords else {
        return;
    };

    // Verify it's actually a Storage building
    let Some(view) = state.get_pack_at(bx, by) else {
        return;
    };
    if view.pack_type != PackType::Storage {
        return;
    }

    // Validate player access
    let p_info = state.query_player_opt(pid, |ecs, entity| {
        let pos = ecs.get::<PlayerPosition>(entity)?;
        let pstats = ecs.get::<PlayerStats>(entity)?;
        Some((pos.x, pos.y, pstats.clan_id.unwrap_or(0)))
    });
    let Some((px, py, p_clan)) = p_info else {
        return;
    };
    if validate_pack_access(&view, (px, py), p_clan, pid).is_err() {
        return;
    }

    // Pre-fetch player entity before acquiring the write lock.
    let Some(player_entity) = state.get_player_entity(pid) else {
        return;
    };
    if !pack_withdraw_state_ready(state, pid, bx, by) {
        send_pack_state_error(tx);
        return;
    }

    // Atomic read-validate-write: single ecs.write() lock covers both storage and
    // player — prevents TOCTOU crystal duplication by concurrent clan members.
    let result = modify_pack_with_db(state, bx, by, |ecs, building_entity| {
        let storage_crys = ecs
            .get::<BuildingStorage>(building_entity)
            .expect("BuildingStorage checked before storage transfer")
            .crystals;
        let player_crys = ecs
            .get::<PlayerStats>(player_entity)
            .expect("PlayerStats checked before storage transfer")
            .crystals;

        let mut new_player = [0i64; 6];
        let mut new_storage = [0i64; 6];
        for i in 0..6 {
            let count = player_crys[i] + storage_crys[i];
            if sliders[i] < 0 || count - sliders[i] < 0 {
                return None;
            }
            new_player[i] = count - sliders[i];
            new_storage[i] = sliders[i];
        }

        ecs.get_mut::<BuildingStorage>(building_entity)
            .expect("BuildingStorage checked before storage transfer")
            .crystals = new_storage;
        ecs.get_mut::<PlayerStats>(player_entity)
            .expect("PlayerStats checked before storage transfer")
            .crystals = new_player;
        let mut f = ecs
            .get_mut::<PlayerFlags>(player_entity)
            .expect("PlayerFlags checked before storage transfer");
        f.dirty = true;
        Some(new_player)
    });

    let new_player = match result {
        Ok(Some(new_player)) => new_player,
        Ok(None) => return,
        Err(e) => {
            tracing::error!(x = bx, y = by, error = %e, "Storage transfer failed");
            send_pack_state_error(tx);
            return;
        }
    };

    send_u_packet(tx, "@B", &basket(&new_player, 1).1);
    // Re-open Storage GUI with updated values (C# ref: p.win = GUIWin(p))
    open_storage_gui(state, tx, pid, &view);
}

// ─── Crafter GUI ──────────────────────────────────────────────────────────

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Open Crafter GUI: if craft in progress show progress, else show recipe list.
/// C# ref: `Crafter.GUIWin` -> `StaticSystem.FilledPage` / `GlobalFirstPage`.
fn open_crafter_gui(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    view: &PackView,
) {
    if view.owner_id != pid {
        return;
    }

    let craft_state = state
        .building_index
        .get(&((view.x, view.y).into()))
        .and_then(|ent| {
            let ecs = state.ecs.read();
            let c = ecs.get::<BuildingCrafting>(*ent)?;
            Some((c.recipe_id, c.num, c.end_ts))
        });

    let Some((recipe_id, num, end_ts)) = craft_state else {
        return;
    };

    if let Some(rid) = recipe_id {
        show_crafter_progress(tx, view, rid, num, end_ts);
    } else {
        show_crafter_recipes(tx, view);
    }

    state.modify_player(pid, |ecs, entity| {
        if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) {
            ui.current_window = Some(format!("pack:{}:{}", view.x, view.y));
        }
        Some(())
    });
}

fn show_crafter_progress(
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    view: &PackView,
    recipe_id: i32,
    num: i32,
    end_ts: i64,
) {
    let now = now_ts();
    let recipe = crafting::recipe_by_id(recipe_id);
    let recipe_name = recipe.map_or("?", |r| r.title);

    let done = now >= end_ts;
    let progress = if done {
        100
    } else {
        let total_sec = recipe.map_or(1, |r| i64::from(r.time_sec) * i64::from(num));
        let start_ts = end_ts - total_sec;
        let elapsed = now - start_ts;
        ((elapsed * 100) / total_sec.max(1)).clamp(0, 99) as i32
    };

    let bar_filled = progress / 2;
    let bar_empty = 50 - bar_filled;
    let bar = format!(
        "{}{}",
        "|".repeat(bar_filled as usize),
        "-".repeat(bar_empty as usize)
    );

    let status = if done {
        "ГОТОВО".to_string()
    } else {
        let remain = end_ts - now;
        format!("осталось {remain}с")
    };

    let text = format!("Крафт: {recipe_name} x{num}\n\n[{bar}] {progress}%\n{status}");

    use super::horb::{Button, Horb};
    let mut win = Horb::new("Крафтер").text(text);
    if done {
        win = win.button(Button::new(
            "Забрать",
            format!("craft_claim:{}:{}", view.x, view.y),
        ));
    }
    win.close_button().send_raw(tx);
}

fn show_crafter_recipes(tx: &mpsc::UnboundedSender<Vec<u8>>, view: &PackView) {
    let recipes = crafting::recipes();
    let crys_names = ["зель", "синь", "крась", "фиоль", "бель", "голь"];

    let mut text = String::from("Выберите рецепт:\n");
    use super::horb::{Button, Horb};
    let mut win = Horb::new("Крафтер");

    for r in recipes {
        let cost_str: Vec<String> = r
            .cost_crys
            .iter()
            .map(|c| {
                let name = crys_names.get(c.id as usize).unwrap_or(&"?");
                format!("{name}x{}", c.num)
            })
            .collect();
        let cost_display = if cost_str.is_empty() {
            String::new()
        } else {
            format!(" ({})", cost_str.join("+"))
        };

        text.push_str(&format!(
            "\n- {} x{} - {}с{}",
            r.title, r.result.num, r.time_sec, cost_display
        ));

        win = win.button(Button::new(
            r.title,
            format!("craft_recipe:{}:{}:{}", r.id, view.x, view.y),
        ));
    }

    win.text(text)
        .button(Button::new(
            "Удалить",
            format!("pack_op:remove:{}:{}", view.x, view.y),
        ))
        .close_button()
        .send_raw(tx);
}

/// Show recipe details + Start button.
/// Called from `craft_recipe:{id}:{x}:{y}` but `handle_complex_button` parses
/// only the prefix `craft_recipe:` and passes the rest as a string.
fn handle_craft_recipe_view(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    _pid: PlayerId,
    args: &str,
) {
    let _ = state;
    let parts: Vec<&str> = args.split(':').collect();
    if parts.len() < 3 {
        send_crafter_action_error(tx);
        return;
    }
    let (recipe_id, bx, by) = match (
        parts[0].parse::<i32>(),
        parts[1].parse::<i32>(),
        parts[2].parse::<i32>(),
    ) {
        (Ok(recipe_id), Ok(bx), Ok(by)) => (recipe_id, bx, by),
        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
            tracing::warn!(action = args, error = ?e, "Invalid craft recipe action");
            send_crafter_action_error(tx);
            return;
        }
    };

    let Some(recipe) = crafting::recipe_by_id(recipe_id) else {
        return;
    };

    let crys_names = ["зель", "синь", "крась", "фиоль", "бель", "голь"];

    let mut cost_lines = String::new();
    for c in recipe.cost_crys {
        let name = crys_names.get(c.id as usize).unwrap_or(&"?");
        cost_lines.push_str(&format!("  {name} x{}\n", c.num));
    }
    for c in recipe.cost_res {
        cost_lines.push_str(&format!("  предмет#{} x{}\n", c.id, c.num));
    }

    let text = format!(
        "Рецепт: {}\nРезультат: x{}\nВремя: {}с\n\nСтоимость:\n{}",
        recipe.title, recipe.result.num, recipe.time_sec, cost_lines
    );

    use super::horb::{Button, Horb};
    Horb::new("Крафтер")
        .text(text)
        .button(Button::new(
            "Запустить (x1)",
            format!("craft_start:{recipe_id}:1:{bx}:{by}"),
        ))
        .close_button()
        .send_raw(tx);
}

/// Start crafting: deduct resources, set timer.
/// Button format: `craft_start:{recipe_id}:{num}:{x}:{y}`
fn handle_craft_start(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    args: &str,
) {
    let parts: Vec<&str> = args.split(':').collect();
    if parts.len() < 4 {
        send_crafter_action_error(tx);
        return;
    }
    let (recipe_id, num, bx, by) = match (
        parts[0].parse::<i32>(),
        parts[1].parse::<i32>(),
        parts[2].parse::<i32>(),
        parts[3].parse::<i32>(),
    ) {
        (Ok(recipe_id), Ok(num), Ok(bx), Ok(by)) => (recipe_id, num.max(1), bx, by),
        (Err(e), _, _, _) | (_, Err(e), _, _) | (_, _, Err(e), _) | (_, _, _, Err(e)) => {
            tracing::warn!(player_id = %pid, action = args, error = ?e, "Invalid craft start action");
            send_crafter_action_error(tx);
            return;
        }
    };

    let Some(recipe) = crafting::recipe_by_id(recipe_id) else {
        return;
    };

    let Some(view) = state.get_pack_at(bx, by) else {
        return;
    };
    if view.pack_type != PackType::Craft || view.owner_id != pid {
        return;
    }

    let standing_on_crafter = state
        .query_player_opt(pid, |ecs, entity| {
            let pos = ecs.get::<PlayerPosition>(entity)?;
            Some(pos.x == bx && pos.y == by)
        })
        .unwrap_or(false);
    if !standing_on_crafter {
        send_u_packet(tx, "OK", &ok_message("Недостаточно ресов", "...").1);
        return;
    }

    let craft_state = state
        .building_index
        .get(&((bx, by).into()))
        .and_then(|ent| {
            let ecs = state.ecs.read();
            let c = ecs.get::<BuildingCrafting>(*ent)?;
            Some(c.recipe_id.is_some())
        });
    let Some(already_crafting) = craft_state else {
        tracing::error!(
            x = bx,
            y = by,
            "Building crafting component missing for craft start"
        );
        send_crafter_action_error(tx);
        return;
    };
    if already_crafting {
        send_u_packet(tx, "OK", &ok_message("Крафтер", "Крафт уже запущен").1);
        return;
    }

    let deducted = state
        .modify_player(pid, |ecs, entity| {
            if ecs.get::<PlayerStats>(entity).is_none()
                || ecs.get::<PlayerFlags>(entity).is_none()
                || (!recipe.cost_res.is_empty() && ecs.get::<PlayerInventory>(entity).is_none())
            {
                send_crafter_state_error(tx);
                return None;
            }
            {
                let pstats = ecs.get::<PlayerStats>(entity)?;
                for c in recipe.cost_crys {
                    if pstats.crystals[c.id as usize] < i64::from(c.num) * i64::from(num) {
                        return Some(false);
                    }
                }
                if !recipe.cost_res.is_empty() {
                    let inv = ecs.get::<PlayerInventory>(entity)?;
                    for c in recipe.cost_res {
                        let have = inv.items.get(&c.id).copied().unwrap_or(0);
                        if have < c.num * num {
                            return Some(false);
                        }
                    }
                }
            }

            {
                let mut pstats = ecs.get_mut::<PlayerStats>(entity)?;
                for c in recipe.cost_crys {
                    pstats.crystals[c.id as usize] -= i64::from(c.num) * i64::from(num);
                }
                send_u_packet(tx, "@B", &basket(&pstats.crystals, 1).1);
            }

            if !recipe.cost_res.is_empty() {
                let mut inv = ecs.get_mut::<PlayerInventory>(entity)?;
                for c in recipe.cost_res {
                    let entry = inv.items.entry(c.id).or_insert(0);
                    *entry -= c.num * num;
                }
                send_inventory(tx, &mut inv);
            }
            let mut flags = ecs.get_mut::<PlayerFlags>(entity)?;
            flags.dirty = true;

            Some(true)
        })
        .flatten();

    let Some(deducted) = deducted else {
        return;
    };
    if !deducted {
        send_u_packet(tx, "OK", &ok_message("Крафтер", "Недостаточно ресурсов").1);
        return;
    }

    let end_ts = now_ts() + i64::from(recipe.time_sec) * i64::from(num);
    let updated = match modify_pack_with_db(state, bx, by, |ecs, entity| {
        if let Some(mut c) = ecs.get_mut::<BuildingCrafting>(entity) {
            c.recipe_id = Some(recipe_id);
            c.num = num;
            c.end_ts = end_ts;
            true
        } else {
            false
        }
    }) {
        Ok(updated) => updated,
        Err(e) => {
            tracing::error!(x = bx, y = by, error = %e, "Craft start failed after resource deduction");
            false
        }
    };
    if !updated {
        state.modify_player(pid, |ecs, entity| {
            if ecs.get::<PlayerStats>(entity).is_none()
                || ecs.get::<PlayerFlags>(entity).is_none()
                || (!recipe.cost_res.is_empty() && ecs.get::<PlayerInventory>(entity).is_none())
            {
                send_crafter_state_error(tx);
                return None;
            }
            {
                let mut pstats = ecs.get_mut::<PlayerStats>(entity)?;
                for c in recipe.cost_crys {
                    pstats.crystals[c.id as usize] += i64::from(c.num) * i64::from(num);
                }
                send_u_packet(tx, "@B", &basket(&pstats.crystals, 1).1);
            }

            if !recipe.cost_res.is_empty() {
                let mut inv = ecs.get_mut::<PlayerInventory>(entity)?;
                for c in recipe.cost_res {
                    let entry = inv.items.entry(c.id).or_insert(0);
                    *entry += c.num * num;
                }
                send_inventory(tx, &mut inv);
            }

            let mut f = ecs.get_mut::<PlayerFlags>(entity)?;
            f.dirty = true;
            Some(())
        });
        send_crafter_action_error(tx);
        return;
    }

    broadcast_pack_update(state, &view);
    show_crafter_progress(tx, &view, recipe_id, num, end_ts);
}

/// Claim finished craft. Button format: `craft_claim:{x}:{y}`
fn handle_craft_claim(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    args: &str,
) {
    let parts: Vec<&str> = args.split(':').collect();
    if parts.len() < 2 {
        send_crafter_action_error(tx);
        return;
    }
    let (bx, by) = match (parts[0].parse::<i32>(), parts[1].parse::<i32>()) {
        (Ok(bx), Ok(by)) => (bx, by),
        (Err(e), _) | (_, Err(e)) => {
            tracing::warn!(player_id = %pid, action = args, error = ?e, "Invalid craft claim action");
            send_crafter_action_error(tx);
            return;
        }
    };

    let Some(view) = state.get_pack_at(bx, by) else {
        return;
    };
    if view.pack_type != PackType::Craft || view.owner_id != pid {
        return;
    }

    let craft_info = state
        .building_index
        .get(&((bx, by).into()))
        .and_then(|ent| {
            let ecs = state.ecs.read();
            let c = ecs.get::<BuildingCrafting>(*ent)?;
            Some((c.recipe_id, c.num, c.end_ts))
        });

    let Some((Some(recipe_id), num, end_ts)) = craft_info else {
        return;
    };

    if now_ts() < end_ts {
        send_u_packet(tx, "OK", &ok_message("Крафтер", "Крафт ещё не завершён").1);
        return;
    }

    let Some(recipe) = crafting::recipe_by_id(recipe_id) else {
        return;
    };

    let Some(player_entity) = state.get_player_entity(pid) else {
        tracing::error!(player_id = %pid, "Player entity missing for craft claim");
        send_crafter_action_error(tx);
        return;
    };
    let Some(building_entity) = state
        .building_index
        .get(&((bx, by).into()))
        .map(|entry| *entry.value())
    else {
        tracing::error!(player_id = %pid, x = bx, y = by, "Craft building entity missing for claim");
        send_crafter_action_error(tx);
        return;
    };

    let claimed = {
        let mut ecs = state.ecs.write();
        if ecs.get::<PlayerInventory>(player_entity).is_none()
            || ecs.get::<PlayerFlags>(player_entity).is_none()
            || ecs.get::<BuildingCrafting>(building_entity).is_none()
            || ecs.get::<BuildingFlags>(building_entity).is_none()
        {
            tracing::error!(player_id = %pid, x = bx, y = by, "Required state missing for craft claim");
            send_crafter_state_error(tx);
            return;
        }
        let Some(craft) = ecs.get::<BuildingCrafting>(building_entity) else {
            tracing::error!(player_id = %pid, x = bx, y = by, "Building crafting component missing for claim");
            return;
        };
        if craft.recipe_id != Some(recipe_id) || craft.num != num || craft.end_ts != end_ts {
            false
        } else {
            {
                let mut inv = ecs
                    .get_mut::<PlayerInventory>(player_entity)
                    .expect("PlayerInventory checked before craft claim mutation");
                let entry = inv.items.entry(recipe.result.id).or_insert(0);
                *entry += recipe.result.num * num;
                send_inventory(tx, &mut inv);
            }

            {
                let mut craft = ecs
                    .get_mut::<BuildingCrafting>(building_entity)
                    .expect("BuildingCrafting checked before craft claim mutation");
                craft.recipe_id = None;
                craft.num = 0;
                craft.end_ts = 0;
            }
            let mut flags = ecs
                .get_mut::<PlayerFlags>(player_entity)
                .expect("PlayerFlags checked before craft claim mutation");
            flags.dirty = true;
            let mut flags = ecs
                .get_mut::<BuildingFlags>(building_entity)
                .expect("BuildingFlags checked before craft claim mutation");
            flags.dirty = true;
            true
        }
    };

    if !claimed {
        send_crafter_action_error(tx);
        return;
    }

    broadcast_pack_update(state, &view);
    show_crafter_recipes(tx, &view);
}

/// Build the Teleport GUI showing list of nearby teleports (1:1 with C# `Teleport.GUIWin`).
fn open_teleport_gui(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    view: &PackView,
) {
    let nearby_tps: Vec<(i32, i32)> = {
        let ecs = state.ecs.read();
        state
            .building_index
            .iter()
            .filter_map(|entry| {
                let entity = *entry.value();
                let meta = ecs.get::<BuildingMetadata>(entity)?;
                if meta.pack_type != PackType::Teleport {
                    return None;
                }
                let pos = ecs.get::<GridPosition>(entity)?;
                if pos.x == view.x && pos.y == view.y {
                    return None;
                }
                if (pos.x - view.x).abs() >= 1000 || (pos.y - view.y).abs() >= 1000 {
                    return None;
                }
                let pstats = ecs.get::<BuildingStats>(entity)?;
                if pstats.charge <= 0 {
                    return None;
                }
                Some((pos.x, pos.y))
            })
            .collect()
    };

    use super::horb::{Button, Horb};

    let pstats_info = state
        .building_index
        .get(&((view.x, view.y).into()))
        .and_then(|ent| {
            let ecs = state.ecs.read();
            let pstats = ecs.get::<BuildingStats>(*ent)?;
            Some((pstats.hp, pstats.max_hp))
        });
    let Some((hp, mhp)) = pstats_info else {
        tracing::error!(
            x = view.x,
            y = view.y,
            "Building stats missing for teleport GUI"
        );
        send_pack_action_error(tx);
        return;
    };

    let text = if nearby_tps.is_empty() {
        format!(
            "Заряд: {}\nПрочность: {}/{}\n\nНет доступных телепортов поблизости.",
            view.charge, hp, mhp
        )
    } else {
        format!(
            "Заряд: {}\nПрочность: {}/{}\n\nДоступные телепорты:",
            view.charge, hp, mhp
        )
    };

    // Мини-карта: rect на чанк вокруг телепорта (цвет по пустоте центральной
    // клетки, 1:1 C# ConvertMapPart) + кликабельные ТП-точки прямо на канвасе
    // (markers): клик по точке = телепорт (`tp:x:y`). Те же ТП дублируются
    // кнопками-списком ниже (пользователь хотел и список, и точки на карте).
    let markers: Vec<(i32, i32, String)> = nearby_tps
        .iter()
        .map(|&(tpx, tpy)| (tpx, tpy, format!("tp:{tpx}:{tpy}")))
        .collect();
    let mut win = Horb::new("Тп").text(text).minimap(
        view.x,
        view.y,
        8, // radius in chunks
        |x, y| {
            if state.world.valid_coord(x, y) {
                Some(state.world.is_empty(x, y))
            } else {
                None
            }
        },
        &markers,
    );

    for (tpx, tpy) in &nearby_tps {
        win = win.button(Button::new(
            format!("TP {tpx}:{tpy}"),
            format!("tp:{tpx}:{tpy}"),
        ));
    }
    win.button(Button::new(
        "Забрать деньги",
        format!("pack_op:take_money:{}:{}", view.x, view.y),
    ))
    .button(Button::new(
        "Забрать кристаллы",
        format!("pack_op:take_crys:{}:{}", view.x, view.y),
    ))
    .button(Button::new(
        "Удалить",
        format!("pack_op:remove:{}:{}", view.x, view.y),
    ))
    .close_button()
    .send(state, tx, pid, format!("pack:{}:{}", view.x, view.y));
}

/// Handle `tp:{x}:{y}` button — teleport player to destination TP.
fn handle_teleport_action(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    coords: &str,
) {
    let parts: Vec<&str> = coords.split(':').collect();
    if parts.len() != 2 {
        return;
    }
    let Ok(dest_x) = parts[0].parse::<i32>() else {
        return;
    };
    let Ok(dest_y) = parts[1].parse::<i32>() else {
        return;
    };

    let Some(dest_view) = state.get_pack_at(dest_x, dest_y) else {
        tracing::warn!(
            player_id = %pid,
            destination_x = dest_x,
            destination_y = dest_y,
            "TP action: destination not found"
        );
        return;
    };
    if dest_view.pack_type != PackType::Teleport || dest_view.charge <= 0 {
        tracing::warn!(
            player_id = %pid,
            destination_x = dest_x,
            destination_y = dest_y,
            "TP action: destination not a valid teleport"
        );
        return;
    }

    let src_coords = state.query_player_opt(pid, |ecs, entity| {
        let ui = ecs.get::<PlayerUI>(entity)?;
        let window = ui.current_window.as_deref()?;
        let rest = window.strip_prefix("pack:")?;
        let p: Vec<&str> = rest.split(':').collect();
        if p.len() == 2 {
            Some((p[0].parse::<i32>().ok()?, p[1].parse::<i32>().ok()?))
        } else {
            None
        }
    });

    let Some((src_x, src_y)) = src_coords else {
        tracing::warn!(
            player_id = %pid,
            "TP action: player not at a teleport window"
        );
        return;
    };

    let Some(src_view) = state.get_pack_at(src_x, src_y) else {
        return;
    };
    if src_view.pack_type != PackType::Teleport || src_view.charge <= 0 {
        return;
    }

    state.modify_player(pid, |ecs, entity| {
        if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) {
            ui.current_window = None;
        }
        Some(())
    });
    let g = gu_close();
    send_u_packet(tx, g.0, &g.1);

    let tp_y = dest_y + 3;
    state.modify_player(pid, |ecs, entity| {
        if let Some(mut pos) = ecs.get_mut::<PlayerPosition>(entity) {
            pos.x = dest_x;
            pos.y = tp_y;
        }
        Some(())
    });

    let tp_pkt = tp(dest_x, tp_y);
    send_u_packet(tx, tp_pkt.0, &tp_pkt.1);

    crate::net::session::play::chunks::check_chunk_changed(state, tx, pid);

    tracing::info!(
        player_id = %pid,
        from_x = src_x,
        from_y = src_y,
        to_x = dest_x,
        to_y = tp_y,
        "Teleported player"
    );
}

// ─── Spot GUI ────────────────────────────────────────────────────────────────

/// 1:1 with C# `Spot.GUIWin`:
/// - Non-owner: returns null (no window opens).
/// - Owner: returns `new Window() { Tabs = [] }` (empty window with programmator controls).
fn open_spot_gui(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    view: &PackView,
) {
    // C# ref: `if (p.id != ownerid) return null;`
    if view.owner_id != pid {
        close_player_window(state, tx, pid);
        return;
    }

    use super::horb::{Button, Horb};

    Horb::new("СПОТ")
        .button(Button::new(
            "Удалить",
            format!("pack_op:remove:{}:{}", view.x, view.y),
        ))
        .close_button()
        .send(state, tx, pid, format!("pack:{}:{}", view.x, view.y));
}

// ─── Market GUI ──────────────────────────────────────────────────────────

/// Open Market GUI with tabs (1:1 with C# `Market.GUIWin`).
/// `active_tab` is one of: "sellcrys", "buycrys", "auc".
fn open_market_gui(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    view: &PackView,
    active_tab: &str,
) {
    let is_owner = view.owner_id == pid;

    // Fetch player money and crystals
    let player_info = state.query_player_opt(pid, |ecs, entity| {
        let pstats = ecs.get::<PlayerStats>(entity)?;
        Some((pstats.money, pstats.crystals))
    });

    let Some((player_money, player_crys)) = player_info else {
        return;
    };

    // Вкладки: активная получает пустой action, остальные — свой.
    let tabs = market_tabs(active_tab);

    let (page, window_tag) = match active_tab {
        "buycrys" => (
            build_market_buy_page(state, player_money, is_owner, tabs),
            format!("market:{}:{}:buycrys", view.x, view.y),
        ),
        _ => (
            build_market_sell_page(state, &player_crys, is_owner, tabs),
            format!("market:{}:{}:sellcrys", view.x, view.y),
        ),
    };

    page.send(state, tx, pid, window_tag);
}

/// Вкладки market как `Vec<Tab>` для `Horb`-builder.
pub fn market_tabs(active_tab: &str) -> Vec<super::horb::Tab> {
    use super::horb::Tab;
    [
        ("ПРОДАЖА", "sellcrys"),
        ("Покупка", "buycrys"),
        ("Auc", "auc"),
    ]
    .into_iter()
    .map(|(label, action)| {
        if active_tab == action {
            Tab::active(label)
        } else {
            Tab::new(label, action)
        }
    })
    .collect()
}

/// Build sell tab page JSON.
/// C# ref: Market.BuildSelltab — `CrystalConfig` with sell prices, sliders up to player's crystals.
fn build_market_sell_page(
    state: &Arc<GameState>,
    player_crys: &[i64; 6],
    is_owner: bool,
    tabs: Vec<super::horb::Tab>,
) -> super::horb::Horb {
    use super::horb::{Button, Horb};
    // crys_lines format: "LeftMin:RightMin:Denominator:CurrentValue:Label"
    // C# CrysLine(label, leftMin=0, rightMin=0, denominator=player_crys[i], currentValue=0)
    let lines: Vec<String> = (0..6)
        .map(|i| {
            let cost = market::get_crystal_cost(state, i);
            let label = format!("<color=#aaeeaa>{cost}$</color>");
            format!("0:0:{}:0:{}", player_crys[i], label)
        })
        .collect();

    tabs.into_iter()
        .fold(Horb::new("Market"), Horb::tab)
        .text("Продажа кри")
        .crystals(" ", "цена", false, lines)
        // Порядок: сначала «Продать», затем «Продать всё» (девиация от C#
        // референса — явное требование пользователя).
        .button(Button::new("Продать", "sell:%M%"))
        .button(Button::new("Продать всё", "sellall"))
        .close_button()
        .admin(is_owner)
}

/// Build buy tab page JSON.
/// C# ref: Market.BuildBuytab — `CrystalConfig` with buy prices (10x), sliders denominator =
/// player.money / (cost * 10). `BuyMode` = true (`crys_buy: true`).
fn build_market_buy_page(
    state: &Arc<GameState>,
    player_money: i64,
    is_owner: bool,
    tabs: Vec<super::horb::Tab>,
) -> super::horb::Horb {
    use super::horb::{Button, Horb};
    let lines: Vec<String> = (0..6)
        .map(|i| {
            let buy_price = market::get_crystal_buy_price(state, i);
            let max_can_buy = if buy_price > 0 {
                player_money / buy_price
            } else {
                0
            };
            let label = format!("<color=#aaeeaa>{buy_price}$</color>");
            format!("0:0:{max_can_buy}:0:{label}")
        })
        .collect();

    tabs.into_iter()
        .fold(Horb::new("Market"), Horb::tab)
        .text("Покупка")
        .crystals(" ", "цена", true, lines)
        .button(Button::new("Купить", "buy:%M%"))
        .close_button()
        .admin(is_owner)
}

/// Resolve market coordinates and tab from `current_window` ("market:{x}:{y}:{tab}").
pub fn resolve_market_window(state: &Arc<GameState>, pid: PlayerId) -> Option<(i32, i32, String)> {
    state.query_player_opt(pid, |ecs, entity| {
        let ui = ecs.get::<PlayerUI>(entity)?;
        let window = ui.current_window.as_deref()?;
        let rest = window.strip_prefix("market:")?;
        let parts: Vec<&str> = rest.split(':').collect();
        if parts.len() == 3 {
            Some((
                parts[0].parse::<i32>().ok()?,
                parts[1].parse::<i32>().ok()?,
                parts[2].to_string(),
            ))
        } else {
            None
        }
    })
}

/// Handle Market tab switching.
async fn handle_market_tab_switch(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    tab: &str,
) {
    let Some((bx, by, _old_tab)) = resolve_market_window(state, pid) else {
        return;
    };
    let Some(view) = state.get_pack_at(bx, by) else {
        return;
    };
    if view.pack_type != PackType::Market {
        return;
    }
    if tab == "auc" {
        crate::net::session::ui::auction_gui::open_auc_grid(state, tx, pid, bx, by).await;
    } else {
        open_market_gui(state, tx, pid, &view, tab);
    }
}

/// Handle "sell:%M%" — sell crystals from sliders.
/// C# ref: `MarketSystem.Sell(sliders, p, m)`.
fn handle_market_sell(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    slider_data: &str,
) {
    let Some(sliders) = parse_six_i64_fields(slider_data) else {
        return;
    };

    let Some((bx, by, _tab)) = resolve_market_window(state, pid) else {
        return;
    };
    let Some(view) = state.get_pack_at(bx, by) else {
        return;
    };
    if view.pack_type != PackType::Market {
        return;
    }

    do_market_sell(state, tx, pid, &sliders, bx, by);
}

/// Handle "sellall" — sell all player's crystals.
/// C# ref: `MarketSystem.Sell(p.crys.cry, p, m)`.
fn handle_market_sellall(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let Some((bx, by, _tab)) = resolve_market_window(state, pid) else {
        return;
    };
    let Some(view) = state.get_pack_at(bx, by) else {
        return;
    };
    if view.pack_type != PackType::Market {
        return;
    }

    // Get player's full crystal array
    let player_crys = state.query_player_opt(pid, |ecs, entity| {
        ecs.get::<PlayerStats>(entity).map(|s| s.crystals)
    });
    let Some(player_crys) = player_crys else {
        tracing::error!(player_id = %pid, "Player stats missing for market sellall");
        send_market_action_error(tx);
        return;
    };

    let sliders: Vec<i64> = player_crys.to_vec();
    do_market_sell(state, tx, pid, &sliders, bx, by);
}

/// Common sell logic (used by sell and sellall).
/// C# ref: `MarketSystem.Sell`:
///   for each i: if `RemoveCrys` succeeds, money += value * GetCrysCost(i)
///   market.moneyinside += (long)(money * 0.1)
fn do_market_sell(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    sliders: &[i64],
    bx: i32,
    by: i32,
) {
    let Some(player_entity) = state.get_player_entity(pid) else {
        tracing::error!(player_id = %pid, "Player entity missing for market sell");
        send_market_action_error(tx);
        return;
    };
    let Some(building_entity) = state
        .building_index
        .get(&((bx, by).into()))
        .map(|entry| *entry.value())
    else {
        tracing::error!(x = bx, y = by, "Market building entity missing for sell");
        send_market_action_error(tx);
        return;
    };

    let sell_result = 'sell: {
        let mut ecs = state.ecs.write();
        if ecs.get::<PlayerStats>(player_entity).is_none()
            || ecs.get::<PlayerFlags>(player_entity).is_none()
            || ecs.get::<BuildingStorage>(building_entity).is_none()
            || ecs.get::<BuildingFlags>(building_entity).is_none()
        {
            break 'sell None;
        }

        let mut total_money: i64 = 0;
        let Some((crystals, money_now, creds_now)) = ({
            let Some(mut pstats) = ecs.get_mut::<PlayerStats>(player_entity) else {
                break 'sell None;
            };
            for i in 0..6 {
                let to_sell = sliders[i];
                if to_sell <= 0 {
                    continue;
                }
                // C# RemoveCrys: only succeeds if player has enough.
                if pstats.crystals[i] >= to_sell {
                    let price = market::get_crystal_cost(state, i);
                    let Some(earned) = to_sell.checked_mul(price) else {
                        continue;
                    };
                    pstats.crystals[i] -= to_sell;
                    total_money = total_money.saturating_add(earned);
                }
            }
            pstats.money = pstats.money.saturating_add(total_money);
            Some((pstats.crystals, pstats.money, pstats.creds))
        }) else {
            break 'sell None;
        };

        if total_money > 0 {
            let Some(mut storage) = ecs.get_mut::<BuildingStorage>(building_entity) else {
                break 'sell None;
            };
            storage.money += total_money / 10;
            let mut flags = ecs
                .get_mut::<BuildingFlags>(building_entity)
                .expect("BuildingFlags checked before market sell mutation");
            flags.dirty = true;
            let mut flags = ecs
                .get_mut::<PlayerFlags>(player_entity)
                .expect("PlayerFlags checked before market sell mutation");
            flags.dirty = true;
        }

        Some((crystals, money_now, creds_now))
    };
    let Some((crystals, money_now, creds_now)) = sell_result else {
        tracing::error!(player_id = %pid, x = bx, y = by, "Market sell failed before mutation");
        send_market_state_error(tx);
        return;
    };

    send_u_packet(tx, "@B", &basket(&crystals, 1).1);
    send_u_packet(tx, "P$", &money(money_now, creds_now).1);

    // Re-render sell tab with updated crystal counts
    let Some(view) = state.get_pack_at(bx, by) else {
        return;
    };
    open_market_gui(state, tx, pid, &view, "sellcrys");
}

/// Handle "buy:%M%" — buy crystals with money.
/// C# ref: `MarketSystem.Buy(sliders, p, m)`.
fn handle_market_buy(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    slider_data: &str,
) {
    let Some(sliders) = parse_six_i64_fields(slider_data) else {
        return;
    };

    let Some((bx, by, _tab)) = resolve_market_window(state, pid) else {
        return;
    };
    let Some(view) = state.get_pack_at(bx, by) else {
        return;
    };
    if view.pack_type != PackType::Market {
        return;
    }

    // Buy crystals: deduct money, add crystals
    // C# ref: for each i: if sliders[i] > 0 && player can afford -> deduct money, add crystals
    let bought = state
        .modify_player(pid, |ecs, entity| {
            if ecs.get::<PlayerStats>(entity).is_none() || ecs.get::<PlayerFlags>(entity).is_none()
            {
                send_market_state_error(tx);
                return Some(false);
            }
            let (crystals, money_now, creds_now) = {
                let mut pstats = ecs.get_mut::<PlayerStats>(entity)?;
                for i in 0..6 {
                    let to_buy = sliders[i];
                    if to_buy <= 0 {
                        continue;
                    }
                    // checked_mul: protect against overflow in release mode (wrapping mul could yield
                    // negative cost, bypassing the affordability check and granting free crystals/money).
                    let Some(cost) = to_buy.checked_mul(market::get_crystal_buy_price(state, i))
                    else {
                        continue;
                    };
                    // C# ref: if p.money - (sliders[i] * World.GetCrysCost(i) * 10) < 0 continue
                    if pstats.money < cost {
                        continue;
                    }
                    pstats.money -= cost;
                    pstats.crystals[i] = pstats.crystals[i].saturating_add(to_buy);
                }
                (pstats.crystals, pstats.money, pstats.creds)
            };
            let mut f = ecs.get_mut::<PlayerFlags>(entity)?;
            f.dirty = true;
            send_u_packet(tx, "@B", &basket(&crystals, 1).1);
            send_u_packet(tx, "P$", &money(money_now, creds_now).1);
            Some(true)
        })
        .flatten();
    let Some(bought) = bought else {
        send_market_state_error(tx);
        return;
    };
    if !bought {
        return;
    }

    // Re-render buy tab with updated money
    open_market_gui(state, tx, pid, &view, "buycrys");
}

/// Handle "getprofit" — owner withdraws accumulated market profit.
/// C# ref: `Market.onadmn` — transfer moneyinside to player, reset to 0,
/// then re-open the admin `RichList` page.
fn handle_market_getprofit(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let Some((bx, by, _tab)) = resolve_market_window(state, pid) else {
        return;
    };
    let Some(view) = state.get_pack_at(bx, by) else {
        return;
    };
    if view.pack_type != PackType::Market || view.owner_id != pid {
        return;
    }
    let player_state_ready = state
        .query_player(pid, |ecs, entity| {
            ecs.get::<PlayerStats>(entity).is_some() && ecs.get::<PlayerFlags>(entity).is_some()
        })
        .unwrap_or(false);
    if !player_state_ready {
        send_market_state_error(tx);
        return;
    }
    let building_state_ready = state
        .building_index
        .get(&((bx, by).into()))
        .map(|ent| {
            let ecs = state.ecs.read();
            ecs.get::<BuildingStorage>(*ent).is_some() && ecs.get::<BuildingFlags>(*ent).is_some()
        })
        .unwrap_or(false);
    if !building_state_ready {
        send_market_state_error(tx);
        return;
    }

    // Transfer profit from building to player
    let mut amount = 0i64;
    let updated = match modify_pack_with_db(state, bx, by, |ecs, entity| {
        let mut storage = ecs
            .get_mut::<BuildingStorage>(entity)
            .expect("BuildingStorage checked before market profit mutation");
        amount = storage.money;
        storage.money = 0;
        true
    }) {
        Ok(updated) => updated,
        Err(e) => {
            tracing::error!(x = bx, y = by, error = %e, "Market profit withdrawal failed");
            send_market_state_error(tx);
            return;
        }
    };
    if !updated {
        send_market_action_error(tx);
        return;
    }

    if amount > 0 {
        state.modify_player(pid, |ecs, entity| {
            let (money_now, creds_now) = {
                let mut s = ecs.get_mut::<PlayerStats>(entity)?;
                s.money += amount;
                (s.money, s.creds)
            };
            let mut f = ecs.get_mut::<PlayerFlags>(entity)?;
            f.dirty = true;
            send_u_packet(tx, "P$", &money(money_now, creds_now).1);
            Some(())
        });
    }

    // Re-open admin page with updated profit (now 0)
    open_market_admin_gui(state, tx, pid, bx, by);
}

/// Open Market admin page with `RichList` (1:1 with C# `Market.onadmn`).
/// Shows HP and profit withdrawal button. Called from ADMN gear icon.
pub fn open_market_admin_gui(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    pack_x: i32,
    pack_y: i32,
) {
    let Some(view) = state.get_pack_at(pack_x, pack_y) else {
        return;
    };
    if view.owner_id != pid {
        return;
    }

    // Fetch building details from ECS
    let details = state
        .building_index
        .get(&((pack_x, pack_y).into()))
        .and_then(|ent| {
            let ecs = state.ecs.read();
            let pstats = ecs.get::<BuildingStats>(*ent)?;
            let storage = ecs.get::<BuildingStorage>(*ent)?;
            Some((pstats.hp, storage.money))
        });

    let Some((hp, money_inside)) = details else {
        return;
    };

    let profit_label = format!("прибыль {money_inside}$");
    let profit_btn_label = if money_inside > 0 {
        "Получить"
    } else {
        ""
    };
    let profit_btn_action = if money_inside > 0 { "getprofit" } else { "" };

    use super::horb::{Horb, RichRow};
    Horb::new("Market")
        .text(" ")
        .rich_row(RichRow::text(format!("hp {hp}")))
        .rich_row(RichRow::button(
            profit_label,
            profit_btn_label,
            profit_btn_action,
        ))
        .close_button()
        .send(state, tx, pid, format!("market:{pack_x}:{pack_y}:admin"));
}

// ─── Settings save ──────────────────────────────────────────────────────

/// Handle `save:{RichList data}` from Settings GUI.
/// C# ref: `Settings.Save(p, list)` → updates settings dict → `SendSettings` + `SendSettingsGUI`.
fn handle_settings_save(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    data: &str,
) {
    // Unity RichList macro %R% substitutes to "key:value#key:value#".
    let Some(pairs) = parse_settings_pairs(data) else {
        tracing::warn!(player_id = %pid, payload = data, "Malformed settings payload");
        send_u_packet(
            tx,
            "OK",
            &ok_message("НАСТРОЙКИ", "Некорректный формат настроек.").1,
        );
        return;
    };

    let parse_i32 = |key: &str| -> Result<Option<i32>, std::num::ParseIntError> {
        pairs.get(key).map(|v| v.parse::<i32>()).transpose()
    };
    let parse_bool = |key: &str| -> Result<Option<bool>, String> {
        pairs
            .get(key)
            .map(|v| match *v {
                "0" => Ok(false),
                "1" => Ok(true),
                other => Err(format!("invalid bool value for {key}: {other}")),
            })
            .transpose()
    };

    let isca = match parse_i32("isca") {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(player_id = %pid, error = ?e, "Invalid isca setting");
            send_u_packet(
                tx,
                "OK",
                &ok_message("НАСТРОЙКИ", "Некорректный масштаб интерфейса.").1,
            );
            return;
        }
    };
    let tsca = match parse_i32("tsca") {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(player_id = %pid, error = ?e, "Invalid tsca setting");
            send_u_packet(
                tx,
                "OK",
                &ok_message("НАСТРОЙКИ", "Некорректный масштаб территории.").1,
            );
            return;
        }
    };
    let mous = match parse_bool("mous") {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(player_id = %pid, error = %e, "Invalid mous setting");
            send_u_packet(
                tx,
                "OK",
                &ok_message("НАСТРОЙКИ", "Некорректное значение настройки.").1,
            );
            return;
        }
    };
    let pot = match parse_bool("pot") {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(player_id = %pid, error = %e, "Invalid pot setting");
            send_u_packet(
                tx,
                "OK",
                &ok_message("НАСТРОЙКИ", "Некорректное значение настройки.").1,
            );
            return;
        }
    };
    let frc = match parse_bool("frc") {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(player_id = %pid, error = %e, "Invalid frc setting");
            send_u_packet(
                tx,
                "OK",
                &ok_message("НАСТРОЙКИ", "Некорректное значение настройки.").1,
            );
            return;
        }
    };
    let ctrl = match parse_bool("ctrl") {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(player_id = %pid, error = %e, "Invalid ctrl setting");
            send_u_packet(
                tx,
                "OK",
                &ok_message("НАСТРОЙКИ", "Некорректное значение настройки.").1,
            );
            return;
        }
    };
    let mof = match parse_bool("mof") {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(player_id = %pid, error = %e, "Invalid mof setting");
            send_u_packet(
                tx,
                "OK",
                &ok_message("НАСТРОЙКИ", "Некорректное значение настройки.").1,
            );
            return;
        }
    };

    let saved = state
        .modify_player(pid, |ecs, entity| {
            if ecs
                .get::<crate::game::player::PlayerSettings>(entity)
                .is_none()
                || ecs.get::<PlayerFlags>(entity).is_none()
            {
                return None;
            }
            let mut s = ecs
                .get_mut::<crate::game::player::PlayerSettings>(entity)
                .expect("PlayerSettings checked before settings save");
            if let Some(v) = isca {
                s.isca = v;
            }
            if let Some(v) = tsca {
                s.tsca = v;
            }
            if let Some(v) = mous {
                s.mous = v;
            }
            if let Some(v) = pot {
                s.pot = v;
            }
            if let Some(v) = frc {
                s.frc = v;
            }
            if let Some(v) = ctrl {
                s.ctrl = v;
            }
            if let Some(v) = mof {
                s.mof = v;
            }
            ecs.get_mut::<PlayerFlags>(entity)
                .expect("PlayerFlags checked before settings save")
                .dirty = true;
            Some(())
        })
        .flatten()
        .is_some();
    if !saved {
        tracing::error!(player_id = %pid, "Player settings state missing for save");
        send_u_packet(
            tx,
            "OK",
            &ok_message("НАСТРОЙКИ", "Состояние настроек недоступно.").1,
        );
        return;
    }

    // C# ref: SendSettings → send #S with updated values, then re-show GUI
    // For now we send #S with the values and re-open the settings GUI
    let Some(sett_wire) = build_settings_wire(state, pid) else {
        tracing::error!(player_id = %pid, "Player settings component missing after save");
        send_u_packet(
            tx,
            "OK",
            &ok_message("НАСТРОЙКИ", "Настройки игрока недоступны.").1,
        );
        return;
    };
    send_u_packet(tx, "#S", &sett_wire);
    crate::net::session::social::misc::handle_sett_ty(state, tx, pid, &[]);
}

/// Build #S packet payload from player's current settings.
/// Wire format: `#key#value#key#value...` — 1:1 с `SettingsPacket.Encode()` в C# референсе.
fn build_settings_wire(state: &Arc<GameState>, pid: PlayerId) -> Option<Vec<u8>> {
    let s = state.query_player_opt(pid, |ecs, entity| {
        ecs.get::<crate::game::player::PlayerSettings>(entity)
            .copied()
    })?;
    let pairs: &[(&str, String)] = &[
        ("cc", s.cc.to_string()),
        ("snd", if s.snd { "1" } else { "0" }.to_string()),
        ("mus", if s.mus { "1" } else { "0" }.to_string()),
        ("isca", s.isca.to_string()),
        ("tsca", s.tsca.to_string()),
        ("mous", if s.mous { "1" } else { "0" }.to_string()),
        ("pot", if s.pot { "1" } else { "0" }.to_string()),
        ("frc", if s.frc { "1" } else { "0" }.to_string()),
        ("ctrl", if s.ctrl { "1" } else { "0" }.to_string()),
        ("mof", if s.mof { "1" } else { "0" }.to_string()),
    ];
    let inner = pairs
        .iter()
        .map(|(k, v)| format!("{k}#{v}"))
        .collect::<Vec<_>>()
        .join("#");
    Some(format!("#{inner}").into_bytes())
}

// ─── Программатор ────────────────────────────────────────────────────────────

fn open_create_prog_dialog(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    use crate::net::session::ui::horb::{Button, Horb};

    Horb::new("НОВАЯ ПРОГРАММА")
        .text("Введите название программы")
        .input("Название программы...", true)
        .button(Button::new("Создать", "createprog:%I%"))
        .close_button()
        .send(state, tx, pid, "createprog");
}

fn send_market_action_error(tx: &mpsc::UnboundedSender<Vec<u8>>) {
    send_u_packet(tx, "OK", &ok_message("МАРКЕТ", "Некорректное действие.").1);
}

fn send_market_state_error(tx: &mpsc::UnboundedSender<Vec<u8>>) {
    send_u_packet(
        tx,
        "OK",
        &ok_message("МАРКЕТ", "Состояние маркета недоступно.").1,
    );
}

fn send_pack_action_error(tx: &mpsc::UnboundedSender<Vec<u8>>) {
    send_u_packet(tx, "OK", &ok_message("ЗДАНИЕ", "Некорректное действие.").1);
}

fn send_pack_state_error(tx: &mpsc::UnboundedSender<Vec<u8>>) {
    send_u_packet(
        tx,
        "OK",
        &ok_message("ЗДАНИЕ", "Состояние здания недоступно.").1,
    );
}

fn send_crafter_action_error(tx: &mpsc::UnboundedSender<Vec<u8>>) {
    send_u_packet(tx, "OK", &ok_message("КРАФТЕР", "Некорректное действие.").1);
}

fn send_crafter_state_error(tx: &mpsc::UnboundedSender<Vec<u8>>) {
    send_u_packet(
        tx,
        "OK",
        &ok_message("КРАФТЕР", "Состояние крафтера недоступно.").1,
    );
}

fn send_programmator_error(tx: &mpsc::UnboundedSender<Vec<u8>>, message: &str) {
    send_u_packet(tx, "OK", &ok_message("ПРОГРАММАТОР", message).1);
}

fn clear_programmator_window(state: &Arc<GameState>, pid: PlayerId) {
    state.modify_player(pid, |ecs, entity| {
        if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) {
            ui.current_window = None;
        }
        Some(())
    });
}

async fn handle_open_prog(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    prog_id: i32,
) {
    let p = match state.db.get_program(prog_id).await {
        Ok(Some(program)) => program,
        Ok(None) => {
            send_programmator_error(tx, "Программа не найдена.");
            return;
        }
        Err(e) => {
            tracing::error!(player_id = %pid, program_id = prog_id, error = ?e, "DB get failed for openprog");
            send_programmator_error(tx, "Не удалось прочитать программу.");
            return;
        }
    };
    if p.player_id != pid.as_i32() {
        tracing::warn!(
            player_id = %pid,
            program_id = prog_id,
            owner_id = p.player_id,
            "Rejected foreign program open"
        );
        send_programmator_error(tx, "Программа недоступна.");
        return;
    }
    if let Err(e) = state.db.set_selected_program(pid.into(), Some(p.id)).await {
        tracing::error!(player_id = %pid, program_id = p.id, error = ?e, "DB selected program update failed for openprog");
        send_programmator_error(tx, "Не удалось выбрать программу.");
        return;
    }
    state.modify_player(pid, |ecs, entity| {
        if let Some(mut ps) = ecs.get_mut::<crate::game::programmator::ProgrammatorState>(entity) {
            ps.selected_id = Some(p.id);
            ps.selected_data = Some(p.code.clone());
        }
        if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) {
            ui.current_window = None;
        }
        Some(())
    });
    // C# `StaticGUI.OpenProg`: `win = null` (→ `Gu` закрыть список) → `OpenProg` (#P).
    // Без `Gu` окно-список программ не закрывалось поверх редактора.
    send_u_packet(tx, "Gu", &crate::protocol::packets::gu_close().1);
    send_u_packet(
        tx,
        "#P",
        &crate::protocol::packets::open_programmator(p.id, &p.name, &p.code).1,
    );
}

async fn handle_create_prog(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    name: &str,
) {
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    match state.db.insert_program(pid.into(), name, "").await {
        Ok(prog_id) => {
            if let Err(e) = state
                .db
                .set_selected_program(pid.into(), Some(prog_id))
                .await
            {
                tracing::error!(player_id = %pid, program_id = prog_id, error = ?e, "DB selected program update failed for createprog");
                send_programmator_error(tx, "Не удалось выбрать программу.");
                return;
            }
            state.modify_player(pid, |ecs, entity| {
                if let Some(mut ps) =
                    ecs.get_mut::<crate::game::programmator::ProgrammatorState>(entity)
                {
                    ps.selected_id = Some(prog_id);
                    ps.selected_data = Some(String::new());
                }
                if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) {
                    ui.current_window = None;
                }
                Some(())
            });
            // C# `NewProg`: `win = null` (→ `Gu`) перед открытием редактора (#P).
            send_u_packet(tx, "Gu", &crate::protocol::packets::gu_close().1);
            send_u_packet(
                tx,
                "#P",
                &crate::protocol::packets::open_programmator(prog_id, name, "").1,
            );
        }
        Err(e) => {
            tracing::error!(player_id = %pid, error = ?e, "DB insert failed for createprog");
            send_programmator_error(tx, "Не удалось создать программу.");
        }
    }
}

async fn handle_rename_prog(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    prog_id: i32,
    name: &str,
) {
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    let p = match state.db.get_program(prog_id).await {
        Ok(Some(program)) => program,
        Ok(None) => {
            send_programmator_error(tx, "Программа не найдена.");
            return;
        }
        Err(e) => {
            tracing::error!(player_id = %pid, program_id = prog_id, error = ?e, "DB get failed for rename program");
            send_programmator_error(tx, "Не удалось прочитать программу.");
            return;
        }
    };
    if p.player_id != pid.as_i32() {
        tracing::warn!(
            player_id = %pid,
            program_id = prog_id,
            owner_id = p.player_id,
            "Rejected foreign program rename"
        );
        send_programmator_error(tx, "Программа недоступна.");
        return;
    }
    if let Err(e) = state.db.rename_program(prog_id, name).await {
        tracing::error!(player_id = %pid, program_id = prog_id, error = ?e, "DB rename failed for program");
        send_programmator_error(tx, "Не удалось переименовать программу.");
        return;
    }
    clear_programmator_window(state, pid);
    send_u_packet(
        tx,
        "#p",
        &crate::protocol::packets::open_programmator(prog_id, name, &p.code).1,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    struct CraftTestState {
        state: Arc<GameState>,
        player: crate::db::PlayerRow,
        db_path: std::path::PathBuf,
        world_name: String,
        dir: std::path::PathBuf,
    }

    impl CraftTestState {
        fn cleanup(&self) {
            let _ = std::fs::remove_file(&self.db_path);
            let _ = std::fs::remove_file(self.db_path.with_extension("db-wal"));
            let _ = std::fs::remove_file(self.db_path.with_extension("db-shm"));
            let _ = std::fs::remove_file(self.dir.join(format!("{}_v2.map", self.world_name)));
            let _ = std::fs::remove_file(
                self.dir
                    .join(format!("{}_durability.mapb", self.world_name)),
            );
        }
    }

    #[test]
    fn parse_six_i64_fields_accepts_exact_six_values() {
        assert_eq!(
            parse_six_i64_fields("1:2:3:4:5:6"),
            Some([1, 2, 3, 4, 5, 6])
        );
        assert_eq!(
            parse_six_i64_fields("-1:0:10:20:30:40"),
            Some([-1, 0, 10, 20, 30, 40])
        );
    }

    #[test]
    fn parse_six_i64_fields_rejects_partial_or_malformed_values() {
        assert_eq!(parse_six_i64_fields("1:2:3:4:5"), None);
        assert_eq!(parse_six_i64_fields("1:2:3:4:5:6:7"), None);
        assert_eq!(parse_six_i64_fields("1:x:2:3:4:5:6"), None);
        assert_eq!(parse_six_i64_fields("1::2:3:4:5"), None);
    }

    #[test]
    fn parse_rich_key_values_rejects_malformed_pairs() {
        let parsed = parse_rich_key_values("cost:10#clan:1").unwrap();
        assert_eq!(parsed.get("cost"), Some(&"10"));
        assert_eq!(parsed.get("clan"), Some(&"1"));
        assert!(parse_rich_key_values("cost").is_none());
        assert!(parse_rich_key_values("cost:").is_none());
        assert!(parse_rich_key_values(":10").is_none());
    }

    #[test]
    fn parse_settings_pairs_accepts_unity_richlist_payload() {
        let parsed = parse_settings_pairs("isca:1#mous:0#").unwrap();
        assert_eq!(parsed.get("isca"), Some(&"1"));
        assert_eq!(parsed.get("mous"), Some(&"0"));
    }

    #[test]
    fn parse_settings_pairs_rejects_missing_or_empty_fields() {
        assert!(parse_settings_pairs("").is_none());
        assert!(parse_settings_pairs("isca").is_none());
        assert!(parse_settings_pairs("isca:").is_none());
        assert!(parse_settings_pairs(":1").is_none());
        assert!(parse_settings_pairs("isca:1##mous:0").is_none());
    }

    #[test]
    fn parse_settings_pairs_rejects_legacy_equals_comma_payload() {
        assert!(parse_settings_pairs("isca=1,mous=0").is_none());
    }

    #[test]
    fn parse_rich_bool_accepts_only_explicit_bool_values() {
        assert_eq!(parse_rich_bool("1"), Some(true));
        assert_eq!(parse_rich_bool("true"), Some(true));
        assert_eq!(parse_rich_bool("0"), Some(false));
        assert_eq!(parse_rich_bool("false"), Some(false));
        assert_eq!(parse_rich_bool("yes"), None);
    }

    #[tokio::test]
    async fn programmator_open_button_clears_server_window_state() {
        let test = make_craft_test_state("prog_open_window_state", 10, 10).await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let prog_id = test
            .state
            .db
            .insert_program(test.player.id, "main", "source")
            .await
            .unwrap();
        let pid = PlayerId(test.player.id);
        test.state.modify_player(pid, |ecs, entity| {
            ecs.get_mut::<PlayerUI>(entity)?.current_window = Some("prog".to_string());
            Some(())
        });

        handle_gui_button(&test.state, &tx, pid, &format!("openprog:{prog_id}")).await;

        let events = drain_events(&mut rx);
        let names: Vec<&str> = events.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, vec!["Gu", "#P", "Gu"]);
        assert_eq!(current_window(&test.state, pid), None);

        test.cleanup();
    }

    #[tokio::test]
    async fn programmator_create_button_clears_server_window_state() {
        let test = make_craft_test_state("prog_create_window_state", 10, 10).await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        test.state.modify_player(pid, |ecs, entity| {
            ecs.get_mut::<PlayerUI>(entity)?.current_window = Some("prog".to_string());
            Some(())
        });

        handle_gui_button(&test.state, &tx, pid, "createprog:main").await;

        let events = drain_events(&mut rx);
        let names: Vec<&str> = events.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, vec!["Gu", "#P", "Gu"]);
        assert_eq!(current_window(&test.state, pid), None);

        test.cleanup();
    }

    #[tokio::test]
    async fn programmator_rename_confirms_with_update_packet_and_closes_horb() {
        let test = make_craft_test_state("prog_rename_window_state", 10, 10).await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let prog_id = test
            .state
            .db
            .insert_program(test.player.id, "old", "source")
            .await
            .unwrap();
        let pid = PlayerId(test.player.id);
        test.state.modify_player(pid, |ecs, entity| {
            ecs.get_mut::<PlayerUI>(entity)?.current_window = Some(format!("pren:{prog_id}"));
            Some(())
        });

        handle_gui_button(&test.state, &tx, pid, &format!("rename:{prog_id}:new")).await;

        let events = drain_events(&mut rx);
        let names: Vec<&str> = events.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, vec!["#p", "Gu"]);
        assert_eq!(current_window(&test.state, pid), None);
        let update_json: serde_json::Value = serde_json::from_slice(&events[0].1).unwrap();
        assert_eq!(update_json["id"], prog_id);
        assert_eq!(update_json["title"], "new");
        assert_eq!(update_json["source"], "source");

        test.cleanup();
    }

    #[tokio::test]
    async fn settings_save_missing_player_flags_is_explicit_error_without_settings_mutation() {
        let test = make_craft_test_state("settings_missing_player_flags", 10, 10).await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut settings = ecs
                .get_mut::<crate::game::player::PlayerSettings>(entity)
                .unwrap();
            settings.isca = 1;
            settings.mous = false;
            ecs.entity_mut(entity).remove::<PlayerFlags>();
        }

        handle_settings_save(&test.state, &tx, pid, "isca:5#mous:1#");

        let saved_settings = test
            .state
            .query_player_opt(pid, |ecs, entity| {
                Some(
                    ecs.get::<crate::game::player::PlayerSettings>(entity)?
                        .to_owned(),
                )
            })
            .unwrap();
        assert_eq!(saved_settings.isca, 1);
        assert!(!saved_settings.mous);
        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert_eq!(
            events[0].1,
            "НАСТРОЙКИ#Состояние настроек недоступно.".as_bytes()
        );
        assert!(!events.iter().any(|(event, _)| event == "#S"));

        test.cleanup();
    }

    #[tokio::test]
    async fn craft_start_rejects_remote_player_without_deducting_resources() {
        let test = make_craft_test_state("remote", 5, 5).await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        handle_craft_start(&test.state, &tx, test.player.id.into(), "0:1:10:10");

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert_eq!(events[0].1, "Недостаточно ресов#...".as_bytes());

        let state_after = craft_state(&test.state, 10, 10);
        assert_eq!(state_after, (None, 0, 0));

        let crystals = player_crystals(&test.state, test.player.id.into());
        assert_eq!(crystals[0], 100);

        test.cleanup();
    }

    #[tokio::test]
    async fn craft_start_rejects_missing_crafting_component_without_deducting_resources() {
        let test = make_craft_test_state("missing_component", 10, 10).await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let entity = *test.state.building_index.get(&((10, 10).into())).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity).remove::<BuildingCrafting>();
        }

        handle_craft_start(&test.state, &tx, test.player.id.into(), "0:1:10:10");

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert_eq!(events[0].1, "КРАФТЕР#Некорректное действие.".as_bytes());

        let crystals = player_crystals(&test.state, test.player.id.into());
        assert_eq!(crystals[0], 100);

        test.cleanup();
    }

    #[tokio::test]
    async fn craft_start_missing_player_flags_is_explicit_error_without_deducting_resources() {
        let test = make_craft_test_state("missing_player_flags", 10, 10).await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let entity = test.state.get_player_entity(test.player.id.into()).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity).remove::<PlayerFlags>();
        }

        handle_craft_start(&test.state, &tx, test.player.id.into(), "0:1:10:10");

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert_eq!(
            events[0].1,
            "КРАФТЕР#Состояние крафтера недоступно.".as_bytes()
        );
        assert_eq!(player_crystals(&test.state, test.player.id.into())[0], 100);
        assert_eq!(craft_state(&test.state, 10, 10), (None, 0, 0));

        test.cleanup();
    }

    #[tokio::test]
    async fn craft_start_on_crafter_origin_deducts_and_starts_recipe() {
        let test = make_craft_test_state("local", 10, 10).await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        handle_craft_start(&test.state, &tx, test.player.id.into(), "0:1:10:10");

        let (recipe_id, num, end_ts) = craft_state(&test.state, 10, 10);
        assert_eq!(recipe_id, Some(0));
        assert_eq!(num, 1);
        assert!(end_ts > 0);

        let crystals = player_crystals(&test.state, test.player.id.into());
        assert_eq!(crystals[0], 50);

        test.cleanup();
    }

    #[tokio::test]
    async fn craft_claim_clears_crafter_before_second_claim_can_duplicate_reward() {
        let test = make_craft_test_state("claim_once", 10, 10).await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        handle_craft_start(&test.state, &tx, test.player.id.into(), "0:1:10:10");
        {
            let entity = *test.state.building_index.get(&((10, 10).into())).unwrap();
            let mut ecs = test.state.ecs.write();
            let mut craft = ecs.get_mut::<BuildingCrafting>(entity).unwrap();
            craft.end_ts = 0;
        }
        drain_events(&mut rx);

        handle_craft_claim(&test.state, &tx, test.player.id.into(), "10:10");
        handle_craft_claim(&test.state, &tx, test.player.id.into(), "10:10");

        assert_eq!(
            player_inventory_count(&test.state, test.player.id.into(), 0),
            1
        );
        assert_eq!(craft_state(&test.state, 10, 10), (None, 0, 0));

        test.cleanup();
    }

    #[tokio::test]
    async fn craft_claim_missing_building_flags_is_explicit_error_without_reward_or_clear() {
        let test = make_craft_test_state("claim_missing_flags", 10, 10).await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        handle_craft_start(&test.state, &tx, test.player.id.into(), "0:1:10:10");
        {
            let entity = *test.state.building_index.get(&((10, 10).into())).unwrap();
            let mut ecs = test.state.ecs.write();
            let mut craft = ecs.get_mut::<BuildingCrafting>(entity).unwrap();
            craft.end_ts = 0;
            ecs.entity_mut(entity).remove::<BuildingFlags>();
        }
        drain_events(&mut rx);

        handle_craft_claim(&test.state, &tx, test.player.id.into(), "10:10");

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert_eq!(
            events[0].1,
            "КРАФТЕР#Состояние крафтера недоступно.".as_bytes()
        );
        assert_eq!(
            player_inventory_count(&test.state, test.player.id.into(), 0),
            0
        );
        assert_eq!(craft_state(&test.state, 10, 10).0, Some(0));

        test.cleanup();
    }

    #[tokio::test]
    async fn market_sell_missing_building_flags_is_explicit_error_without_money_or_crystal_mutation()
     {
        let test = make_market_test_state("sell_missing_building_flags").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let building_entity = *test.state.building_index.get(&((10, 10).into())).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(building_entity).remove::<BuildingFlags>();
        }
        let before_money = player_money(&test.state, test.player.id.into());
        let before_crystals = player_crystals(&test.state, test.player.id.into());

        do_market_sell(
            &test.state,
            &tx,
            test.player.id.into(),
            &[10, 0, 0, 0, 0, 0],
            10,
            10,
        );

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert_eq!(
            events[0].1,
            "МАРКЕТ#Состояние маркета недоступно.".as_bytes()
        );
        assert_eq!(
            player_money(&test.state, test.player.id.into()),
            before_money
        );
        assert_eq!(
            player_crystals(&test.state, test.player.id.into()),
            before_crystals
        );

        test.cleanup();
    }

    #[tokio::test]
    async fn market_buy_missing_player_flags_is_explicit_error_without_money_or_crystal_mutation() {
        let test = make_market_test_state("buy_missing_player_flags").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let player_entity = test.state.get_player_entity(test.player.id.into()).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut ui = ecs.get_mut::<PlayerUI>(player_entity).unwrap();
            ui.current_window = Some("market:10:10:buycrys".to_string());
            ecs.entity_mut(player_entity).remove::<PlayerFlags>();
        }
        let before_money = player_money(&test.state, test.player.id.into());
        let before_crystals = player_crystals(&test.state, test.player.id.into());

        handle_market_buy(&test.state, &tx, test.player.id.into(), "1:0:0:0:0:0");

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert_eq!(
            events[0].1,
            "МАРКЕТ#Состояние маркета недоступно.".as_bytes()
        );
        assert_eq!(
            player_money(&test.state, test.player.id.into()),
            before_money
        );
        assert_eq!(
            player_crystals(&test.state, test.player.id.into()),
            before_crystals
        );

        test.cleanup();
    }

    #[tokio::test]
    async fn market_getprofit_missing_building_flags_is_explicit_error_without_profit_mutation() {
        let test = make_market_test_state("getprofit_missing_building_flags").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let player_entity = test.state.get_player_entity(test.player.id.into()).unwrap();
        let building_entity = *test.state.building_index.get(&((10, 10).into())).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut ui = ecs.get_mut::<PlayerUI>(player_entity).unwrap();
            ui.current_window = Some("market:10:10:admin".to_string());
            let mut storage = ecs.get_mut::<BuildingStorage>(building_entity).unwrap();
            storage.money = 777;
            ecs.entity_mut(building_entity).remove::<BuildingFlags>();
        }
        let before_money = player_money(&test.state, test.player.id.into());

        handle_market_getprofit(&test.state, &tx, test.player.id.into());

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert_eq!(
            events[0].1,
            "МАРКЕТ#Состояние маркета недоступно.".as_bytes()
        );
        assert_eq!(
            player_money(&test.state, test.player.id.into()),
            before_money
        );
        assert_eq!(market_storage_money(&test.state, 10, 10), 777);

        test.cleanup();
    }

    #[tokio::test]
    async fn pack_take_money_missing_player_flags_is_explicit_error_without_storage_mutation() {
        let test = make_market_test_state("pack_take_money_missing_player_flags").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let player_entity = test.state.get_player_entity(test.player.id.into()).unwrap();
        let building_entity = *test.state.building_index.get(&((10, 10).into())).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut storage = ecs.get_mut::<BuildingStorage>(building_entity).unwrap();
            storage.money = 777;
            ecs.entity_mut(player_entity).remove::<PlayerFlags>();
        }
        let view = test.state.get_pack_at(10, 10).unwrap();
        let before_money = player_money(&test.state, test.player.id.into());

        handle_pack_take_money(&test.state, &tx, test.player.id.into(), &view);

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert_eq!(
            events[0].1,
            "ЗДАНИЕ#Состояние здания недоступно.".as_bytes()
        );
        assert_eq!(
            player_money(&test.state, test.player.id.into()),
            before_money
        );
        assert_eq!(market_storage_money(&test.state, 10, 10), 777);

        test.cleanup();
    }

    #[tokio::test]
    async fn pack_take_crystals_missing_player_flags_is_explicit_error_without_storage_mutation() {
        let test = make_market_test_state("pack_take_crystals_missing_player_flags").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let player_entity = test.state.get_player_entity(test.player.id.into()).unwrap();
        let building_entity = *test.state.building_index.get(&((10, 10).into())).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut storage = ecs.get_mut::<BuildingStorage>(building_entity).unwrap();
            storage.crystals = [7, 6, 5, 4, 3, 2];
            ecs.entity_mut(player_entity).remove::<PlayerFlags>();
        }
        let view = test.state.get_pack_at(10, 10).unwrap();
        let before_crystals = player_crystals(&test.state, test.player.id.into());

        handle_pack_take_crystals(&test.state, &tx, test.player.id.into(), &view);

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert_eq!(
            events[0].1,
            "ЗДАНИЕ#Состояние здания недоступно.".as_bytes()
        );
        assert_eq!(
            player_crystals(&test.state, test.player.id.into()),
            before_crystals
        );
        assert_eq!(
            market_storage_crystals(&test.state, 10, 10),
            [7, 6, 5, 4, 3, 2]
        );

        test.cleanup();
    }

    #[tokio::test]
    async fn storage_transfer_missing_player_flags_is_explicit_error_without_crystal_mutation() {
        let test = make_storage_test_state("storage_transfer_missing_player_flags").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let player_entity = test.state.get_player_entity(test.player.id.into()).unwrap();
        let building_entity = *test.state.building_index.get(&((10, 10).into())).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut ui = ecs.get_mut::<PlayerUI>(player_entity).unwrap();
            ui.current_window = Some("pack:10:10".to_string());
            let mut storage = ecs.get_mut::<BuildingStorage>(building_entity).unwrap();
            storage.crystals = [10, 0, 0, 0, 0, 0];
            ecs.entity_mut(player_entity).remove::<PlayerFlags>();
        }
        let before_player = player_crystals(&test.state, test.player.id.into());

        handle_storage_transfer(&test.state, &tx, test.player.id.into(), "50:0:0:0:0:0");

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert_eq!(
            events[0].1,
            "ЗДАНИЕ#Состояние здания недоступно.".as_bytes()
        );
        assert_eq!(
            player_crystals(&test.state, test.player.id.into()),
            before_player
        );
        assert_eq!(
            market_storage_crystals(&test.state, 10, 10),
            [10, 0, 0, 0, 0, 0]
        );

        test.cleanup();
    }

    async fn make_craft_test_state(label: &str, player_x: i32, player_y: i32) -> CraftTestState {
        let dir = std::env::temp_dir();
        let nonce = format!("{}_{}_{}", label, std::process::id(), unique_test_nonce());
        let db_path = dir.join(format!("craft_start_{nonce}.db"));
        let _ = std::fs::remove_file(&db_path);

        let database = crate::db::Database::open(&db_path).await.unwrap();
        let mut player = database
            .create_player("craft-user", "p", "h")
            .await
            .unwrap();
        player.x = player_x;
        player.y = player_y;
        player.crystals[0] = 100;

        let extra = crate::db::BuildingExtra {
            hp: 1000,
            max_hp: 1000,
            ..crate::db::BuildingExtra::default()
        };
        let _building_id = database
            .insert_building("F", 10, 10, player.id, 0, &extra)
            .await
            .unwrap();

        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();
        let world_name = format!("craft_start_world_{nonce}");
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
        let _ = crate::game::buildings::load_buildings_config(crate::test_config_path(
            "configs/buildings.json",
        ));
        let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config)
            .await
            .unwrap();

        CraftTestState {
            state,
            player,
            db_path,
            world_name,
            dir,
        }
    }

    async fn make_market_test_state(label: &str) -> CraftTestState {
        let dir = std::env::temp_dir();
        let nonce = format!("{}_{}_{}", label, std::process::id(), unique_test_nonce());
        let db_path = dir.join(format!("market_{nonce}.db"));
        let _ = std::fs::remove_file(&db_path);

        let database = crate::db::Database::open(&db_path).await.unwrap();
        let mut player = database
            .create_player("market-user", "p", "h")
            .await
            .unwrap();
        player.x = 10;
        player.y = 10;
        player.money = 10_000;
        player.crystals[0] = 100;

        let extra = crate::db::BuildingExtra {
            hp: 1000,
            max_hp: 1000,
            money_inside: 0,
            ..crate::db::BuildingExtra::default()
        };
        database
            .insert_building("M", 10, 10, player.id, 0, &extra)
            .await
            .unwrap();

        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();
        let world_name = format!("market_world_{nonce}");
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
        let _ = crate::game::buildings::load_buildings_config(crate::test_config_path(
            "configs/buildings.json",
        ));
        let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config)
            .await
            .unwrap();

        CraftTestState {
            state,
            player,
            db_path,
            world_name,
            dir,
        }
    }

    async fn make_storage_test_state(label: &str) -> CraftTestState {
        let dir = std::env::temp_dir();
        let nonce = format!("{}_{}_{}", label, std::process::id(), unique_test_nonce());
        let db_path = dir.join(format!("storage_{nonce}.db"));
        let _ = std::fs::remove_file(&db_path);

        let database = crate::db::Database::open(&db_path).await.unwrap();
        let mut player = database
            .create_player("storage-user", "p", "h")
            .await
            .unwrap();
        player.x = 10;
        player.y = 10;
        player.money = 10_000;
        player.crystals[0] = 100;

        let extra = crate::db::BuildingExtra {
            hp: 1000,
            max_hp: 1000,
            money_inside: 0,
            ..crate::db::BuildingExtra::default()
        };
        database
            .insert_building("L", 10, 10, player.id, 0, &extra)
            .await
            .unwrap();

        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();
        let world_name = format!("storage_world_{nonce}");
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
        let _ = crate::game::buildings::load_buildings_config(crate::test_config_path(
            "configs/buildings.json",
        ));
        let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config)
            .await
            .unwrap();

        CraftTestState {
            state,
            player,
            db_path,
            world_name,
            dir,
        }
    }

    fn craft_state(state: &Arc<GameState>, bx: i32, by: i32) -> (Option<i32>, i32, i64) {
        state
            .building_index
            .get(&((bx, by).into()))
            .and_then(|ent| {
                let ecs = state.ecs.read();
                let craft = ecs.get::<BuildingCrafting>(*ent)?;
                Some((craft.recipe_id, craft.num, craft.end_ts))
            })
            .unwrap()
    }

    fn player_crystals(state: &Arc<GameState>, pid: PlayerId) -> [i64; 6] {
        state
            .query_player_opt(pid, |ecs, entity| {
                Some(ecs.get::<PlayerStats>(entity)?.crystals)
            })
            .unwrap()
    }

    fn player_inventory_count(state: &Arc<GameState>, pid: PlayerId, item_id: i32) -> i32 {
        state
            .query_player_opt(pid, |ecs, entity| {
                Some(
                    ecs.get::<PlayerInventory>(entity)?
                        .items
                        .get(&item_id)
                        .copied()
                        .unwrap_or(0),
                )
            })
            .unwrap()
    }

    fn player_money(state: &Arc<GameState>, pid: PlayerId) -> i64 {
        state
            .query_player_opt(pid, |ecs, entity| {
                Some(ecs.get::<PlayerStats>(entity)?.money)
            })
            .unwrap()
    }

    fn market_storage_money(state: &Arc<GameState>, bx: i32, by: i32) -> i64 {
        state
            .building_index
            .get(&((bx, by).into()))
            .and_then(|ent| {
                let ecs = state.ecs.read();
                Some(ecs.get::<BuildingStorage>(*ent)?.money)
            })
            .unwrap()
    }

    fn market_storage_crystals(state: &Arc<GameState>, bx: i32, by: i32) -> [i64; 6] {
        state
            .building_index
            .get(&((bx, by).into()))
            .and_then(|ent| {
                let ecs = state.ecs.read();
                Some(ecs.get::<BuildingStorage>(*ent)?.crystals)
            })
            .unwrap()
    }

    fn drain_events(rx: &mut mpsc::UnboundedReceiver<Vec<u8>>) -> Vec<(String, Vec<u8>)> {
        let mut events = Vec::new();
        while let Ok(frame) = rx.try_recv() {
            let mut buf = BytesMut::from(&frame[..]);
            let packet = crate::protocol::Packet::try_decode(&mut buf)
                .expect("valid packet")
                .expect("decoded packet");
            events.push((packet.event_str().to_owned(), packet.payload.to_vec()));
        }
        events
    }

    fn current_window(state: &Arc<GameState>, pid: PlayerId) -> Option<String> {
        state.query_player_opt(pid, |ecs, entity| {
            Some(ecs.get::<PlayerUI>(entity)?.current_window.clone())
        })?
    }

    fn unique_test_nonce() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }
}
