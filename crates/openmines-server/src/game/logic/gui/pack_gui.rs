#![allow(
    clippy::too_many_lines,
    clippy::needless_pass_by_value,
    clippy::option_if_let_else,
    clippy::assigning_clones,
    clippy::items_after_statements,
    clippy::used_underscore_binding,
    clippy::semicolon_if_nothing_returned,
    clippy::missing_panics_doc,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::significant_drop_tightening,
    clippy::map_unwrap_or,
    clippy::manual_let_else,
    clippy::format_push_string,
    clippy::single_match_else,
    clippy::nonminimal_bool,
    clippy::collapsible_if,
    clippy::cast_possible_wrap,
    clippy::redundant_closure_for_method_calls,
    clippy::needless_range_loop
)]

use crate::net::session::ui::gui_buttons::{close_player_window, parse_rich_bool, parse_rich_key_values};
use crate::game::logic::gui::crafter_gui::open_crafter_gui;
use crate::game::logic::gui::market_gui::open_market_gui;
use crate::game::buildings::{BuildingOwnership, BuildingStats, BuildingStorage};
use crate::game::logic::buildings::modify_pack_with_db;
use crate::game::logic::pack_command::{
    send_action_error as send_pack_action_error, send_state_error as send_pack_state_error,
    withdraw_state_ready as pack_withdraw_state_ready,
};
use crate::game::player::{PlayerFlags, PlayerPosition, PlayerStats, PlayerUI};
use crate::net::session::prelude::*;

// ─── Pack Operations ─────────────────────────────────────────────────────────

pub async fn handle_pack_operation(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, op: &str) {
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
        "open" => {
            if view.pack_type == PackType::Clans {
                crate::game::logic::clans::handle_clan_menu(state, tx, pid).await;
            } else {
                open_pack_gui(state, tx, pid, &view);
            }
        }
        "take_money" => handle_pack_take_money(state, tx, pid, &view),
        "take_crys" => handle_pack_take_crystals(state, tx, pid, &view),
        "remove" => {
            crate::game::logic::buildings::handle_remove_building(state, tx, pid, x, y);
        }
        _ => {}
    }
}

pub fn handle_pack_operation_sync_fast_path(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    op: &str,
) -> bool {
    let parts: Vec<&str> = op.split(':').collect();
    if parts.len() < 3 {
        send_pack_action_error(tx);
        return true;
    }
    let cmd = parts[0];
    if cmd == "remove" {
        return false;
    }
    let (x, y) = match (parts[1].parse::<i32>(), parts[2].parse::<i32>()) {
        (Ok(x), Ok(y)) => (x, y),
        (Err(e), _) | (_, Err(e)) => {
            tracing::warn!(player_id = %pid, action = op, error = ?e, "Invalid pack operation coordinates");
            send_pack_action_error(tx);
            return true;
        }
    };

    let Some(view) = state.get_pack_at(x, y) else {
        return true;
    };
    if cmd == "open" && view.pack_type == PackType::Clans {
        return false;
    }

    let p_info = state.query_player_opt(pid, |ecs, entity| {
        let pos = ecs.get::<PlayerPosition>(entity)?;
        let pstats = ecs.get::<PlayerStats>(entity)?;
        Some((pos.x, pos.y, pstats.clan_id.unwrap_or(0)))
    });

    let Some((px, py, p_clan)) = p_info else {
        return true;
    };

    if view.pack_type == PackType::Market && cmd == "open" {
        let Ok(cells) = view.pack_type.building_cells() else {
            tracing::error!(pack_type = ?view.pack_type, "Missing building config for pack GUI");
            return true;
        };
        if !cells
            .iter()
            .any(|(dx, dy, _)| view.x + dx == px && view.y + dy == py)
        {
            return true;
        }
    } else if validate_pack_access(&view, (px, py), p_clan, pid).is_err() {
        return true;
    }

    match cmd {
        "open" => open_pack_gui(state, tx, pid, &view),
        "take_money" => handle_pack_take_money(state, tx, pid, &view),
        "take_crys" => handle_pack_take_crystals(state, tx, pid, &view),
        _ => {}
    }
    true
}

pub fn open_pack_gui(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, view: &PackView) {
    // C# ref: Gate.GUIWin() returns null — no window opens
    if view.pack_type == PackType::Gate {
        close_player_window(state, tx, pid);
        return;
    }
    if view.pack_type == PackType::Storage {
        return;
    }
    // Teleport windows are emitted as immutable `GuiView` effects from the
    // command/movement paths. This legacy direct-delivery helper must not
    // reintroduce a second delivery path.
    if view.pack_type == PackType::Teleport {
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
        return;
    }
    if view.pack_type == PackType::Up {
        crate::game::logic::up_building::open_up_gui(state, tx, pid, view);
        return;
    }
    if view.pack_type == PackType::Resp {
        // Респ: визитёрский GUI с кнопкой «ПРИВЯЗАТЬ» (1:1 C# `Resp.GUIWin`).
        // Без этой ветки респ падал в generic GUI без bind → «невозможно
        // привязаться» (репорт). `handle_pack_action` (был dead code) этот тип
        // обрабатывал, но реальный путь открытия — `open_pack_gui`.
        crate::game::logic::packs::open_resp_gui(state, tx, pid, view);
        return;
    }
    if view.pack_type == PackType::Gun {
        crate::game::logic::packs::open_gun_gui(state, tx, pid, view.x, view.y);
        return;
    }
    if view.pack_type == PackType::Clans {
        // Clans GUI requires DB awaits; async `pack_op:open` handles it explicitly.
        // This sync path is also used from movement/game-loop code, where Tokio
        // reactor is not guaranteed.
        return;
    }

    let title = view.pack_type.name();

    let text = format!(
        "Здание: {}\nЗаряд: {}/{}\nПрочность: {}/{}",
        title, view.charge, view.max_charge, view.hp, view.max_hp
    );
    use crate::game::logic::horb::{Button, Horb};
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
    tx: &Outbox,
    pid: PlayerId,
    pack_x: i32,
    pack_y: i32,
) {
    use crate::game::logic::horb::{Button, Horb, RichRow};
    let Some(view) = state.get_pack_at(pack_x, pack_y) else {
        return;
    };
    if view.owner_id != pid {
        return;
    }

    let details = state.query_building_opt(pack_x, pack_y, |ecs, entity| {
        let st = ecs.get::<BuildingStats>(entity)?;
        let storage = ecs.get::<BuildingStorage>(entity)?;
        let own = ecs.get::<BuildingOwnership>(entity)?;
        Some((st.cost, storage.money, own.clan_id))
    });
    let Some((cost, money, clan_id)) = details else {
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
        .rich_row(RichRow::text(format!(
            "Прочность: {}/{}",
            view.hp, view.max_hp
        )))
        .rich_row(RichRow::text(format!(
            "Заряд: {}/{}",
            view.charge, view.max_charge
        )))
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
pub fn handle_pack_save(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, richlist_data: &str) {
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

pub fn handle_pack_take_money(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, view: &PackView) {
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

pub fn handle_pack_take_crystals(
    state: &Arc<GameState>,
    tx: &Outbox,
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
