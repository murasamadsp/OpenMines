//! Логика взаимодействия с объектами (паками) на карте.
use crate::game::buildings::{BuildingOwnership, BuildingStats, BuildingStorage};
use crate::game::player::{PlayerMetadata, PlayerPosition, PlayerStats, PlayerUI};
use crate::net::session::prelude::*;
use crate::net::session::social::buildings::modify_pack_with_db;

// TODO: will be used when pack interaction is fully wired to session dispatch
#[allow(dead_code)]
pub fn handle_pack_action(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    x: i32,
    y: i32,
) {
    let Some(pack) = state.get_pack_at(x, y) else {
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

    match pack.pack_type {
        PackType::Resp => {
            // Resp allows anyone standing on it to interact (bind).
            // Only check proximity, not ownership.
            if !pack
                .pack_type
                .building_cells()
                .iter()
                .any(|(dx, dy, _)| pack.x + dx == px && pack.y + dy == py)
            {
                return;
            }
            open_resp_gui(state, tx, pid, &pack);
        }
        PackType::Market => {
            // Market allows anyone standing on it to buy/sell.
            // Only check proximity, not ownership.
            if !pack
                .pack_type
                .building_cells()
                .iter()
                .any(|(dx, dy, _)| pack.x + dx == px && pack.y + dy == py)
            {
                return;
            }
            crate::net::session::ui::gui_buttons::open_pack_gui(state, tx, pid, &pack);
        }
        _ => {
            if validate_pack_access(&pack, (px, py), p_clan, pid).is_err() {
                return;
            }
            crate::net::session::ui::gui_buttons::handle_gui_button(
                state,
                tx,
                pid,
                &format!("pack_op:open:{}:{}", x, y),
            );
        }
    }
}

/// Open Resp visitor GUI (1:1 with C# `Resp.GUIWin`).
/// Shows bind button if not bound, or "you are bound" message.
/// Owner gets admin gear icon to access fill/settings page.
pub fn open_resp_gui(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    view: &PackView,
) {
    // Get player resp and check if bound here
    let (is_bound, is_owner) = state
        .query_player(pid, |ecs, entity| {
            let meta = ecs.get::<PlayerMetadata>(entity)?;
            let bound = meta.resp_x == Some(view.x) && meta.resp_y == Some(view.y);
            Some((bound, view.owner_id == pid))
        })
        .flatten()
        .unwrap_or((false, false));

    // Get cost from ECS
    let cost = state
        .building_index
        .get(&(view.x, view.y))
        .and_then(|ent| {
            let ecs = state.ecs.read();
            let stats = ecs.get::<BuildingStats>(*ent)?;
            Some(stats.cost)
        })
        .unwrap_or(10);

    let text = if is_bound {
        format!(
            "@@Респ - это место, где будет появляться ваш робот\n\
             после уничтожения (HP = 0)\n\n\
             Цена восстановления: <color=green>${cost}</color>\n\n\
             <color=#8f8>Вы привязаны к этому респу.</color>"
        )
    } else {
        format!(
            "@@Респ - это место, где будет появляться ваш робот\n\
             после уничтожения (HP = 0)\n\n\
             Цена восстановления: <color=green>${cost}</color>\n\n\
             <color=#f88>Привязать робота к респу?</color>"
        )
    };

    let mut buttons: Vec<serde_json::Value> = Vec::new();
    if !is_bound {
        buttons.push(serde_json::json!("ПРИВЯЗАТЬ"));
        buttons.push(serde_json::json!(format!(
            "resp_bind:{}:{}",
            view.x, view.y
        )));
    }
    buttons.push(serde_json::json!("ВЫЙТИ"));
    buttons.push(serde_json::json!("exit"));

    let mut gui = serde_json::json!({
        "title": "РЕСП",
        "text": text,
        "buttons": buttons,
        "back": false
    });

    // Owner gets admin gear icon
    if is_owner {
        gui["admin"] = serde_json::json!(true);
    }

    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());

    state.modify_player(pid, |ecs, entity| {
        if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) {
            ui.current_window = Some(format!("resp:{}:{}", view.x, view.y));
        }
        Some(())
    });
}

/// Open Resp admin page with RichList (fill sliders, cost, clanzone, profit).
/// 1:1 with C# `Resp.AdmnPage`.
pub fn open_resp_admin_gui(
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
        let ownership = ecs.get::<BuildingOwnership>(*ent)?;
        Some((
            stats.charge,
            stats.max_charge,
            stats.cost,
            storage.money,
            ownership.clan_id,
        ))
    });

    let Some((charge, max_charge, cost, money_inside, clan_id)) = details else {
        return;
    };

    // Get player's blue crystals for fill button availability
    let blue_crys = state
        .query_player(pid, |ecs, entity| {
            ecs.get::<PlayerStats>(entity).map(|s| s.crystals[1]) // Blue = index 1
        })
        .flatten()
        .unwrap_or(0);

    // Build fill bar: percent, label, crystal type, button actions
    let charge_i = charge as i32;
    let max_charge_i = max_charge as i32;
    let percent = if max_charge_i > 0 {
        ((charge as f64) / (max_charge as f64 / 100.0)).round() as i32
    } else {
        0
    };
    let bar_label = format!("{charge_i}/{max_charge_i}");

    // Fill button actions (active only if player has enough blue crystals)
    let fill100_action = if blue_crys >= 100 {
        format!("resp_fill:100:{}:{}", pack_x, pack_y)
    } else {
        String::new()
    };
    let fill1000_action = if blue_crys >= 1000 {
        format!("resp_fill:1000:{}:{}", pack_x, pack_y)
    } else {
        String::new()
    };
    let fill_max_action = if blue_crys > 0 {
        format!("resp_fill:max:{}:{}", pack_x, pack_y)
    } else {
        String::new()
    };

    // RichList format per C# Window.ToString():
    // Each entry = [label, type_str, values, action, value]
    // Fill: label="заряд", type="fill",
    //   values="{percent}#{barLabel}#{crystal_type}#{action100}#{action1000}#{actionMax}",
    //   action="", value=""
    let fill_values =
        format!("{percent}#{bar_label}#1#{fill100_action}#{fill1000_action}#{fill_max_action}");

    // Profit button
    let profit_label = format!("прибыль {money_inside}$");
    let profit_btn_action = if money_inside > 0 {
        format!("resp_profit:{}:{}", pack_x, pack_y)
    } else {
        String::new()
    };
    let profit_btn_label = if money_inside > 0 {
        "Получить"
    } else {
        ""
    };

    let rich_list = serde_json::json!([
        // Fill entry
        "заряд",
        "fill",
        fill_values,
        "",
        "",
        // HP text
        "hp",
        "text",
        "",
        "",
        "",
        // Cost uint
        "cost",
        "uint",
        "0",
        "cost",
        cost.to_string(),
        // Profit button
        profit_label,
        "button",
        profit_btn_label,
        profit_btn_action,
        profit_btn_label,
        // Clan bool
        "Клановый респ",
        "bool",
        "0",
        "clan",
        if clan_id > 0 { "1" } else { "0" },
        // Clanzone uint
        // TODO: clanzone not persisted in ECS — no field yet; hardcoded to 0
        "clanzone",
        "uint",
        "0",
        "clanzone",
        "0"
    ]);

    let buttons = vec![
        serde_json::json!("СОХРАНИТЬ"),
        serde_json::json!("resp_save:%R%"),
        serde_json::json!("ВЫЙТИ"),
        serde_json::json!("exit"),
    ];

    let gui = serde_json::json!({
        "title": "РЕСП",
        "richList": rich_list,
        "text": " ",
        "buttons": buttons,
        "back": false,
        "admin": true
    });

    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
}

/// Handle resp bind button click.
pub fn handle_resp_bind(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    pack_x: i32,
    pack_y: i32,
) {
    let Some(view) = state.get_pack_at(pack_x, pack_y) else {
        return;
    };
    if view.pack_type != PackType::Resp {
        return;
    }

    state.modify_player(pid, |ecs, entity| {
        let mut meta = ecs.get_mut::<PlayerMetadata>(entity)?;
        meta.resp_x = Some(pack_x);
        meta.resp_y = Some(pack_y);
        Some(())
    });

    // Re-open GUI to show "bound" state
    open_resp_gui(state, tx, pid, &view);
}

/// Handle resp fill button (+100, +1000, max).
/// Deducts blue crystals from player, adds charge to resp.
pub fn handle_resp_fill(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    amount_str: &str,
    pack_x: i32,
    pack_y: i32,
) {
    let Some(view) = state.get_pack_at(pack_x, pack_y) else {
        return;
    };
    if view.owner_id != pid {
        return;
    }

    // Get max fill amount from building
    let fill_info = state.building_index.get(&(pack_x, pack_y)).and_then(|ent| {
        let ecs = state.ecs.read();
        let stats = ecs.get::<BuildingStats>(*ent)?;
        Some((stats.charge, stats.max_charge))
    });
    let Some((charge, max_charge)) = fill_info else {
        return;
    };

    let requested: i64 = match amount_str {
        "100" => 100,
        "1000" => 1000,
        "max" => (max_charge - charge) as i64,
        _ => return,
    };

    if requested <= 0 {
        return;
    }

    // Deduct blue crystals from player (cap at available)
    let actual = state
        .modify_player(pid, |ecs, entity| {
            let mut s = ecs.get_mut::<PlayerStats>(entity)?;
            let available = s.crystals[1]; // Blue = index 1
            let to_take = requested.min(available);
            if to_take <= 0 {
                return Some(0i64);
            }
            s.crystals[1] -= to_take;
            send_u_packet(tx, "@B", &basket(&s.crystals, 1).1);
            Some(to_take)
        })
        .flatten()
        .unwrap_or(0);

    if actual <= 0 {
        return;
    }

    // Add charge to building
    let _ = modify_pack_with_db(state, pack_x, pack_y, |ecs, entity| {
        if let Some(mut stats) = ecs.get_mut::<BuildingStats>(entity) {
            stats.charge += actual as f32;
            if stats.charge > stats.max_charge {
                stats.charge = stats.max_charge;
            }
        }
    });

    // Refresh admin GUI
    open_resp_admin_gui(state, tx, pid, pack_x, pack_y);
}

/// Handle resp profit withdrawal.
pub fn handle_resp_profit(
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

    let mut amount = 0i64;
    let _ = modify_pack_with_db(state, pack_x, pack_y, |ecs, entity| {
        if let Some(mut s) = ecs.get_mut::<BuildingStorage>(entity) {
            amount = s.money;
            s.money = 0;
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

    // Refresh admin GUI
    open_resp_admin_gui(state, tx, pid, pack_x, pack_y);
}

/// Handle resp admin save (cost, clan toggle, clanzone).
/// Button format: `resp_save:{richlist_data}` (coordinates from `current_window`).
/// RichList data from client: `key:value#key:value#...` (hash-separated, colon key:value).
pub fn handle_resp_save(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    richlist_data: &str,
) {
    // Resolve coordinates from current_window ("resp:{x}:{y}")
    let coords = state
        .query_player(pid, |ecs, entity| {
            let ui = ecs.get::<PlayerUI>(entity)?;
            let window = ui.current_window.as_deref()?;
            let rest = window.strip_prefix("resp:")?;
            let parts: Vec<&str> = rest.split(':').collect();
            if parts.len() == 2 {
                Some((parts[0].parse::<i32>().ok()?, parts[1].parse::<i32>().ok()?))
            } else {
                None
            }
        })
        .flatten();

    let Some((pack_x, pack_y)) = coords else {
        return;
    };

    let Some(view) = state.get_pack_at(pack_x, pack_y) else {
        return;
    };
    if view.owner_id != pid {
        return;
    }

    // Parse RichList key:value pairs separated by '#'
    // C# ref: `args.RichList = ...Split('#').Select(x => x.Split(':')).ToDictionary(x[0], x[1])`
    let mut fields: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for pair in richlist_data.split('#') {
        if pair.is_empty() {
            continue;
        }
        if let Some((k, v)) = pair.split_once(':') {
            fields.insert(k, v);
        }
    }

    // Pre-fetch owner's clan_id before acquiring ECS write lock to avoid deadlock
    let owner_clan = state
        .query_player(pid, |ecs, e| {
            ecs.get::<PlayerStats>(e).and_then(|s| s.clan_id)
        })
        .flatten()
        .unwrap_or(0);

    let _ = modify_pack_with_db(state, pack_x, pack_y, |ecs, entity| {
        if let Some(mut stats) = ecs.get_mut::<BuildingStats>(entity) {
            if let Some(cost_str) = fields.get("cost") {
                if let Ok(c) = cost_str.parse::<i32>() {
                    if (0..=5000).contains(&c) {
                        stats.cost = c;
                    }
                }
            }
        }
        if let Some(mut ownership) = ecs.get_mut::<BuildingOwnership>(entity) {
            if let Some(clan_str) = fields.get("clan") {
                if clan_str == &"1" || clan_str == &"true" {
                    ownership.clan_id = owner_clan;
                } else {
                    ownership.clan_id = 0;
                }
            }
        }
    });

    // Refresh admin GUI
    open_resp_admin_gui(state, tx, pid, pack_x, pack_y);
}
