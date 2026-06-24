//! Обработка нажатий GUI-кнопок игроком.
use crate::game::buildings::{
    BuildingCrafting, BuildingMetadata, BuildingOwnership, BuildingStats, BuildingStorage,
    GridPosition,
};
use crate::game::crafting;
use crate::game::market;
use crate::game::player::{PlayerFlags, PlayerInventory, PlayerPosition, PlayerStats, PlayerUI};
use crate::net::session::outbound::inventory_sync::send_inventory;
use crate::net::session::prelude::*;
use crate::net::session::social::buildings::{broadcast_pack_update, modify_pack_with_db};

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
        if let Ok(item) = rest.parse::<i32>() {
            super::auction_gui::open_item_auc(state, tx, pid, item).await;
        }
    } else if let Some(rest) = button.strip_prefix("openorder:") {
        if let Ok(id) = rest.parse::<i32>() {
            super::auction_gui::open_order(state, tx, pid, id).await;
        }
    } else if let Some(rest) = button.strip_prefix("auccreate:") {
        if let Ok(item) = rest.parse::<i32>() {
            super::auction_gui::open_order_creation(state, tx, pid, item);
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
        }
    } else if let Some(rest) = button.strip_prefix("aucminbet:") {
        if let Ok(id) = rest.parse::<i32>() {
            super::auction_gui::place_minimal_bet(state, tx, pid, id).await;
        }
    } else if let Some(rest) = button.strip_prefix("aucbet:") {
        // aucbet:{id}:{amount}; невалидная сумма → просто переоткрыть ордер (1:1 C#).
        let parts: Vec<&str> = rest.splitn(2, ':').collect();
        if let [id, amount] = parts.as_slice() {
            if let Ok(id) = id.parse::<i32>() {
                if let Ok(amount) = amount.parse::<i64>() {
                    super::auction_gui::place_bet(state, tx, pid, id, amount).await;
                } else {
                    super::auction_gui::open_order(state, tx, pid, id).await;
                }
            }
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
        return;
    }
    let cmd = parts[0];
    let x = parts[1].parse::<i32>().unwrap_or(0);
    let y = parts[2].parse::<i32>().unwrap_or(0);

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
        if !view
            .pack_type
            .building_cells()
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
    let pstats_info = state.building_index.get(&(view.x, view.y)).and_then(|ent| {
        let ecs = state.ecs.read();
        let pstats = ecs.get::<BuildingStats>(*ent)?;
        Some((pstats.hp, pstats.max_hp))
    });
    let (hp, mhp) = pstats_info.unwrap_or((0, 0));

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
    let details = state.building_index.get(&(pack_x, pack_y)).and_then(|ent| {
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

    let mut fields: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for pair in richlist_data.split('#') {
        if let Some((k, v)) = pair.split_once(':') {
            fields.insert(k, v);
        }
    }
    let owner_clan = state
        .query_player_opt(pid, |ecs, e| {
            ecs.get::<PlayerStats>(e).and_then(|s| s.clan_id)
        })
        .unwrap_or(0);

    let _ = modify_pack_with_db(state, pack_x, pack_y, |ecs, entity| {
        if let Some(mut st) = ecs.get_mut::<BuildingStats>(entity) {
            if let Some(c) = fields.get("cost").and_then(|s| s.parse::<i32>().ok()) {
                if (0..=5000).contains(&c) {
                    st.cost = c;
                }
            }
        }
        if let Some(mut own) = ecs.get_mut::<BuildingOwnership>(entity) {
            if let Some(clan) = fields.get("clan") {
                own.clan_id = if *clan == "1" { owner_clan } else { 0 };
            }
        }
    });

    open_pack_admin_gui(state, tx, pid, pack_x, pack_y);
}

fn handle_pack_take_money(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    view: &PackView,
) {
    let mut amount = 0i64;
    if modify_pack_with_db(state, view.x, view.y, |ecs, entity| {
        if let Some(mut s) = ecs.get_mut::<BuildingStorage>(entity) {
            amount = s.money;
            s.money = 0;
        }
    })
    .is_ok()
    {
        if amount > 0 {
            state.modify_player(pid, |ecs, entity| {
                // B2: пометить dirty (см. do_market_sell) — pack take тоже мутирует деньги.
                if let Some(mut f) = ecs.get_mut::<PlayerFlags>(entity) {
                    f.dirty = true;
                }
                let mut s = ecs.get_mut::<PlayerStats>(entity)?;
                s.money += amount;
                send_u_packet(tx, "P$", &money(s.money, s.creds).1);
                Some(())
            });
        }
    }
}

fn handle_pack_take_crystals(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    view: &PackView,
) {
    let mut amount = [0i64; 6];
    if modify_pack_with_db(state, view.x, view.y, |ecs, entity| {
        if let Some(mut s) = ecs.get_mut::<BuildingStorage>(entity) {
            amount = s.crystals;
            s.crystals = [0; 6];
        }
    })
    .is_ok()
    {
        if amount.iter().sum::<i64>() > 0 {
            state.modify_player(pid, |ecs, entity| {
                // B2: пометить dirty (см. do_market_sell) — pack take кристаллов.
                if let Some(mut f) = ecs.get_mut::<PlayerFlags>(entity) {
                    f.dirty = true;
                }
                let mut s = ecs.get_mut::<PlayerStats>(entity)?;
                for i in 0..6 {
                    s.crystals[i] += amount[i];
                }
                send_u_packet(tx, "@B", &basket(&s.crystals, 1).1);
                Some(())
            });
        }
    }
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
        .get(&(view.x, view.y))
        .and_then(|ent| {
            let ecs = state.ecs.read();
            let s = ecs.get::<BuildingStorage>(*ent)?;
            Some(s.crystals)
        })
        .unwrap_or([0; 6]);

    // Fetch player crystals
    let player_crys = state
        .query_player_opt(pid, |ecs, entity| {
            ecs.get::<PlayerStats>(entity).map(|s| s.crystals)
        })
        .unwrap_or([0; 6]);

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
    // Parse 6 colon-separated i64 values
    let sliders: Vec<i64> = slider_data
        .split(':')
        .filter_map(|s| s.parse().ok())
        .collect();
    if sliders.len() != 6 {
        return;
    }

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

    // Atomic read-validate-write: single ecs.write() lock covers both storage and
    // player — prevents TOCTOU crystal duplication by concurrent clan members.
    let result = modify_pack_with_db(state, bx, by, |ecs, building_entity| {
        let storage_crys = ecs.get::<BuildingStorage>(building_entity)?.crystals;
        let player_crys = ecs.get::<PlayerStats>(player_entity)?.crystals;

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

        ecs.get_mut::<BuildingStorage>(building_entity)?.crystals = new_storage;
        ecs.get_mut::<PlayerStats>(player_entity)?.crystals = new_player;
        if let Some(mut f) = ecs.get_mut::<PlayerFlags>(player_entity) {
            f.dirty = true;
        }
        Some(new_player)
    });

    let Ok(Some(new_player)) = result else {
        return;
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

    let craft_state = state.building_index.get(&(view.x, view.y)).and_then(|ent| {
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
        return;
    }
    let recipe_id = parts[0].parse::<i32>().unwrap_or(-1);
    let bx = parts[1].parse::<i32>().unwrap_or(0);
    let by = parts[2].parse::<i32>().unwrap_or(0);

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
        return;
    }
    let recipe_id = parts[0].parse::<i32>().unwrap_or(-1);
    let num = parts[1].parse::<i32>().unwrap_or(0).max(1);
    let bx = parts[2].parse::<i32>().unwrap_or(0);
    let by = parts[3].parse::<i32>().unwrap_or(0);

    let Some(recipe) = crafting::recipe_by_id(recipe_id) else {
        return;
    };

    let Some(view) = state.get_pack_at(bx, by) else {
        return;
    };
    if view.pack_type != PackType::Craft || view.owner_id != pid {
        return;
    }

    let already_crafting = state
        .building_index
        .get(&(bx, by))
        .and_then(|ent| {
            let ecs = state.ecs.read();
            let c = ecs.get::<BuildingCrafting>(*ent)?;
            Some(c.recipe_id.is_some())
        })
        .unwrap_or(false);
    if already_crafting {
        send_u_packet(tx, "OK", &ok_message("Крафтер", "Крафт уже запущен").1);
        return;
    }

    let deducted = state
        .modify_player(pid, |ecs, entity| {
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

            Some(true)
        })
        .flatten()
        .unwrap_or(false);

    if !deducted {
        send_u_packet(tx, "OK", &ok_message("Крафтер", "Недостаточно ресурсов").1);
        return;
    }

    let end_ts = now_ts() + i64::from(recipe.time_sec) * i64::from(num);
    let _ = modify_pack_with_db(state, bx, by, |ecs, entity| {
        if let Some(mut c) = ecs.get_mut::<BuildingCrafting>(entity) {
            c.recipe_id = Some(recipe_id);
            c.num = num;
            c.end_ts = end_ts;
        }
    });

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
        return;
    }
    let bx = parts[0].parse::<i32>().unwrap_or(0);
    let by = parts[1].parse::<i32>().unwrap_or(0);

    let Some(view) = state.get_pack_at(bx, by) else {
        return;
    };
    if view.pack_type != PackType::Craft || view.owner_id != pid {
        return;
    }

    let craft_info = state.building_index.get(&(bx, by)).and_then(|ent| {
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

    state.modify_player(pid, |ecs, entity| {
        let mut inv = ecs.get_mut::<PlayerInventory>(entity)?;
        let entry = inv.items.entry(recipe.result.id).or_insert(0);
        *entry += recipe.result.num * num;
        send_inventory(tx, &mut inv);
        Some(())
    });

    let _ = modify_pack_with_db(state, bx, by, |ecs, entity| {
        if let Some(mut c) = ecs.get_mut::<BuildingCrafting>(entity) {
            c.recipe_id = None;
            c.num = 0;
            c.end_ts = 0;
        }
    });

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
                if pstats.charge <= 0.0 {
                    return None;
                }
                Some((pos.x, pos.y))
            })
            .collect()
    };

    use super::horb::{Button, Horb};

    let pstats_info = state.building_index.get(&(view.x, view.y)).and_then(|ent| {
        let ecs = state.ecs.read();
        let pstats = ecs.get::<BuildingStats>(*ent)?;
        Some((pstats.hp, pstats.max_hp))
    });
    let (hp, mhp) = pstats_info.unwrap_or((0, 0));

    let text = if nearby_tps.is_empty() {
        format!(
            "Заряд: {:.0}\nПрочность: {}/{}\n\nНет доступных телепортов поблизости.",
            view.charge, hp, mhp
        )
    } else {
        format!(
            "Заряд: {:.0}\nПрочность: {}/{}\n\nДоступные телепорты:",
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
        tracing::warn!(pid, "TP action: destination {dest_x}:{dest_y} not found");
        return;
    };
    if dest_view.pack_type != PackType::Teleport || dest_view.charge <= 0.0 {
        tracing::warn!(pid, "TP action: destination not a valid teleport");
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
        tracing::warn!(pid, "TP action: player not at a teleport window");
        return;
    };

    let Some(src_view) = state.get_pack_at(src_x, src_y) else {
        return;
    };
    if src_view.pack_type != PackType::Teleport || src_view.charge <= 0.0 {
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
        pid,
        "Teleported from ({src_x},{src_y}) to ({dest_x},{tp_y})"
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
    let sliders: Vec<i64> = slider_data
        .split(':')
        .filter_map(|s| s.parse().ok())
        .collect();
    if sliders.len() != 6 {
        return;
    }

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
    let player_crys = state
        .query_player_opt(pid, |ecs, entity| {
            ecs.get::<PlayerStats>(entity).map(|s| s.crystals)
        })
        .unwrap_or([0; 6]);

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
    let mut total_money: i64 = 0;

    // Deduct crystals from player, compute money earned
    state.modify_player(pid, |ecs, entity| {
        // B2: пометить dirty — иначе периодический 10s-сейв (только dirty) пропускает
        // сделку, и при краше до дисконнекта деньги/кристаллы теряются (как auction.rs).
        if let Some(mut f) = ecs.get_mut::<PlayerFlags>(entity) {
            f.dirty = true;
        }
        let mut pstats = ecs.get_mut::<PlayerStats>(entity)?;
        for i in 0..6 {
            let to_sell = sliders[i];
            if to_sell <= 0 {
                continue;
            }
            // C# RemoveCrys: only succeeds if player has enough
            if pstats.crystals[i] >= to_sell {
                let price = market::get_crystal_cost(state, i);
                let Some(earned) = to_sell.checked_mul(price) else {
                    continue;
                };
                pstats.crystals[i] -= to_sell;
                total_money = total_money.saturating_add(earned);
            }
        }
        // Add money to player
        pstats.money = pstats.money.saturating_add(total_money);
        // Send updates
        send_u_packet(tx, "@B", &basket(&pstats.crystals, 1).1);
        send_u_packet(tx, "P$", &money(pstats.money, pstats.creds).1);
        Some(())
    });

    // Market owner gets 10% commission (C# ref: m.moneyinside += (long)(money * 0.1))
    if total_money > 0 {
        let commission = total_money / 10;
        let _ = modify_pack_with_db(state, bx, by, |ecs, entity| {
            if let Some(mut storage) = ecs.get_mut::<BuildingStorage>(entity) {
                storage.money += commission;
            }
        });
    }

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
    let sliders: Vec<i64> = slider_data
        .split(':')
        .filter_map(|s| s.parse().ok())
        .collect();
    if sliders.len() != 6 {
        return;
    }

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
    state.modify_player(pid, |ecs, entity| {
        // B2: пометить dirty (см. do_market_sell) — иначе сделка теряется при краше.
        if let Some(mut f) = ecs.get_mut::<PlayerFlags>(entity) {
            f.dirty = true;
        }
        let mut pstats = ecs.get_mut::<PlayerStats>(entity)?;
        for i in 0..6 {
            let to_buy = sliders[i];
            if to_buy <= 0 {
                continue;
            }
            // checked_mul: protect against overflow in release mode (wrapping mul could yield
            // negative cost, bypassing the affordability check and granting free crystals/money).
            let Some(cost) = to_buy.checked_mul(market::get_crystal_buy_price(state, i)) else {
                continue;
            };
            // C# ref: if p.money - (sliders[i] * World.GetCrysCost(i) * 10) < 0 continue
            if pstats.money < cost {
                continue;
            }
            pstats.money -= cost;
            pstats.crystals[i] = pstats.crystals[i].saturating_add(to_buy);
        }
        send_u_packet(tx, "@B", &basket(&pstats.crystals, 1).1);
        send_u_packet(tx, "P$", &money(pstats.money, pstats.creds).1);
        Some(())
    });

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

    // Transfer profit from building to player
    let mut amount = 0i64;
    let _ = modify_pack_with_db(state, bx, by, |ecs, entity| {
        if let Some(mut storage) = ecs.get_mut::<BuildingStorage>(entity) {
            amount = storage.money;
            storage.money = 0;
        }
    });

    if amount > 0 {
        state.modify_player(pid, |ecs, entity| {
            // B2: пометить dirty (см. do_market_sell).
            if let Some(mut f) = ecs.get_mut::<PlayerFlags>(entity) {
                f.dirty = true;
            }
            let mut s = ecs.get_mut::<PlayerStats>(entity)?;
            s.money += amount;
            send_u_packet(tx, "P$", &money(s.money, s.creds).1);
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
    let details = state.building_index.get(&(pack_x, pack_y)).and_then(|ent| {
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
    // RichList macro %R% substitutes to "key=val,key=val,..."
    let pairs: std::collections::HashMap<&str, &str> = data
        .split(',')
        .filter_map(|kv| kv.split_once('='))
        .collect();

    state.modify_player(pid, |ecs, entity| {
        let mut s = ecs.get_mut::<crate::game::player::PlayerSettings>(entity)?;
        if let Some(&v) = pairs.get("isca") {
            s.isca = v.parse().unwrap_or(s.isca);
        }
        if let Some(&v) = pairs.get("tsca") {
            s.tsca = v.parse().unwrap_or(s.tsca);
        }
        if let Some(&v) = pairs.get("mous") {
            s.mous = v == "1";
        }
        if let Some(&v) = pairs.get("pot") {
            s.pot = v == "1";
        }
        if let Some(&v) = pairs.get("frc") {
            s.frc = v == "1";
        }
        if let Some(&v) = pairs.get("ctrl") {
            s.ctrl = v == "1";
        }
        if let Some(&v) = pairs.get("mof") {
            s.mof = v == "1";
        }
        if let Some(mut f) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) {
            f.dirty = true;
        }
        Some(())
    });

    // C# ref: SendSettings → send #S with updated values, then re-show GUI
    // For now we send #S with the values and re-open the settings GUI
    let sett_wire = build_settings_wire(state, pid);
    send_u_packet(tx, "#S", &sett_wire);
    crate::net::session::social::misc::handle_sett_ty(state, tx, pid, &[]);
}

/// Build #S packet payload from player's current settings.
/// Wire format: `#key#value#key#value...` — 1:1 с `SettingsPacket.Encode()` в C# референсе.
fn build_settings_wire(state: &Arc<GameState>, pid: PlayerId) -> Vec<u8> {
    let s = state
        .query_player_opt(pid, |ecs, entity| {
            ecs.get::<crate::game::player::PlayerSettings>(entity)
                .copied()
        })
        .unwrap_or_default();
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
    format!("#{inner}").into_bytes()
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

async fn handle_open_prog(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    prog_id: i32,
) {
    let prog = state.db.get_program(prog_id).await.ok().flatten();
    let Some(p) = prog else { return };
    if p.player_id != pid {
        return;
    }
    state.modify_player(pid, |ecs, entity| {
        if let Some(mut ps) = ecs.get_mut::<crate::game::programmator::ProgrammatorState>(entity) {
            ps.selected_id = Some(p.id);
            ps.selected_data = Some(p.code.clone());
        }
        Some(())
    });
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
    match state.db.insert_program(pid, name, "").await {
        Ok(prog_id) => {
            let prog = state.db.get_program(prog_id).await.ok().flatten();
            let Some(p) = prog else { return };
            state.modify_player(pid, |ecs, entity| {
                if let Some(mut ps) =
                    ecs.get_mut::<crate::game::programmator::ProgrammatorState>(entity)
                {
                    ps.selected_id = Some(p.id);
                    ps.selected_data = Some(String::new());
                }
                Some(())
            });
            send_u_packet(
                tx,
                "#P",
                &crate::protocol::packets::open_programmator(p.id, &p.name, &p.code).1,
            );
        }
        Err(e) => tracing::warn!("[createprog] DB insert failed pid={pid}: {e:#}"),
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
    let prog = state.db.get_program(prog_id).await.ok().flatten();
    let Some(p) = prog else { return };
    if p.player_id != pid {
        return;
    }
    if let Err(e) = state.db.rename_program(prog_id, name).await {
        tracing::warn!("[rename] DB rename failed pid={pid} id={prog_id}: {e:#}");
        return;
    }
    send_u_packet(
        tx,
        "#P",
        &crate::protocol::packets::open_programmator(prog_id, name, &p.code).1,
    );
}
