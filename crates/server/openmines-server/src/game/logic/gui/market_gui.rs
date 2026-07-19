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
    clippy::redundant_closure_for_method_calls
)]

use crate::game::buildings::{BuildingFlags, BuildingStats, BuildingStorage};
use crate::game::logic::buildings::modify_pack_with_db;
use crate::game::market;
use crate::game::player::{PlayerFlags, PlayerStats, PlayerUI};
use crate::net::session::prelude::*;
use crate::net::session::ui::crystal_form::parse_amounts as parse_six_i64_fields;

// ─── Market GUI ──────────────────────────────────────────────────────────

pub fn send_market_action_error(tx: &Outbox) {
    send_u_packet(tx, "OK", &ok_message("МАРКЕТ", "Некорректное действие.").1);
}

#[allow(dead_code)]
pub fn send_market_state_error(tx: &Outbox) {
    send_u_packet(
        tx,
        "OK",
        &ok_message("МАРКЕТ", "Состояние маркета недоступно.").1,
    );
}

/// Open Market GUI with tabs (1:1 with C# `Market.GUIWin`).
/// `active_tab` is one of: "sellcrys", "buycrys", "auc".
pub fn open_market_gui(
    state: &Arc<GameState>,
    tx: &dyn crate::net::session::wire::PacketSink,
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
pub fn market_tabs(active_tab: &str) -> Vec<crate::game::logic::horb::Tab> {
    use crate::game::logic::horb::Tab;
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
    tabs: Vec<crate::game::logic::horb::Tab>,
) -> crate::game::logic::horb::Horb {
    use crate::game::logic::horb::{Button, Horb};
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
    tabs: Vec<crate::game::logic::horb::Tab>,
) -> crate::game::logic::horb::Horb {
    use crate::game::logic::horb::{Button, Horb};
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
pub async fn handle_market_tab_switch(
    state: &Arc<GameState>,
    tx: &Outbox,
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
        crate::game::logic::auction_gui::open_auc_grid(state, tx, pid, bx, by).await;
    } else {
        handle_market_tab_switch_sync(state, tx, pid, tab);
    }
}

pub fn handle_market_tab_switch_sync(
    state: &Arc<GameState>,
    tx: &Outbox,
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
#[allow(dead_code)]
pub fn handle_market_sell(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, slider_data: &str) {
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
#[allow(dead_code)]
pub fn handle_market_sellall(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId) {
    let Some((bx, by, _tab)) = resolve_market_window(state, pid) else {
        return;
    };
    let Some(view) = state.get_pack_at(bx, by) else {
        return;
    };
    if view.pack_type != PackType::Market {
        return;
    }

    let Some(outcome) = crate::game::economy::market::sell_all_crystals(state, pid) else {
        tracing::error!(player_id = %pid, "Market sellall failed");
        send_market_action_error(tx);
        return;
    };

    // Обновить moneyinside в здании
    if let Some(building_entity) = state.building_entity_at(bx, by) {
        let mut ecs = state.ecs_write_profiled("market.sellall_building");
        if let Some(mut storage) = ecs.get_mut::<BuildingStorage>(building_entity) {
            storage.money += outcome.earned / 10;
        }
        if let Some(mut flags) = ecs.get_mut::<BuildingFlags>(building_entity) {
            flags.dirty = true;
        }
        drop(ecs);
        state.mark_building_dirty(building_entity);
    }

    send_u_packet(tx, "@B", &basket(&outcome.crystals, 1).1);
    send_u_packet(tx, "P$", &money(outcome.money, outcome.creds).1);

    // Re-render sell tab
    let Some(view) = state.get_pack_at(bx, by) else {
        return;
    };
    open_market_gui(state, tx, pid, &view, "sellcrys");
}

/// Common sell logic (used by sell and sellall).
/// C# ref: `MarketSystem.Sell`:
///   for each i: if `RemoveCrys` succeeds, money += value * GetCrysCost(i)
///   market.moneyinside += (long)(money * 0.1)
#[allow(dead_code)]
pub fn do_market_sell(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    sliders: &[i64],
    bx: i32,
    by: i32,
) {
    // Проверить состояние здания до мутации
    if let Some(building_entity) = state.building_entity_at(bx, by) {
        let ecs = state.ecs_read_profiled("market.sell_check");
        if ecs.get::<BuildingFlags>(building_entity).is_none()
            || ecs.get::<BuildingStorage>(building_entity).is_none()
        {
            send_market_state_error(tx);
            return;
        }
    } else {
        send_market_state_error(tx);
        return;
    }

    let sliders_array: [i64; 6] = sliders.try_into().unwrap_or([0; 6]);

    let Some(outcome) = crate::game::economy::market::sell_crystals(state, pid, &sliders_array)
    else {
        tracing::error!(player_id = %pid, x = bx, y = by, "Market sell failed");
        send_market_state_error(tx);
        return;
    };

    // Обновить moneyinside в здании
    if let Some(building_entity) = state.building_entity_at(bx, by) {
        let mut ecs = state.ecs_write_profiled("market.sell_building");
        if let Some(mut storage) = ecs.get_mut::<BuildingStorage>(building_entity) {
            storage.money += outcome.earned / 10;
        }
        if let Some(mut flags) = ecs.get_mut::<BuildingFlags>(building_entity) {
            flags.dirty = true;
        }
        drop(ecs);
        state.mark_building_dirty(building_entity);
    }

    send_u_packet(tx, "@B", &basket(&outcome.crystals, 1).1);
    send_u_packet(tx, "P$", &money(outcome.money, outcome.creds).1);

    // Re-render sell tab with updated crystal counts
    let Some(view) = state.get_pack_at(bx, by) else {
        return;
    };
    open_market_gui(state, tx, pid, &view, "sellcrys");
}

/// Handle "buy:%M%" — buy crystals with money.
/// C# ref: `MarketSystem.Buy(sliders, p, m)`.
#[allow(dead_code)]
pub fn handle_market_buy(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, slider_data: &str) {
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

    let Some(outcome) = crate::game::economy::market::buy_crystals(state, pid, &sliders) else {
        tracing::error!(player_id = %pid, x = bx, y = by, "Market buy failed");
        send_market_state_error(tx);
        return;
    };

    send_u_packet(tx, "@B", &basket(&outcome.crystals, 1).1);
    send_u_packet(tx, "P$", &money(outcome.money, outcome.creds).1);

    // Re-render buy tab with updated money
    open_market_gui(state, tx, pid, &view, "buycrys");
}

/// Handle "getprofit" — owner withdraws accumulated market profit.
/// C# ref: `Market.onadmn` — transfer moneyinside to player, reset to 0,
/// then re-open the admin `RichList` page.
#[allow(dead_code)]
pub fn handle_market_getprofit(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId) {
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
        .query_building_opt(bx, by, |ecs, entity| {
            Some(
                ecs.get::<BuildingStorage>(entity).is_some()
                    && ecs.get::<BuildingFlags>(entity).is_some(),
            )
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
        let money_result = state
            .modify_player(pid, |ecs, entity| {
                let (money_now, creds_now) = {
                    let mut s = ecs.get_mut::<PlayerStats>(entity)?;
                    s.money += amount;
                    (s.money, s.creds)
                };
                let mut f = ecs.get_mut::<PlayerFlags>(entity)?;
                f.dirty = true;
                Some((money_now, creds_now))
            })
            .flatten();
        if let Some((money_now, creds_now)) = money_result {
            send_u_packet(tx, "P$", &money(money_now, creds_now).1);
        }
    }

    // Re-open admin page with updated profit (now 0)
    open_market_admin_gui(state, tx, pid, bx, by);
}

/// Open Market admin page with `RichList` (1:1 with C# `Market.onadmn`).
/// Shows HP and profit withdrawal button. Called from ADMN gear icon.
pub fn open_market_admin_gui(
    state: &Arc<GameState>,
    tx: &dyn crate::net::session::wire::PacketSink,
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
        let pstats = ecs.get::<BuildingStats>(entity)?;
        let storage = ecs.get::<BuildingStorage>(entity)?;
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

    use crate::game::logic::horb::{Horb, RichRow};
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
