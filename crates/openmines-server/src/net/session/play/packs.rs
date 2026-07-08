//! Логика взаимодействия с объектами (паками) на карте.
use crate::game::buildings::{BuildingFlags, BuildingOwnership, BuildingStats, BuildingStorage};
use crate::game::player::{PlayerFlags, PlayerMetadata, PlayerStats, PlayerUI};
use crate::net::session::prelude::*;
use crate::net::session::social::buildings::modify_pack_with_db;

fn send_resp_action_error(tx: &mpsc::UnboundedSender<Vec<u8>>) {
    send_u_packet(tx, "OK", &ok_message("РЕСП", "Некорректное действие.").1);
}

fn send_resp_state_error(tx: &mpsc::UnboundedSender<Vec<u8>>) {
    send_u_packet(
        tx,
        "OK",
        &ok_message("РЕСП", "Состояние респа недоступно.").1,
    );
}

fn send_gun_state_error(tx: &mpsc::UnboundedSender<Vec<u8>>) {
    send_u_packet(
        tx,
        "OK",
        &ok_message("Пушка", "Состояние пушки недоступно.").1,
    );
}

#[derive(Clone, Copy)]
enum FillRequest {
    Fixed(i64),
    Max,
}

enum FillResult {
    Filled { crystals: [i64; 6] },
    Noop,
    MissingState,
}

struct RespSaveFields {
    cost: Option<i32>,
    clanzone: Option<i32>,
    clan_enabled: Option<bool>,
}

fn parse_resp_save_fields(data: &str) -> Option<RespSaveFields> {
    let mut cost = None;
    let mut clanzone = None;
    let mut clan_enabled = None;
    let trimmed = data.strip_suffix('#').unwrap_or(data);
    if trimmed.is_empty() {
        return None;
    }
    for pair in trimmed.split('#') {
        let (key, value) = pair.split_once(':')?;
        if key.is_empty() || value.is_empty() {
            return None;
        }
        match key {
            "cost" => {
                let parsed = value.parse::<i32>().ok()?;
                if !(0..=5000).contains(&parsed) {
                    return None;
                }
                cost = Some(parsed);
            }
            "clanzone" => clanzone = Some(value.parse::<i32>().ok()?),
            "clan" => {
                clan_enabled = Some(match value {
                    "1" | "true" => true,
                    "0" | "false" => false,
                    _ => return None,
                });
            }
            _ => return None,
        }
    }
    if cost.is_none() && clanzone.is_none() && clan_enabled.is_none() {
        return None;
    }
    Some(RespSaveFields {
        cost,
        clanzone,
        clan_enabled,
    })
}

fn apply_charge_fill(
    state: &Arc<GameState>,
    pid: PlayerId,
    pack_x: i32,
    pack_y: i32,
    crystal_index: usize,
    request: FillRequest,
) -> FillResult {
    let Some(player_entity) = state.get_player_entity(pid) else {
        return FillResult::MissingState;
    };
    let Some(building_entity) = state.building_entity_at(pack_x, pack_y) else {
        return FillResult::MissingState;
    };

    let mut ecs = state.ecs.write();
    if ecs.get::<PlayerStats>(player_entity).is_none()
        || ecs.get::<PlayerFlags>(player_entity).is_none()
        || ecs.get::<BuildingStats>(building_entity).is_none()
        || ecs.get::<BuildingFlags>(building_entity).is_none()
    {
        return FillResult::MissingState;
    }

    let (charge, max_charge) = {
        let building_stats = ecs
            .get::<BuildingStats>(building_entity)
            .expect("BuildingStats checked before charge fill");
        (building_stats.charge, building_stats.max_charge)
    };
    if charge >= max_charge {
        return FillResult::Noop;
    }
    let requested = match request {
        FillRequest::Fixed(value) => value,
        FillRequest::Max => i64::from(max_charge - charge),
    };
    if requested <= 0 {
        return FillResult::Noop;
    }

    let available = ecs
        .get::<PlayerStats>(player_entity)
        .expect("PlayerStats checked before charge fill")
        .crystals[crystal_index];
    let to_take = requested.min(available);
    if to_take <= 0 {
        return FillResult::Noop;
    }

    let crystals = {
        let mut player_stats = ecs
            .get_mut::<PlayerStats>(player_entity)
            .expect("PlayerStats checked before charge fill");
        player_stats.crystals[crystal_index] -= to_take;
        player_stats.crystals
    };
    {
        let mut building_stats = ecs
            .get_mut::<BuildingStats>(building_entity)
            .expect("BuildingStats checked before charge fill");
        let to_take_i32 = i32::try_from(to_take).unwrap_or(i32::MAX);
        building_stats.charge = building_stats
            .charge
            .saturating_add(to_take_i32)
            .min(building_stats.max_charge);
    }
    ecs.get_mut::<PlayerFlags>(player_entity)
        .expect("PlayerFlags checked before charge fill")
        .dirty = true;
    ecs.get_mut::<BuildingFlags>(building_entity)
        .expect("BuildingFlags checked before charge fill")
        .dirty = true;

    FillResult::Filled { crystals }
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
    let player_resp = state.query_player_opt(pid, |ecs, entity| {
        let meta = ecs.get::<PlayerMetadata>(entity)?;
        let bound = meta.resp_x == Some(view.x) && meta.resp_y == Some(view.y);
        Some((bound, view.owner_id == pid))
    });
    let Some((is_bound, is_owner)) = player_resp else {
        tracing::error!(player_id = %pid, "Player metadata missing for resp GUI");
        send_resp_action_error(tx);
        return;
    };

    // Get cost from ECS
    let cost = state.query_building_opt(view.x, view.y, |ecs, entity| {
        let pk_stats = ecs.get::<BuildingStats>(entity)?;
        Some(pk_stats.cost)
    });
    let Some(cost) = cost else {
        tracing::error!(
            x = view.x,
            y = view.y,
            "Building stats missing for resp GUI"
        );
        send_resp_action_error(tx);
        return;
    };

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
    let details = state.query_building_opt(pack_x, pack_y, |ecs, entity| {
        let pk_stats = ecs.get::<BuildingStats>(entity)?;
        let storage = ecs.get::<BuildingStorage>(entity)?;
        let ownership = ecs.get::<BuildingOwnership>(entity)?;
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
        tracing::error!(
            x = pack_x,
            y = pack_y,
            "Building components missing for resp admin GUI"
        );
        send_resp_action_error(tx);
        return;
    };

    // Get player's blue crystals for fill button availability
    let blue_crys = state.query_player_opt(pid, |ecs, entity| {
        ecs.get::<PlayerStats>(entity).map(|s| s.crystals[1]) // Blue = index 1
    });
    let Some(blue_crys) = blue_crys else {
        tracing::error!(player_id = %pid, "Player stats missing for resp admin GUI");
        send_resp_action_error(tx);
        return;
    };

    // Build fill bar: percent, label, crystal type, button actions
    let charge_i = charge;
    let max_charge_i = max_charge;
    let percent = if max_charge > 0 {
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

    let updated = state
        .modify_player(pid, |ecs, entity| {
            if ecs.get::<PlayerMetadata>(entity).is_none()
                || ecs.get::<PlayerFlags>(entity).is_none()
            {
                return Some(false);
            }
            {
                let mut meta = ecs
                    .get_mut::<PlayerMetadata>(entity)
                    .expect("PlayerMetadata checked before resp bind");
                meta.resp_x = Some(pack_x);
                meta.resp_y = Some(pack_y);
            }
            // 1:1 C# `SetResp` + `SaveChanges`: помечаем dirty, иначе flush
            // (dirty-gated) не сохранит привязку и она теряется при релоге.
            ecs.get_mut::<PlayerFlags>(entity)
                .expect("PlayerFlags checked before resp bind")
                .dirty = true;
            Some(true)
        })
        .flatten()
        .unwrap_or(false);
    if !updated {
        tracing::error!(player_id = %pid, pack_x, pack_y, "Resp bind player state missing");
        send_resp_state_error(tx);
        return;
    }

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
    if view.pack_type != crate::game::buildings::PackType::Resp {
        return;
    }

    let request = match amount_str {
        "100" => FillRequest::Fixed(100),
        "1000" => FillRequest::Fixed(1000),
        "max" => FillRequest::Max,
        _ => return,
    };
    match apply_charge_fill(state, pid, pack_x, pack_y, 1, request) {
        FillResult::Filled { crystals } => send_u_packet(tx, "@B", &basket(&crystals, 1).1),
        FillResult::Noop => return,
        FillResult::MissingState => {
            tracing::error!(player_id = %pid, pack_x, pack_y, "Resp fill state missing");
            send_resp_state_error(tx);
            return;
        }
    }

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
    if !resp_profit_state_ready(state, pid, pack_x, pack_y) {
        send_resp_state_error(tx);
        return;
    }

    let Some(player_entity) = state.get_player_entity(pid) else {
        send_resp_state_error(tx);
        return;
    };
    let Some(building_entity) = state.building_entity_at(pack_x, pack_y) else {
        send_resp_state_error(tx);
        return;
    };

    let result = {
        let mut ecs = state.ecs.write();
        if ecs.get::<PlayerStats>(player_entity).is_none()
            || ecs.get::<PlayerFlags>(player_entity).is_none()
            || ecs.get::<BuildingStorage>(building_entity).is_none()
            || ecs.get::<BuildingFlags>(building_entity).is_none()
        {
            None
        } else {
            let amount = {
                let mut storage = ecs
                    .get_mut::<BuildingStorage>(building_entity)
                    .expect("BuildingStorage checked before resp profit withdrawal");
                let amount = storage.money;
                storage.money = 0;
                amount
            };
            let (money_now, creds_now) = if amount > 0 {
                let mut player_stats = ecs
                    .get_mut::<PlayerStats>(player_entity)
                    .expect("PlayerStats checked before resp profit withdrawal");
                player_stats.money += amount;
                (player_stats.money, player_stats.creds)
            } else {
                let player_stats = ecs
                    .get::<PlayerStats>(player_entity)
                    .expect("PlayerStats checked before resp profit read");
                (player_stats.money, player_stats.creds)
            };
            if amount > 0 {
                ecs.get_mut::<PlayerFlags>(player_entity)
                    .expect("PlayerFlags checked before resp profit withdrawal")
                    .dirty = true;
                ecs.get_mut::<BuildingFlags>(building_entity)
                    .expect("BuildingFlags checked before resp profit withdrawal")
                    .dirty = true;
            }
            Some((amount, money_now, creds_now))
        }
    };
    let Some((amount, money_now, creds_now)) = result else {
        tracing::error!(player_id = %pid, pack_x, pack_y, "Resp profit state missing");
        send_resp_state_error(tx);
        return;
    };
    if amount > 0 {
        send_u_packet(tx, "P$", &money(money_now, creds_now).1);
    }

    // Refresh admin GUI
    open_resp_admin_gui(state, tx, pid, pack_x, pack_y);
}

fn resp_profit_state_ready(
    state: &Arc<GameState>,
    pid: PlayerId,
    pack_x: i32,
    pack_y: i32,
) -> bool {
    let player_ready = state
        .query_player(pid, |ecs, entity| {
            ecs.get::<PlayerStats>(entity).is_some() && ecs.get::<PlayerFlags>(entity).is_some()
        })
        .unwrap_or(false);
    let building_ready = state
        .query_building_opt(pack_x, pack_y, |ecs, entity| {
            Some(
                ecs.get::<BuildingStorage>(entity).is_some()
                    && ecs.get::<BuildingFlags>(entity).is_some(),
            )
        })
        .unwrap_or(false);
    player_ready && building_ready
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

    let Some(fields) = parse_resp_save_fields(richlist_data) else {
        send_resp_action_error(tx);
        return;
    };

    // Pre-fetch owner's clan_id before acquiring ECS write lock to avoid deadlock
    let owner_clan = if fields.clan_enabled.is_some() {
        let Some(clan_id) = state.query_player_opt(pid, |ecs, e| {
            Some(ecs.get::<PlayerStats>(e)?.clan_id.unwrap_or(0))
        }) else {
            send_resp_state_error(tx);
            return;
        };
        Some(clan_id)
    } else {
        None
    };
    let building_state_ready = state
        .query_building_opt(pack_x, pack_y, |ecs, entity| {
            Some(
                (fields.cost.is_none() && fields.clanzone.is_none()
                    || ecs.get::<BuildingStats>(entity).is_some())
                    && (fields.clan_enabled.is_none()
                        || ecs.get::<BuildingOwnership>(entity).is_some())
                    && ecs.get::<BuildingFlags>(entity).is_some(),
            )
        })
        .unwrap_or(false);
    if !building_state_ready {
        send_resp_state_error(tx);
        return;
    }

    let updated = match modify_pack_with_db(state, pack_x, pack_y, |ecs, entity| {
        let mut updated = false;
        if fields.cost.is_some() || fields.clanzone.is_some() {
            let mut pk_stats = ecs
                .get_mut::<BuildingStats>(entity)
                .expect("BuildingStats checked before resp save");
            if let Some(cost) = fields.cost {
                pk_stats.cost = cost;
                updated = true;
            }
            if let Some(clanzone) = fields.clanzone {
                pk_stats.clanzone = clanzone;
                updated = true;
            }
        }
        if let Some(clan_enabled) = fields.clan_enabled {
            let mut ownership = ecs
                .get_mut::<BuildingOwnership>(entity)
                .expect("BuildingOwnership checked before resp save");
            ownership.clan_id = if clan_enabled {
                owner_clan.expect("Owner clan checked before resp save")
            } else {
                0
            };
            updated = true;
        }
        updated
    }) {
        Ok(updated) => updated,
        Err(e) => {
            tracing::error!(pack_x, pack_y, error = %e, "Resp admin save failed");
            send_resp_state_error(tx);
            return;
        }
    };
    if !updated {
        send_u_packet(
            tx,
            "OK",
            &ok_message("Респ", "Не удалось сохранить настройки").1,
        );
        return;
    }

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
    let fill_info = state.query_building_opt(pack_x, pack_y, |ecs, entity| {
        let pk_stats = ecs.get::<BuildingStats>(entity)?;
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

    let charge_i = charge;
    let max_charge_i = max_charge;
    let percent = if max_charge > 0 {
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
    if view.pack_type != crate::game::buildings::PackType::Gun {
        return;
    }

    let request = match amount_str {
        "100" => FillRequest::Fixed(100),
        "1000" => FillRequest::Fixed(1000),
        "max" => FillRequest::Max,
        _ => return,
    };
    match apply_charge_fill(state, pid, pack_x, pack_y, 5, request) {
        FillResult::Filled { crystals } => send_u_packet(tx, "@B", &basket(&crystals, 1).1),
        FillResult::Noop => return,
        FillResult::MissingState => {
            tracing::error!(player_id = %pid, pack_x, pack_y, "Gun fill state missing");
            send_gun_state_error(tx);
            return;
        }
    }

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
    let building_state_ready = state
        .query_building_opt(pack_x, pack_y, |ecs, entity| {
            Some(
                ecs.get::<BuildingStats>(entity).is_some()
                    && ecs.get::<BuildingFlags>(entity).is_some(),
            )
        })
        .unwrap_or(false);
    if !building_state_ready {
        tracing::error!(pack_x, pack_y, "Programmator gun fill state missing");
        return;
    }
    let updated = modify_pack_with_db(state, pack_x, pack_y, |ecs, entity| {
        let mut pk_stats = ecs
            .get_mut::<BuildingStats>(entity)
            .expect("BuildingStats checked before programmator gun fill");
        let increment = (pk_stats.max_charge / 20).max(10);
        pk_stats.charge = pk_stats
            .charge
            .saturating_add(increment)
            .min(pk_stats.max_charge);
        true
    });
    if !matches!(updated, Ok(true)) {
        tracing::error!(pack_x, pack_y, result = ?updated, "Programmator gun fill failed");
        return;
    }
    if let Some(updated_view) = state.get_pack_at(pack_x, pack_y) {
        crate::net::session::social::buildings::broadcast_pack_update(state, &updated_view);
    }
}

#[cfg(test)]
mod tests {
    use bytes::BytesMut;

    use super::*;

    struct ChargeFillTestState {
        state: Arc<GameState>,
        player: crate::db::PlayerRow,
        db_path: std::path::PathBuf,
        dir: std::path::PathBuf,
        world_name: String,
    }

    impl ChargeFillTestState {
        fn cleanup(self) {
            let _ = std::fs::remove_file(self.db_path);
            let _ = std::fs::remove_file(self.dir.join(format!("{}_v2.map", self.world_name)));
            let _ = std::fs::remove_file(self.dir.join(format!("{}_road_v2.map", self.world_name)));
            let _ =
                std::fs::remove_file(self.dir.join(format!("{}_durability.map", self.world_name)));
        }
    }

    #[tokio::test]
    async fn resp_fill_missing_player_flags_is_explicit_error_without_charge_or_crystal_mutation() {
        let test = make_charge_fill_test_state("resp_missing_flags", "R", 1, 100).await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let player_entity = test.state.get_player_entity(test.player.id.into()).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(player_entity).remove::<PlayerFlags>();
        }
        let before_crystals = player_crystals(&test.state, test.player.id.into());
        let before_charge = building_charge(&test.state, 10, 10);

        handle_resp_fill(&test.state, &tx, test.player.id.into(), "100", 10, 10);

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert_eq!(events[0].1, "РЕСП#Состояние респа недоступно.".as_bytes());
        assert_eq!(
            player_crystals(&test.state, test.player.id.into()),
            before_crystals
        );
        assert_eq!(building_charge(&test.state, 10, 10), before_charge);

        test.cleanup();
    }

    #[tokio::test]
    async fn resp_bind_missing_player_flags_is_explicit_error_without_resp_mutation() {
        let test = make_charge_fill_test_state("resp_bind_missing_flags", "R", 1, 100).await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let player_entity = test.state.get_player_entity(test.player.id.into()).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(player_entity).remove::<PlayerFlags>();
        }
        let before_resp = player_resp(&test.state, test.player.id.into());

        handle_resp_bind(&test.state, &tx, test.player.id.into(), 10, 10);

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert_eq!(events[0].1, "РЕСП#Состояние респа недоступно.".as_bytes());
        assert_eq!(player_resp(&test.state, test.player.id.into()), before_resp);

        test.cleanup();
    }

    #[tokio::test]
    async fn gun_fill_missing_player_flags_is_explicit_error_without_charge_or_crystal_mutation() {
        let test = make_charge_fill_test_state("gun_missing_flags", "G", 5, 100).await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let player_entity = test.state.get_player_entity(test.player.id.into()).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(player_entity).remove::<PlayerFlags>();
        }
        let before_crystals = player_crystals(&test.state, test.player.id.into());
        let before_charge = building_charge(&test.state, 10, 10);

        handle_gun_fill(&test.state, &tx, test.player.id.into(), "100", 10, 10);

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert_eq!(events[0].1, "Пушка#Состояние пушки недоступно.".as_bytes());
        assert_eq!(
            player_crystals(&test.state, test.player.id.into()),
            before_crystals
        );
        assert_eq!(building_charge(&test.state, 10, 10), before_charge);

        test.cleanup();
    }

    #[tokio::test]
    async fn gun_fill_allows_non_owner_like_reference() {
        let test = make_charge_fill_test_state("gun_non_owner_fill", "G", 5, 0).await;
        let mut filler = test
            .state
            .db
            .create_player("gun-fill-visitor", "p", "h")
            .await
            .unwrap();
        filler.x = 10;
        filler.y = 10;
        filler.crystals[5] = 100;

        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &filler, 2);
        drain_events(&mut rx);

        handle_gun_fill(&test.state, &tx, filler.id.into(), "100", 10, 10);

        let events = drain_events(&mut rx);
        assert_eq!(building_charge(&test.state, 10, 10), 100);
        assert_eq!(player_crystals(&test.state, filler.id.into())[5], 0);
        assert!(
            events.iter().any(|(event, _)| event == "@B"),
            "events: {events:?}"
        );
        assert!(
            events.iter().any(|(event, _)| event == "GU"),
            "events: {events:?}"
        );

        test.cleanup();
    }

    #[tokio::test]
    async fn resp_profit_missing_player_flags_is_explicit_error_without_money_mutation() {
        let test = make_charge_fill_test_state("resp_profit_missing_flags", "R", 1, 100).await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let player_entity = test.state.get_player_entity(test.player.id.into()).unwrap();
        let building_entity = test.state.building_entity_at(10, 10).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.get_mut::<BuildingStorage>(building_entity)
                .unwrap()
                .money = 777;
            ecs.entity_mut(player_entity).remove::<PlayerFlags>();
        }
        let before_money = player_money(&test.state, test.player.id.into());

        handle_resp_profit(&test.state, &tx, test.player.id.into(), 10, 10);

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert_eq!(events[0].1, "РЕСП#Состояние респа недоступно.".as_bytes());
        assert_eq!(
            player_money(&test.state, test.player.id.into()),
            before_money
        );
        assert_eq!(building_storage_money(&test.state, 10, 10), 777);

        test.cleanup();
    }

    #[tokio::test]
    async fn resp_profit_success_moves_money_and_marks_player_and_building_dirty() {
        let test = make_charge_fill_test_state("resp_profit_success", "R", 1, 100).await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let building_entity = test.state.building_entity_at(10, 10).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.get_mut::<BuildingStorage>(building_entity)
                .unwrap()
                .money = 777;
        }
        let before_money = player_money(&test.state, test.player.id.into());

        handle_resp_profit(&test.state, &tx, test.player.id.into(), 10, 10);

        let events = drain_events(&mut rx);
        assert!(events.iter().any(|(event, _)| event == "P$"));
        assert_eq!(
            player_money(&test.state, test.player.id.into()),
            before_money + 777
        );
        assert_eq!(building_storage_money(&test.state, 10, 10), 0);
        assert!(player_dirty(&test.state, test.player.id.into()));
        assert!(building_dirty(&test.state, 10, 10));

        test.cleanup();
    }

    #[tokio::test]
    async fn resp_save_rejects_malformed_cost_without_cost_mutation() {
        let test = make_charge_fill_test_state("resp_save_bad_cost", "R", 1, 100).await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        set_player_window(&test.state, test.player.id.into(), "resp:10:10");
        let before_cost = building_cost(&test.state, 10, 10);

        handle_resp_save(&test.state, &tx, test.player.id.into(), "cost:nope");

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert_eq!(events[0].1, "РЕСП#Некорректное действие.".as_bytes());
        assert_eq!(building_cost(&test.state, 10, 10), before_cost);

        test.cleanup();
    }

    #[tokio::test]
    async fn resp_save_missing_player_stats_is_explicit_error_without_partial_cost_mutation() {
        let test = make_charge_fill_test_state("resp_save_missing_player_stats", "R", 1, 100).await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        set_player_window(&test.state, test.player.id.into(), "resp:10:10");
        let player_entity = test.state.get_player_entity(test.player.id.into()).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(player_entity).remove::<PlayerStats>();
        }
        let before_cost = building_cost(&test.state, 10, 10);

        handle_resp_save(&test.state, &tx, test.player.id.into(), "cost:123#clan:1");

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert_eq!(events[0].1, "РЕСП#Состояние респа недоступно.".as_bytes());
        assert_eq!(building_cost(&test.state, 10, 10), before_cost);

        test.cleanup();
    }

    #[tokio::test]
    async fn resp_save_updates_clanzone_marks_dirty_and_refreshes_admin_gui() {
        let test = make_charge_fill_test_state("resp_save_clanzone", "R", 1, 100).await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        set_player_window(&test.state, test.player.id.into(), "resp:10:10");
        assert_eq!(building_clanzone(&test.state, 10, 10), 0);

        handle_resp_save(&test.state, &tx, test.player.id.into(), "clanzone:321#");

        let events = drain_events(&mut rx);
        assert_eq!(building_clanzone(&test.state, 10, 10), 321);
        assert!(building_dirty(&test.state, 10, 10));
        assert!(
            events.iter().any(|(event, payload)| {
                event == "GU" && String::from_utf8_lossy(payload).contains("321")
            }),
            "admin GUI refresh must include updated clanzone"
        );

        test.cleanup();
    }

    #[tokio::test]
    async fn gun_fill_prog_missing_building_stats_does_not_dirty_building() {
        let test = make_charge_fill_test_state("gun_prog_missing_stats", "G", 5, 100).await;
        let (tx, _rx) = mpsc::unbounded_channel();
        let building_entity = test.state.building_entity_at(10, 10).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(building_entity).remove::<BuildingStats>();
        }

        handle_gun_fill_prog(&test.state, &tx, test.player.id.into(), 10, 10);

        assert!(!building_dirty(&test.state, 10, 10));

        test.cleanup();
    }

    async fn make_charge_fill_test_state(
        label: &str,
        building_code: &str,
        crystal_index: usize,
        crystal_amount: i64,
    ) -> ChargeFillTestState {
        let dir = std::env::temp_dir();
        let nonce = format!("{}_{}_{}", label, std::process::id(), unique_test_nonce());
        let db_path = dir.join(format!("charge_fill_{nonce}.db"));
        let _ = std::fs::remove_file(&db_path);

        let database = crate::db::Database::open(&db_path).await.unwrap();
        let mut player = database
            .create_player("charge-fill-user", "p", "h")
            .await
            .unwrap();
        player.x = 10;
        player.y = 10;
        player.crystals[crystal_index] = crystal_amount;

        let extra = crate::db::BuildingExtra {
            charge: 0,
            max_charge: 1000,
            cost: 0,
            hp: 1000,
            max_hp: 1000,
            money_inside: 0,
            crystals_inside: [0; 6],
            items_inside: std::collections::HashMap::new(),
            craft_recipe_id: None,
            craft_num: 0,
            craft_end_ts: 0,
            craft_ready: false,
            clanzone: 0,
        };
        database
            .insert_building(building_code, 10, 10, player.id, 0, &extra)
            .await
            .unwrap();

        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let repo_root = if manifest_dir.join("configs").is_dir() {
            manifest_dir.to_path_buf()
        } else {
            let parent = manifest_dir
                .parent()
                .expect("server crate must live under repo root");
            if parent.join("configs").is_dir() {
                parent.to_path_buf()
            } else {
                parent
                    .parent()
                    .expect("must live under repo root")
                    .to_path_buf()
            }
        };
        let cells_path = repo_root.join("configs/cells.json");
        let cell_defs =
            crate::world::cells::CellDefs::load(cells_path.to_string_lossy().as_ref()).unwrap();
        let world_name = format!("charge_fill_world_{nonce}");
        let world = crate::world::World::new(&world_name, 2, 2, cell_defs, &dir).unwrap();
        let config = crate::config::Config {
            world_name: world_name.clone(),
            port: 8090,
            world_chunks_w: 2,
            world_chunks_h: 2,
            data_dir: dir.to_string_lossy().to_string(),
            logging: crate::config::LoggingConfig::runtime_baseline(),
            cron: crate::config::CronConfig::runtime_baseline(),
            gameplay: crate::config::GameplayConfig::runtime_baseline(),
        };
        let buildings_path = repo_root.join("configs/buildings.json");
        let _ = crate::game::buildings::load_buildings_config(
            buildings_path.to_string_lossy().as_ref(),
        );
        let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config)
            .await
            .unwrap();

        ChargeFillTestState {
            state,
            player,
            db_path,
            dir,
            world_name,
        }
    }

    fn player_crystals(state: &Arc<GameState>, pid: PlayerId) -> [i64; 6] {
        state
            .query_player_opt(pid, |ecs, entity| {
                Some(ecs.get::<PlayerStats>(entity)?.crystals)
            })
            .unwrap()
    }

    fn building_charge(state: &Arc<GameState>, x: i32, y: i32) -> i32 {
        state
            .query_building_opt(x, y, |ecs, entity| {
                Some(ecs.get::<BuildingStats>(entity)?.charge)
            })
            .unwrap()
    }

    fn building_cost(state: &Arc<GameState>, x: i32, y: i32) -> i32 {
        state
            .query_building_opt(x, y, |ecs, entity| {
                Some(ecs.get::<BuildingStats>(entity)?.cost)
            })
            .unwrap()
    }

    fn building_clanzone(state: &Arc<GameState>, x: i32, y: i32) -> i32 {
        state
            .query_building_opt(x, y, |ecs, entity| {
                Some(ecs.get::<BuildingStats>(entity)?.clanzone)
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

    fn player_resp(state: &Arc<GameState>, pid: PlayerId) -> (Option<i32>, Option<i32>) {
        state
            .query_player_opt(pid, |ecs, entity| {
                let meta = ecs.get::<PlayerMetadata>(entity)?;
                Some((meta.resp_x, meta.resp_y))
            })
            .unwrap()
    }

    fn building_storage_money(state: &Arc<GameState>, x: i32, y: i32) -> i64 {
        state
            .query_building_opt(x, y, |ecs, entity| {
                Some(ecs.get::<BuildingStorage>(entity)?.money)
            })
            .unwrap()
    }

    fn player_dirty(state: &Arc<GameState>, pid: PlayerId) -> bool {
        state
            .query_player_opt(pid, |ecs, entity| {
                Some(ecs.get::<PlayerFlags>(entity)?.dirty)
            })
            .unwrap()
    }

    fn building_dirty(state: &Arc<GameState>, x: i32, y: i32) -> bool {
        state
            .query_building_opt(x, y, |ecs, entity| {
                Some(ecs.get::<BuildingFlags>(entity)?.dirty)
            })
            .unwrap()
    }

    fn set_player_window(state: &Arc<GameState>, pid: PlayerId, window: &str) {
        state.modify_player(pid, |ecs, entity| {
            let mut ui = ecs.get_mut::<PlayerUI>(entity)?;
            ui.current_window = Some(window.to_string());
            Some(())
        });
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

    fn unique_test_nonce() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }
}
