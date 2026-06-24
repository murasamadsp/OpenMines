//! Логика взаимодействия с объектами (паками) на карте.
use crate::game::buildings::{BuildingOwnership, BuildingStats, BuildingStorage};
use crate::game::player::{PlayerMetadata, PlayerPosition, PlayerStats, PlayerUI};
use crate::net::session::prelude::*;
use crate::net::session::social::buildings::modify_pack_with_db;

// TODO: will be used when pack interaction is fully wired to session dispatch
#[allow(dead_code)]
pub async fn handle_pack_action(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    x: i32,
    y: i32,
) {
    let Some(pack) = state.get_pack_at(x, y) else {
        return;
    };

    let p_info = state.query_player_opt(pid, |ecs, entity| {
        let pos = ecs.get::<PlayerPosition>(entity)?;
        let pk_stats = ecs.get::<PlayerStats>(entity)?;
        Some((pos.x, pos.y, pk_stats.clan_id.unwrap_or(0)))
    });

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
                &format!("pack_op:open:{x}:{y}"),
            )
            .await;
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
        .query_player_opt(pid, |ecs, entity| {
            let meta = ecs.get::<PlayerMetadata>(entity)?;
            let bound = meta.resp_x == Some(view.x) && meta.resp_y == Some(view.y);
            Some((bound, view.owner_id == pid))
        })
        .unwrap_or((false, false));

    // Get cost from ECS
    let cost = state
        .building_index
        .get(&(view.x, view.y))
        .and_then(|ent| {
            let ecs = state.ecs.read();
            let pk_stats = ecs.get::<BuildingStats>(*ent)?;
            Some(pk_stats.cost)
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

    use crate::net::session::ui::horb::{Button, Horb};
    let mut win = Horb::new("РЕСП").text(text).admin(is_owner);
    if !is_bound {
        win = win.button(Button::new(
            "ПРИВЯЗАТЬ",
            format!("resp_bind:{}:{}", view.x, view.y),
        ));
    }
    win.close_button()
        .send(state, tx, pid, format!("resp:{}:{}", view.x, view.y));
}

/// Open Resp admin page with `RichList` (fill sliders, cost, clanzone, profit).
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
        let pk_stats = ecs.get::<BuildingStats>(*ent)?;
        let storage = ecs.get::<BuildingStorage>(*ent)?;
        let ownership = ecs.get::<BuildingOwnership>(*ent)?;
        Some((
            pk_stats.charge,
            pk_stats.max_charge,
            pk_stats.cost,
            pk_stats.clanzone,
            storage.money,
            ownership.clan_id,
        ))
    });

    let Some((charge, max_charge, cost, clanzone, money_inside, clan_id)) = details else {
        return;
    };

    // Get player's blue crystals for fill button availability
    let blue_crys = state
        .query_player_opt(pid, |ecs, entity| {
            ecs.get::<PlayerStats>(entity).map(|s| s.crystals[1]) // Blue = index 1
        })
        .unwrap_or(0);

    // Build fill bar: percent, label, crystal type, button actions
    let charge_i = charge as i32;
    let max_charge_i = max_charge as i32;
    let percent = if max_charge_i > 0 {
        (f64::from(charge) / (f64::from(max_charge) / 100.0)).round() as i32
    } else {
        0
    };
    let bar_label = format!("{charge_i}/{max_charge_i}");

    // Fill button actions (active only if player has enough blue crystals)
    let small_fill_action = if blue_crys >= 100 {
        format!("resp_fill:100:{pack_x}:{pack_y}")
    } else {
        String::new()
    };
    let large_fill_action = if blue_crys >= 1000 {
        format!("resp_fill:1000:{pack_x}:{pack_y}")
    } else {
        String::new()
    };
    let fill_max_action = if blue_crys > 0 {
        format!("resp_fill:max:{pack_x}:{pack_y}")
    } else {
        String::new()
    };

    // RichList format per C# Window.ToString():
    // Each entry = [label, type_str, values, action, value]
    // Fill: label="заряд", type="fill",
    //   values="{percent}#{barLabel}#{crystal_type}#{action100}#{action1000}#{actionMax}",
    //   action="", value=""
    let fill_values = format!(
        "{percent}#{bar_label}#1#{small_fill_action}#{large_fill_action}#{fill_max_action}"
    );

    // Profit button
    let profit_label = format!("прибыль {money_inside}$");
    let profit_btn_action = if money_inside > 0 {
        format!("resp_profit:{pack_x}:{pack_y}")
    } else {
        String::new()
    };
    let profit_btn_label = if money_inside > 0 {
        "Получить"
    } else {
        ""
    };

    use crate::net::session::ui::horb::{Button, Horb, RichRow};
    Horb::new("РЕСП")
        .text(" ")
        .rich_row(RichRow::fill("заряд", fill_values))
        .rich_row(RichRow::text("hp"))
        .rich_row(RichRow::uint("cost", "cost", i64::from(cost)))
        .rich_row(RichRow::button(
            profit_label,
            profit_btn_label,
            profit_btn_action,
        ))
        .rich_row(RichRow::toggle("Клановый респ", "clan", clan_id > 0))
        .rich_row(RichRow::uint("clanzone", "clanzone", i64::from(clanzone)))
        .button(Button::new("СОХРАНИТЬ", "resp_save:%R%"))
        .admin(true)
        .close_button()
        .send(state, tx, pid, format!("resp:{pack_x}:{pack_y}"));
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
        {
            let mut meta = ecs.get_mut::<PlayerMetadata>(entity)?;
            meta.resp_x = Some(pack_x);
            meta.resp_y = Some(pack_y);
        }
        // 1:1 C# `SetResp` + `SaveChanges`: помечаем dirty, иначе flush
        // (dirty-gated) не сохранит привязку и она теряется при релоге.
        if let Some(mut f) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) {
            f.dirty = true;
        }
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
        let pk_stats = ecs.get::<BuildingStats>(*ent)?;
        Some((pk_stats.charge, pk_stats.max_charge))
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
        if let Some(mut pk_stats) = ecs.get_mut::<BuildingStats>(entity) {
            pk_stats.charge += actual as f32;
            if pk_stats.charge > pk_stats.max_charge {
                pk_stats.charge = pk_stats.max_charge;
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
/// `RichList` data from client: `key:value#key:value#...` (hash-separated, colon key:value).
pub fn handle_resp_save(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    richlist_data: &str,
) {
    // Resolve coordinates from current_window ("resp:{x}:{y}")
    let coords = state.query_player_opt(pid, |ecs, entity| {
        let ui = ecs.get::<PlayerUI>(entity)?;
        let window = ui.current_window.as_deref()?;
        let rest = window.strip_prefix("resp:")?;
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
        .query_player_opt(pid, |ecs, e| {
            ecs.get::<PlayerStats>(e).and_then(|s| s.clan_id)
        })
        .unwrap_or(0);

    let _ = modify_pack_with_db(state, pack_x, pack_y, |ecs, entity| {
        if let Some(mut pk_stats) = ecs.get_mut::<BuildingStats>(entity) {
            if let Some(cost_str) = fields.get("cost") {
                if let Ok(c) = cost_str.parse::<i32>() {
                    if (0..=5000).contains(&c) {
                        pk_stats.cost = c;
                    }
                }
            }
            // Clanzone: 1:1 C# `Resp.AdminSaveChanges` — парсим без range-check.
            if let Some(cz_str) = fields.get("clanzone") {
                if let Ok(cz) = cz_str.parse::<i32>() {
                    pk_stats.clanzone = cz;
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

/// Открыть GUI пушки (`RichList` Fill, заряд Cyan).
pub fn open_gun_gui(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    pack_x: i32,
    pack_y: i32,
) {
    let fill_info = state.building_index.get(&(pack_x, pack_y)).and_then(|ent| {
        let ecs = state.ecs.read();
        let pk_stats = ecs.get::<BuildingStats>(*ent)?;
        Some((pk_stats.charge, pk_stats.max_charge))
    });
    let Some((charge, max_charge)) = fill_info else {
        return;
    };

    // Cyan = crystals[5] (1:1 CrystalType.Cyan = 5)
    let cyan_crys = state
        .query_player_opt(pid, |ecs, entity| {
            ecs.get::<PlayerStats>(entity).map(|s| s.crystals[5])
        })
        .unwrap_or(0);

    let charge_i = charge as i32;
    let max_charge_i = max_charge as i32;
    let percent = if max_charge_i > 0 {
        (f64::from(charge) / (f64::from(max_charge) / 100.0)).round() as i32
    } else {
        0
    };
    let bar_label = format!("{charge_i}/{max_charge_i}");

    // C# GUIWin: +100 active if cyan>=100, +1000 if cyan>=1000, max always (cyan>=0)
    let small_fill_action = if cyan_crys >= 100 {
        format!("gun_fill:100:{pack_x}:{pack_y}")
    } else {
        String::new()
    };
    let large_fill_action = if cyan_crys >= 1000 {
        format!("gun_fill:1000:{pack_x}:{pack_y}")
    } else {
        String::new()
    };
    let fill_max_action = format!("gun_fill:max:{pack_x}:{pack_y}");

    // crystal_type=5 (Cyan)
    let fill_values = format!(
        "{percent}#{bar_label}#5#{small_fill_action}#{large_fill_action}#{fill_max_action}"
    );

    // Раньше окно слалось как `{"tabs":[{объект}]}` — но `HORBConfig.tabs` это
    // `string[]`, и `JsonUtility` такой JSON НЕ парсит → окно пушки не открывалось
    // («у пушек нет гуи»). Через единый builder — плоский корректный контракт.
    use crate::net::session::ui::horb::{Horb, RichRow};
    Horb::new("Пушка")
        .rich_row(RichRow::fill("заряд", fill_values))
        .close_button()
        .send(state, tx, pid, format!("gun:{pack_x}:{pack_y}"));
}

/// Обработать нажатие кнопки заряда пушки (+100, +1000, max).
pub fn handle_gun_fill(
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

    let fill_info = state.building_index.get(&(pack_x, pack_y)).and_then(|ent| {
        let ecs = state.ecs.read();
        let pk_stats = ecs.get::<BuildingStats>(*ent)?;
        Some((pk_stats.charge, pk_stats.max_charge))
    });
    let Some((charge, max_charge)) = fill_info else {
        return;
    };

    if charge >= max_charge {
        return;
    }

    let requested: i64 = match amount_str {
        "100" => 100,
        "1000" => 1000,
        "max" => (max_charge - charge) as i64,
        _ => return,
    };

    if requested <= 0 {
        return;
    }

    // Deduct Cyan (index 5) from player, cap at available
    let actual = state
        .modify_player(pid, |ecs, entity| {
            let mut s = ecs.get_mut::<PlayerStats>(entity)?;
            let available = s.crystals[5]; // Cyan = index 5
            let to_take = requested.min(available);
            if to_take <= 0 {
                return Some(0i64);
            }
            s.crystals[5] -= to_take;
            send_u_packet(
                tx,
                "@B",
                &crate::protocol::packets::basket(&s.crystals, 1).1,
            );
            Some(to_take)
        })
        .flatten()
        .unwrap_or(0);

    if actual <= 0 {
        return;
    }

    // Add charge to gun (cap at max_charge)
    let _ = modify_pack_with_db(state, pack_x, pack_y, |ecs, entity| {
        if let Some(mut pk_stats) = ecs.get_mut::<BuildingStats>(entity) {
            pk_stats.charge += actual as f32;
            if pk_stats.charge > pk_stats.max_charge {
                pk_stats.charge = pk_stats.max_charge;
            }
        }
    });

    // Broadcast HB O to nearby players (C# `ResendPack`)
    if let Some(updated_view) = state.get_pack_at(pack_x, pack_y) {
        crate::net::session::social::buildings::broadcast_pack_update(state, &updated_view);
    }

    // Refresh GUI
    open_gun_gui(state, tx, pid, pack_x, pack_y);
}

/// Programmator `FillGun`: charge gun at (x, y) with a fixed amount.
/// Simplified version — no crystal deduction, no GUI refresh.
#[allow(clippy::needless_pass_by_value)]
pub fn handle_gun_fill_prog(
    state: &Arc<GameState>,
    _tx: &mpsc::UnboundedSender<Vec<u8>>,
    _pid: PlayerId,
    pack_x: i32,
    pack_y: i32,
) {
    let Some(view) = state.get_pack_at(pack_x, pack_y) else {
        return;
    };
    if view.pack_type != crate::game::buildings::PackType::Gun {
        return;
    }
    let _ = modify_pack_with_db(state, pack_x, pack_y, |ecs, entity| {
        if let Some(mut pk_stats) = ecs.get_mut::<crate::game::buildings::BuildingStats>(entity) {
            let increment = (pk_stats.max_charge * 0.05).max(10.0);
            pk_stats.charge = (pk_stats.charge + increment).min(pk_stats.max_charge);
        }
    });
    if let Some(updated_view) = state.get_pack_at(pack_x, pack_y) {
        crate::net::session::social::buildings::broadcast_pack_update(state, &updated_view);
    }
}
