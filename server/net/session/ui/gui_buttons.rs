//! Обработка нажатий GUI-кнопок игроком.
use crate::game::buildings::{
    BuildingCrafting, BuildingMetadata, BuildingStats, BuildingStorage, GridPosition,
};
use crate::game::crafting;
use crate::game::market;
use crate::game::player::{PlayerInventory, PlayerPosition, PlayerStats, PlayerUI};
use crate::net::session::outbound::inventory_sync::send_inventory;
use crate::net::session::prelude::*;
use crate::net::session::social::buildings::{broadcast_pack_update, modify_pack_with_db};

pub fn handle_gui_button(
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
        .query_player(pid, |ecs, entity| {
            ecs.get::<PlayerUI>(entity)
                .map(|ui| ui.current_window.is_some())
        })
        .flatten()
        .unwrap_or(false);
    if !has_window {
        let g = gu_close();
        send_u_packet(tx, g.0, &g.1);
        return;
    }

    if let Some(rest) = button.strip_prefix("clan_view:") {
        if let Ok(id) = rest.parse::<i32>() {
            crate::net::session::social::clans::handle_clan_preview(state, tx, pid, id);
        }
        return;
    }

    match button {
        "open_buildings" => {
            crate::net::session::social::buildings::handle_buildings_menu(state, tx, pid);
        }
        "createprog_stub" => {
            crate::net::session::social::commands::send_ok(
                tx,
                "Программатор",
                "Создание программы из GUI пока не подключено к БД.",
            );
        }
        "clan_menu" => crate::net::session::social::clans::handle_clan_menu(state, tx, pid),
        "clan_back" => crate::net::session::social::clans::handle_clan_menu(state, tx, pid),
        "clan_create_view" => handle_clan_create_view(tx),
        "clan_requests" => {
            crate::net::session::social::clans::handle_clan_requests_view(state, tx, pid);
        }
        "clan_members" => {
            crate::net::session::social::clans::handle_clan_members_view(state, tx, pid);
        }
        "clan_invite_list" => {
            crate::net::session::social::clans::handle_clan_invite_list(state, tx, pid);
        }
        "clan_invites_view" => {
            crate::net::session::social::clans::handle_clan_invites_view(state, tx, pid);
        }
        "clan_leave" => crate::net::session::social::clans::handle_clan_leave(state, tx, pid),
        // Market tab switching (C# tabs have action strings)
        "sellcrys" => handle_market_tab_switch(state, tx, pid, "sellcrys"),
        "buycrys" => handle_market_tab_switch(state, tx, pid, "buycrys"),
        "auc" => handle_market_tab_switch(state, tx, pid, "auc"),
        "sellall" => handle_market_sellall(state, tx, pid),
        "getprofit" => handle_market_getprofit(state, tx, pid),
        _ => handle_complex_button(state, tx, pid, button),
    }

    // C# ref: after CallWinAction, SendWindow() re-sends the window or closes if null.
    // Safety net: if no handler sent a response and window was cleared, send Gu close.
    let still_has_window = state
        .query_player(pid, |ecs, entity| {
            ecs.get::<PlayerUI>(entity)
                .map(|ui| ui.current_window.is_some())
        })
        .flatten()
        .unwrap_or(false);
    if !still_has_window {
        let g = gu_close();
        send_u_packet(tx, g.0, &g.1);
    }
}

fn handle_clan_create_view(tx: &mpsc::UnboundedSender<Vec<u8>>) {
    let gui = serde_json::json!({
        "title": "СОЗДАНИЕ КЛАНА",
        "text": "Введите название и тег (3 симв.) через пробел в чат после нажатия кнопки 'ВВОД'",
        "buttons": ["ВВОД", "clan_create_input", "Назад", "clan_back"],
        "back": false
    });
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
}

fn handle_complex_button(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    button: &str,
) {
    if let Some(rest) = button.strip_prefix("clan_request:") {
        if let Ok(id) = rest.parse::<i32>() {
            crate::net::session::social::clans::handle_clan_join_request(state, tx, pid, id);
        }
    } else if let Some(rest) = button.strip_prefix("clan_accept:") {
        if let Ok(id) = rest.parse::<i32>() {
            crate::net::session::social::clans::handle_clan_accept(state, tx, pid, id);
        }
    } else if let Some(rest) = button.strip_prefix("clan_invite_send:") {
        if let Ok(id) = rest.parse::<i32>() {
            crate::net::session::social::clans::handle_clan_invite_send(state, tx, pid, id);
        }
    } else if let Some(rest) = button.strip_prefix("clan_invite_accept:") {
        if let Ok(id) = rest.parse::<i32>() {
            crate::net::session::social::clans::handle_clan_invite_accept(state, tx, pid, id);
        }
    } else if let Some(rest) = button.strip_prefix("bld_place:") {
        crate::net::session::social::buildings::handle_place_building(state, tx, pid, rest);
    } else if let Some(rest) = button.strip_prefix("pack_op:") {
        handle_pack_operation(state, tx, pid, rest);
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
    } else if let Some(rest) = button.strip_prefix("sell:") {
        handle_market_sell(state, tx, pid, rest);
    } else if let Some(rest) = button.strip_prefix("buy:") {
        handle_market_buy(state, tx, pid, rest);
    } else {
        // Up building buttons (skill:N, upgrade, delete:N, install:code#N, buyslot)
        super::up_building::handle_up_button(state, tx, pid, button);
    }
}

fn handle_pack_operation(
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

    let p_info = state
        .query_player(pid, |ecs, entity| {
            let pos = ecs.get::<PlayerPosition>(entity)?;
            let stats = ecs.get::<PlayerStats>(entity)?;
            Some((pos.x, pos.y, stats.clan_id.unwrap_or(0)))
        })
        .flatten();

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
            crate::net::session::social::buildings::handle_remove_building(state, tx, pid, x, y);
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

    let title = view.pack_type.name();

    // Fetch detailed stats from ECS for GUI
    let stats_info = state.building_index.get(&(view.x, view.y)).and_then(|ent| {
        let ecs = state.ecs.read();
        let stats = ecs.get::<BuildingStats>(*ent)?;
        Some((stats.hp, stats.max_hp))
    });
    let (hp, mhp) = stats_info.unwrap_or((0, 0));

    let text = format!(
        "Здание: {}\nЗаряд: {:.1}\nПрочность: {}/{}",
        title, view.charge, hp, mhp
    );
    let mut buttons = vec![
        serde_json::json!("Забрать деньги"),
        serde_json::json!(format!("pack_op:take_money:{}:{}", view.x, view.y)),
        serde_json::json!("Забрать кристаллы"),
        serde_json::json!(format!("pack_op:take_crys:{}:{}", view.x, view.y)),
        serde_json::json!("Удалить"),
        serde_json::json!(format!("pack_op:remove:{}:{}", view.x, view.y)),
    ];
    buttons.extend(
        CLOSE_WINDOW_BUTTON_LABELS
            .iter()
            .map(|l| serde_json::json!(l)),
    );

    let gui =
        serde_json::json!({ "title": title, "text": text, "buttons": buttons, "back": false });
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());

    state.modify_player(pid, |ecs, entity| {
        if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) {
            ui.current_window = Some(format!("pack:{}:{}", view.x, view.y));
        }
        Some(())
    });
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
        .query_player(pid, |ecs, entity| {
            ecs.get::<PlayerStats>(entity).map(|s| s.crystals)
        })
        .flatten()
        .unwrap_or([0; 6]);

    // Build crys_lines: each line is "LeftMin:RightMin:Denominator:CurrentValue:Label"
    // C# ref: CrysLine("", 0, 0, p.crys.cry[id] + cry, (int)(cry))
    // Serialized as: "{LeftMin}:{RightMin}:{Denominator}:{CurrentValue}:{Label}"
    let crys_lines: Vec<serde_json::Value> = (0..6)
        .map(|i| {
            let denominator = player_crys[i] + storage_crys[i];
            let current_value = storage_crys[i];
            serde_json::json!(format!("0:0:{denominator}:{current_value}:"))
        })
        .collect();

    // Build buttons: "transfer" with %M% macro, plus remove and exit
    let buttons = vec![
        serde_json::json!("transfer"),
        serde_json::json!("transfer:%M%"),
        serde_json::json!("Удалить"),
        serde_json::json!(format!("pack_op:remove:{}:{}", view.x, view.y)),
        serde_json::json!("ВЫЙТИ"),
        serde_json::json!("exit"),
    ];

    let gui = serde_json::json!({
        "title": "Склад",
        "crys_left": " ",
        "crys_right": " ",
        "crys_lines": crys_lines,
        "buttons": buttons,
        "back": false
    });
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());

    state.modify_player(pid, |ecs, entity| {
        if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) {
            ui.current_window = Some(format!("pack:{}:{}", view.x, view.y));
        }
        Some(())
    });
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
    let coords = state
        .query_player(pid, |ecs, entity| {
            let ui = ecs.get::<PlayerUI>(entity)?;
            let window = ui.current_window.as_deref()?;
            let parts: Vec<&str> = window.strip_prefix("pack:")?.split(':').collect();
            if parts.len() == 2 {
                Some((parts[0].parse::<i32>().ok()?, parts[1].parse::<i32>().ok()?))
            } else {
                None
            }
        })
        .flatten();

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
    let p_info = state
        .query_player(pid, |ecs, entity| {
            let pos = ecs.get::<PlayerPosition>(entity)?;
            let stats = ecs.get::<PlayerStats>(entity)?;
            Some((pos.x, pos.y, stats.clan_id.unwrap_or(0)))
        })
        .flatten();
    let Some((px, py, p_clan)) = p_info else {
        return;
    };
    if validate_pack_access(&view, (px, py), p_clan, pid).is_err() {
        return;
    }

    // Perform the transfer: read storage crystals, compute new values
    // Get current storage crystals
    let storage_crys = state
        .building_index
        .get(&(bx, by))
        .and_then(|ent| {
            let ecs = state.ecs.read();
            let s = ecs.get::<BuildingStorage>(*ent)?;
            Some(s.crystals)
        })
        .unwrap_or([0; 6]);

    // Get current player crystals
    let player_crys = state
        .query_player(pid, |ecs, entity| {
            ecs.get::<PlayerStats>(entity).map(|s| s.crystals)
        })
        .flatten()
        .unwrap_or([0; 6]);

    // Validate slider values (C# ref: count - sliders[i] >= 0 && sliders[i] >= 0)
    let mut new_player = [0i64; 6];
    let mut new_storage = [0i64; 6];
    for i in 0..6 {
        let count = player_crys[i] + storage_crys[i];
        if sliders[i] < 0 || count - sliders[i] < 0 {
            return; // Invalid slider value
        }
        new_player[i] = count - sliders[i];
        new_storage[i] = sliders[i];
    }

    // Apply to storage
    if modify_pack_with_db(state, bx, by, |ecs, entity| {
        if let Some(mut s) = ecs.get_mut::<BuildingStorage>(entity) {
            s.crystals = new_storage;
        }
    })
    .is_err()
    {
        return;
    }

    // Apply to player and send @B update
    state.modify_player(pid, |ecs, entity| {
        let mut s = ecs.get_mut::<PlayerStats>(entity)?;
        s.crystals = new_player;
        send_u_packet(tx, "@B", &basket(&s.crystals, 1).1);
        Some(())
    });

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

    let mut buttons: Vec<serde_json::Value> = Vec::new();
    if done {
        buttons.push(serde_json::json!("Забрать"));
        buttons.push(serde_json::json!(format!(
            "craft_claim:{}:{}",
            view.x, view.y
        )));
    }
    buttons.push(serde_json::json!("ВЫЙТИ"));
    buttons.push(serde_json::json!("exit"));

    let gui = serde_json::json!({
        "title": "Крафтер",
        "text": text,
        "buttons": buttons,
        "back": false
    });
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
}

fn show_crafter_recipes(tx: &mpsc::UnboundedSender<Vec<u8>>, view: &PackView) {
    let recipes = crafting::recipes();
    let crys_names = ["зель", "синь", "крась", "фиоль", "бель", "голь"];

    let mut text = String::from("Выберите рецепт:\n");
    let mut buttons: Vec<serde_json::Value> = Vec::new();

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

        buttons.push(serde_json::json!(r.title));
        buttons.push(serde_json::json!(format!(
            "craft_recipe:{}:{}:{}",
            r.id, view.x, view.y
        )));
    }

    buttons.push(serde_json::json!("Удалить"));
    buttons.push(serde_json::json!(format!(
        "pack_op:remove:{}:{}",
        view.x, view.y
    )));
    buttons.push(serde_json::json!("ВЫЙТИ"));
    buttons.push(serde_json::json!("exit"));

    let gui = serde_json::json!({
        "title": "Крафтер",
        "text": text,
        "buttons": buttons,
        "back": false
    });
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
}

/// Show recipe details + Start button.
/// Called from `craft_recipe:{id}:{x}:{y}` but handle_complex_button parses
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

    let buttons = vec![
        serde_json::json!("Запустить (x1)"),
        serde_json::json!(format!("craft_start:{}:1:{}:{}", recipe_id, bx, by)),
        serde_json::json!("ВЫЙТИ"),
        serde_json::json!("exit"),
    ];

    let gui = serde_json::json!({
        "title": "Крафтер",
        "text": text,
        "buttons": buttons,
        "back": false
    });
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
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
                let stats = ecs.get::<PlayerStats>(entity)?;
                for c in recipe.cost_crys {
                    if stats.crystals[c.id as usize] < i64::from(c.num) * i64::from(num) {
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
                let mut stats = ecs.get_mut::<PlayerStats>(entity)?;
                for c in recipe.cost_crys {
                    stats.crystals[c.id as usize] -= i64::from(c.num) * i64::from(num);
                }
                send_u_packet(tx, "@B", &basket(&stats.crystals, 1).1);
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
                let stats = ecs.get::<BuildingStats>(entity)?;
                if stats.charge <= 0.0 {
                    return None;
                }
                Some((pos.x, pos.y))
            })
            .collect()
    };

    let mut buttons: Vec<serde_json::Value> = Vec::new();
    for (tpx, tpy) in &nearby_tps {
        buttons.push(serde_json::json!(format!("TP {}:{}", tpx, tpy)));
        buttons.push(serde_json::json!(format!("tp:{}:{}", tpx, tpy)));
    }
    buttons.push(serde_json::json!("Забрать деньги"));
    buttons.push(serde_json::json!(format!(
        "pack_op:take_money:{}:{}",
        view.x, view.y
    )));
    buttons.push(serde_json::json!("Забрать кристаллы"));
    buttons.push(serde_json::json!(format!(
        "pack_op:take_crys:{}:{}",
        view.x, view.y
    )));
    buttons.push(serde_json::json!("Удалить"));
    buttons.push(serde_json::json!(format!(
        "pack_op:remove:{}:{}",
        view.x, view.y
    )));
    buttons.extend(
        CLOSE_WINDOW_BUTTON_LABELS
            .iter()
            .map(|l| serde_json::json!(l)),
    );

    let stats_info = state.building_index.get(&(view.x, view.y)).and_then(|ent| {
        let ecs = state.ecs.read();
        let stats = ecs.get::<BuildingStats>(*ent)?;
        Some((stats.hp, stats.max_hp))
    });
    let (hp, mhp) = stats_info.unwrap_or((0, 0));

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

    let gui = serde_json::json!({
        "title": "Тп",
        "text": text,
        "buttons": buttons,
        "back": false
    });
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());

    state.modify_player(pid, |ecs, entity| {
        if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) {
            ui.current_window = Some(format!("pack:{}:{}", view.x, view.y));
        }
        Some(())
    });
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

    let src_coords = state
        .query_player(pid, |ecs, entity| {
            let ui = ecs.get::<PlayerUI>(entity)?;
            let window = ui.current_window.as_deref()?;
            let rest = window.strip_prefix("pack:")?;
            let p: Vec<&str> = rest.split(':').collect();
            if p.len() == 2 {
                Some((p[0].parse::<i32>().ok()?, p[1].parse::<i32>().ok()?))
            } else {
                None
            }
        })
        .flatten();

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
        return;
    }

    // C# ref: `new Window() { Tabs = [] }` — empty-tabs window.
    // Client interprets this as a minimal panel (spot programmator GUI placeholder).
    // The HORB format for an empty-tabs window uses "tabs": [] in the JSON.
    let gui = serde_json::json!({
        "title": "СПОТ",
        "text": "",
        "buttons": [
            "Удалить",
            format!("pack_op:remove:{}:{}", view.x, view.y),
            "ВЫЙТИ",
            "exit"
        ],
        "back": false
    });
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());

    state.modify_player(pid, |ecs, entity| {
        if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) {
            ui.current_window = Some(format!("pack:{}:{}", view.x, view.y));
        }
        Some(())
    });
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
    let player_info = state
        .query_player(pid, |ecs, entity| {
            let stats = ecs.get::<PlayerStats>(entity)?;
            Some((stats.money, stats.crystals))
        })
        .flatten();

    let Some((player_money, player_crys)) = player_info else {
        return;
    };

    // Build tabs array: active tab gets empty action, others get their action string
    let tabs = build_market_tabs(active_tab);

    let (gui_json, window_tag) = match active_tab {
        "buycrys" => (
            build_market_buy_page(player_money, is_owner, &tabs),
            format!("market:{}:{}:buycrys", view.x, view.y),
        ),
        "auc" => (
            build_market_auc_page(&tabs),
            format!("market:{}:{}:auc", view.x, view.y),
        ),
        _ => (
            build_market_sell_page(&player_crys, is_owner, &tabs),
            format!("market:{}:{}:sellcrys", view.x, view.y),
        ),
    };

    send_u_packet(tx, "GU", format!("horb:{gui_json}").as_bytes());

    state.modify_player(pid, |ecs, entity| {
        if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) {
            ui.current_window = Some(window_tag);
        }
        Some(())
    });
}

/// Build the tabs JSON array for Market.
/// C# Window.ToString: active tab = ["Label", ""], inactive = ["Label", "action"].
fn build_market_tabs(active_tab: &str) -> serde_json::Value {
    let sell_active = active_tab == "sellcrys";
    let buy_active = active_tab == "buycrys";
    let auc_active = active_tab == "auc";

    serde_json::json!([
        "ПРОДАЖА",
        if sell_active { "" } else { "sellcrys" },
        "Покупка",
        if buy_active { "" } else { "buycrys" },
        "Auc",
        if auc_active { "" } else { "auc" }
    ])
}

/// Build sell tab page JSON.
/// C# ref: Market.BuildSelltab — CrystalConfig with sell prices, sliders up to player's crystals.
fn build_market_sell_page(
    player_crys: &[i64; 6],
    is_owner: bool,
    tabs: &serde_json::Value,
) -> serde_json::Value {
    // crys_lines format: "LeftMin:RightMin:Denominator:CurrentValue:Label"
    // C# CrysLine(label, leftMin=0, rightMin=0, denominator=player_crys[i], currentValue=0)
    let crys_lines: Vec<serde_json::Value> = (0..6)
        .map(|i| {
            let cost = market::get_crystal_cost(i);
            let label = format!("<color=#aaeeaa>{cost}$</color>");
            serde_json::json!(format!("0:0:{}:0:{}", player_crys[i], label))
        })
        .collect();

    let buttons: Vec<serde_json::Value> = vec![
        serde_json::json!("sellall"),
        serde_json::json!("sellall"),
        serde_json::json!("sell"),
        serde_json::json!("sell:%M%"),
        serde_json::json!("ВЫЙТИ"),
        serde_json::json!("exit"),
    ];

    let mut gui = serde_json::json!({
        "title": "Market",
        "tabs": tabs,
        "crys_left": " ",
        "crys_right": "цена",
        "crys_lines": crys_lines,
        "text": "Продажа кри",
        "buttons": buttons,
        "back": false
    });

    if is_owner {
        gui["admin"] = serde_json::json!(true);
    }

    gui
}

/// Build buy tab page JSON.
/// C# ref: Market.BuildBuytab — CrystalConfig with buy prices (10x), sliders denominator =
/// player.money / (cost * 10). BuyMode = true (`crys_buy: true`).
fn build_market_buy_page(
    player_money: i64,
    is_owner: bool,
    tabs: &serde_json::Value,
) -> serde_json::Value {
    let crys_lines: Vec<serde_json::Value> = (0..6)
        .map(|i| {
            let buy_price = market::get_crystal_buy_price(i);
            let max_can_buy = if buy_price > 0 {
                player_money / buy_price
            } else {
                0
            };
            let label = format!("<color=#aaeeaa>{buy_price}$</color>");
            serde_json::json!(format!("0:0:{max_can_buy}:0:{label}"))
        })
        .collect();

    let buttons: Vec<serde_json::Value> = vec![
        serde_json::json!("buy"),
        serde_json::json!("buy:%M%"),
        serde_json::json!("ВЫЙТИ"),
        serde_json::json!("exit"),
    ];

    let mut gui = serde_json::json!({
        "title": "Market",
        "tabs": tabs,
        "crys_left": " ",
        "crys_right": "цена",
        "crys_lines": crys_lines,
        "crys_buy": true,
        "text": "Покупка",
        "buttons": buttons,
        "back": false
    });

    if is_owner {
        gui["admin"] = serde_json::json!(true);
    }

    gui
}

/// Build auction tab page (stub — auction requires DB orders table not yet implemented).
fn build_market_auc_page(tabs: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "title": "МАРКЕТ",
        "tabs": tabs,
        "text": "Аукцион временно недоступен.",
        "buttons": ["ВЫЙТИ", "exit"],
        "back": false
    })
}

/// Resolve market coordinates and tab from current_window ("market:{x}:{y}:{tab}").
fn resolve_market_window(state: &Arc<GameState>, pid: PlayerId) -> Option<(i32, i32, String)> {
    state
        .query_player(pid, |ecs, entity| {
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
        .flatten()
}

/// Handle Market tab switching.
fn handle_market_tab_switch(
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
    open_market_gui(state, tx, pid, &view, tab);
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
        .query_player(pid, |ecs, entity| {
            ecs.get::<PlayerStats>(entity).map(|s| s.crystals)
        })
        .flatten()
        .unwrap_or([0; 6]);

    let sliders: Vec<i64> = player_crys.to_vec();
    do_market_sell(state, tx, pid, &sliders, bx, by);
}

/// Common sell logic (used by sell and sellall).
/// C# ref: `MarketSystem.Sell`:
///   for each i: if RemoveCrys succeeds, money += value * GetCrysCost(i)
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
        let mut stats = ecs.get_mut::<PlayerStats>(entity)?;
        for i in 0..6 {
            let to_sell = sliders[i];
            if to_sell <= 0 {
                continue;
            }
            // C# RemoveCrys: only succeeds if player has enough
            if stats.crystals[i] >= to_sell {
                stats.crystals[i] -= to_sell;
                total_money += to_sell * market::get_crystal_cost(i);
            }
        }
        // Add money to player
        stats.money += total_money;
        // Send updates
        send_u_packet(tx, "@B", &basket(&stats.crystals, 1).1);
        send_u_packet(tx, "P$", &money(stats.money, stats.creds).1);
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
        let mut stats = ecs.get_mut::<PlayerStats>(entity)?;
        for i in 0..6 {
            let to_buy = sliders[i];
            if to_buy <= 0 {
                continue;
            }
            let cost = to_buy * market::get_crystal_buy_price(i);
            // C# ref: if p.money - (sliders[i] * World.GetCrysCost(i) * 10) < 0 continue
            if stats.money < cost {
                continue;
            }
            stats.money -= cost;
            stats.crystals[i] += to_buy;
        }
        send_u_packet(tx, "@B", &basket(&stats.crystals, 1).1);
        send_u_packet(tx, "P$", &money(stats.money, stats.creds).1);
        Some(())
    });

    // Re-render buy tab with updated money
    open_market_gui(state, tx, pid, &view, "buycrys");
}

/// Handle "getprofit" — owner withdraws accumulated market profit.
/// C# ref: `Market.onadmn` — transfer moneyinside to player, reset to 0,
/// then re-open the admin RichList page.
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
            let mut s = ecs.get_mut::<PlayerStats>(entity)?;
            s.money += amount;
            send_u_packet(tx, "P$", &money(s.money, s.creds).1);
            Some(())
        });
    }

    // Re-open admin page with updated profit (now 0)
    open_market_admin_gui(state, tx, pid, bx, by);
}

/// Open Market admin page with RichList (1:1 with C# `Market.onadmn`).
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
        let stats = ecs.get::<BuildingStats>(*ent)?;
        let storage = ecs.get::<BuildingStorage>(*ent)?;
        Some((stats.hp, storage.money))
    });

    let Some((hp, money_inside)) = details else {
        return;
    };

    // C# ref: RichList with hp text + profit button
    // RichList format: [label, type, values, action, value] per entry
    let profit_label = format!("прибыль {money_inside}$");
    let profit_btn_label = if money_inside > 0 {
        "Получить"
    } else {
        ""
    };
    let profit_btn_action = if money_inside > 0 { "getprofit" } else { "" };

    let rich_list = serde_json::json!([
        // HP text entry
        format!("hp {hp}"),
        "text",
        "",
        "",
        "",
        // Profit button entry
        profit_label,
        "button",
        profit_btn_label,
        profit_btn_action,
        profit_btn_label
    ]);

    let gui = serde_json::json!({
        "title": "Market",
        "richList": rich_list,
        "text": " ",
        "buttons": ["ВЫЙТИ", "exit"],
        "back": false
    });

    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());

    // Keep market window tag so getprofit can resolve coordinates
    state.modify_player(pid, |ecs, entity| {
        if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) {
            ui.current_window = Some(format!("market:{}:{}:admin", pack_x, pack_y));
        }
        Some(())
    });
}
