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
//! Обработка нажатий GUI-кнопок игроком.
use crate::game::logic::gui::crafter_gui;
use crate::game::logic::gui::market_gui;
use crate::game::logic::gui::pack_gui;
use crate::game::logic::gui::programmator_gui;
use crate::game::player::{PlayerInventory, PlayerUI};
use crate::net::session::prelude::*;

pub fn parse_rich_key_values(data: &str) -> Option<std::collections::HashMap<&str, &str>> {
    let mut fields = std::collections::HashMap::new();
    if data.is_empty() {
        return Some(fields);
    }
    for pair in data.split('#') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair.split_once(':')?;
        if key.is_empty() || value.is_empty() {
            return None;
        }
        fields.insert(key, value);
    }
    Some(fields)
}

pub fn parse_rich_bool(value: &str) -> Option<bool> {
    match value {
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
        _ => None,
    }
}

pub async fn handle_gui_button(
    state: &Arc<GameState>,
    tx: &dyn PacketSink,
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

    if handle_clan_button(state, tx, pid, button).await {
        return;
    }

    match button {
        "open_buildings" => {
            crate::game::logic::buildings::handle_buildings_menu(state, tx, pid);
        }
        "createprog" => programmator_gui::open_create_prog_dialog(state, tx, pid),
        // Runtime routes this action through `PlayerCommand::Gui` before this
        // legacy async dispatcher, where it becomes durable `ProgramMenu`.
        "prog" => {}
        "clan_create_view" => handle_clan_create_view(state, tx, pid),
        // Market tab switching (C# tabs have action strings)
        "sellcrys" => market_gui::handle_market_tab_switch(state, tx, pid, "sellcrys").await,
        "buycrys" => market_gui::handle_market_tab_switch(state, tx, pid, "buycrys").await,
        "auc" => market_gui::handle_market_tab_switch(state, tx, pid, "auc").await,
        "clancreate" | "clan_create" => {
            handle_clan_create_view(state, tx, pid);
        }
        "clan_create_input" => {
            crate::game::logic::commands_social::send_ok(
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

pub fn handle_gui_button_sync_fast_path(
    state: &Arc<GameState>,
    tx: &dyn PacketSink,
    pid: PlayerId,
    button: &str,
) -> bool {
    if button == "exit" || button == "exit:0" || button == "close" {
        state.modify_player(pid, |ecs, entity| {
            if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) {
                ui.current_window = None;
            }
            if let Some(mut inv) = ecs.get_mut::<PlayerInventory>(entity) {
                inv.selected = -1;
            }
            Some(())
        });
        let g = gu_close();
        send_u_packet(tx, g.0, &g.1);
        return true;
    }

    let has_window = state
        .query_player_opt(pid, |ecs, entity| {
            ecs.get::<PlayerUI>(entity)
                .map(|ui| ui.current_window.is_some())
        })
        .unwrap_or(false);
    if !has_window {
        let g = gu_close();
        send_u_packet(tx, g.0, &g.1);
        return true;
    }

    if let Some(rest) = button.strip_prefix("pack_op:") {
        return pack_gui::handle_pack_operation_sync_fast_path(state, tx, pid, rest);
    }
    if let Some(rest) = button.strip_prefix("craft_recipe:") {
        crafter_gui::handle_craft_recipe_view(state, tx, pid, rest);
        return true;
    }
    if let Some(rest) = button.strip_prefix("craft_start:") {
        crafter_gui::handle_craft_start(state, tx, pid, rest);
        return true;
    }
    if let Some(rest) = button.strip_prefix("craft_claim:") {
        crafter_gui::handle_craft_claim(state, tx, pid, rest);
        return true;
    }
    if let Some(rest) = button.strip_prefix("pack_save:") {
        pack_gui::handle_pack_save(state, tx, pid, rest);
        return true;
    }
    if let Some(rest) = button.strip_prefix("save:") {
        crate::net::session::ui::settings::apply(state, tx, pid, rest);
        return true;
    }

    match button {
        "open_buildings" => {
            crate::game::logic::buildings::handle_buildings_menu(state, tx, pid);
            true
        }
        "createprog" => {
            programmator_gui::open_create_prog_dialog(state, tx, pid);
            true
        }
        "clan_create_view" | "clancreate" | "clan_create" => {
            handle_clan_create_view(state, tx, pid);
            true
        }
        "clan_create_input" => {
            crate::game::logic::commands_social::send_ok(
                tx,
                "КЛАНЫ",
                "Введите /clan create НАЗВАНИЕ ТЕГ в чате",
            );
            true
        }
        "sellcrys" | "buycrys" => {
            market_gui::handle_market_tab_switch_sync(state, tx, pid, button);
            true
        }
        _ => false,
    }
}

pub fn open_clan_create_view(state: &Arc<GameState>, tx: &dyn PacketSink, pid: PlayerId) {
    use crate::game::logic::horb::{Button, Horb};
    // exit добавится builder-гарантией последним → Escape закроет окно.
    Horb::new("СОЗДАНИЕ КЛАНА")
        .text("Введите название и тег (3 симв.) через пробел в чат после нажатия кнопки 'ВВОД'")
        .button(Button::new("ВВОД", "clan_create_input"))
        .button(Button::new("Назад", "clan_back"))
        .send(state, tx, pid, "clan");
}

fn handle_clan_create_view(state: &Arc<GameState>, tx: &dyn PacketSink, pid: PlayerId) {
    open_clan_create_view(state, tx, pid);
}

/// Закрыть текущее GUI-окно игрока (сбросить `current_window` + `Gu`).
pub fn close_player_window(state: &Arc<GameState>, tx: &dyn PacketSink, pid: PlayerId) {
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
    tx: &dyn PacketSink,
    pid: PlayerId,
    button: &str,
) {
    if handle_clan_button(state, tx, pid, button).await {
    } else if let Some(rest) = button.strip_prefix("bld_place:") {
        crate::game::logic::buildings::handle_place_building(state, tx, pid, rest).await;
    } else if let Some(rest) = button.strip_prefix("pack_op:") {
        pack_gui::handle_pack_operation(state, tx, pid, rest).await;
    } else if let Some(rest) = button.strip_prefix("craft_recipe:") {
        crafter_gui::handle_craft_recipe_view(state, tx, pid, rest);
    } else if let Some(rest) = button.strip_prefix("craft_start:") {
        crafter_gui::handle_craft_start(state, tx, pid, rest);
    } else if let Some(rest) = button.strip_prefix("craft_claim:") {
        crafter_gui::handle_craft_claim(state, tx, pid, rest);
    } else if let Some(rest) = button.strip_prefix("resp_bind:") {
        let parts: Vec<&str> = rest.split(':').collect();
        if parts.len() == 2 {
            if let (Ok(x), Ok(y)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>()) {
                crate::game::logic::packs::handle_resp_bind(state, tx, pid, x, y);
            }
        }
    } else if let Some(rest) = button.strip_prefix("pack_save:") {
        // Единая админ-панель пака: сохранить cost/clan из %R%.
        pack_gui::handle_pack_save(state, tx, pid, rest);
    } else if let Some(rest) = button.strip_prefix("save:") {
        crate::net::session::ui::settings::apply(state, tx, pid, rest);
    } else if handle_auction_button(state, tx, pid, button).await {
    } else if let Some(rest) = button.strip_prefix("openprog:") {
        if let Ok(id) = rest.parse::<i32>() {
            programmator_gui::handle_open_prog(state, tx, pid, id).await;
        }
    } else if let Some(name) = button.strip_prefix("createprog:") {
        programmator_gui::handle_create_prog(state, tx, pid, name).await;
    } else if let Some(rest) = button.strip_prefix("rename:") {
        // format: "<id>:<name>" (сервер кодирует как `rename:{id}:%I%`, клиент подставляет ввод)
        let parts: Vec<&str> = rest.splitn(2, ':').collect();
        if let [id_str, name] = parts.as_slice() {
            if let Ok(id) = id_str.parse::<i32>() {
                programmator_gui::handle_rename_prog(state, tx, pid, id, name).await;
            }
        }
    } else {
        // Up building buttons are now handled in apply_gui_button_command
    }
}

pub fn is_clan_button(button: &str) -> bool {
    matches!(
        button,
        "clan_menu"
            | "clan_back"
            | "clan_requests"
            | "clan_members"
            | "clan_invite_list"
            | "clan_invites_view"
    ) || button.starts_with("clan_view:")
        || button.starts_with("pack_op:open:")
}

pub async fn handle_clan_button(
    state: &Arc<GameState>,
    tx: &dyn PacketSink,
    pid: PlayerId,
    button: &str,
) -> bool {
    match button {
        _ if button.starts_with("pack_op:open:") => {
            if let Some(rest) = button.strip_prefix("pack_op:") {
                pack_gui::handle_pack_operation(state, tx, pid, rest).await;
            }
            true
        }
        "clan_menu" | "clan_back" => {
            crate::game::logic::clans::handle_clan_menu(state, tx, pid).await;
            true
        }
        "clan_requests" => {
            crate::game::logic::clans::handle_clan_requests_view(state, tx, pid).await;
            true
        }
        "clan_members" => {
            crate::game::logic::clans::handle_clan_members_view(state, tx, pid).await;
            true
        }
        "clan_invite_list" => {
            crate::game::logic::clans::handle_clan_invite_list(state, tx, pid).await;
            true
        }
        "clan_invites_view" => {
            crate::game::logic::clans::handle_clan_invites_view(state, tx, pid).await;
            true
        }
        _ => handle_clan_button_with_id(state, tx, pid, button).await,
    }
}

async fn handle_clan_button_with_id(
    state: &Arc<GameState>,
    tx: &dyn PacketSink,
    pid: PlayerId,
    button: &str,
) -> bool {
    let Some((prefix, raw_id)) = button.split_once(':') else {
        return false;
    };
    let Ok(id) = raw_id.parse::<i32>() else {
        return is_clan_button(button);
    };
    match prefix {
        "clan_view" => {
            crate::game::logic::clans::handle_clan_preview(state, tx, pid, id).await;
        }
        _ => return false,
    }
    true
}

pub fn is_auction_button(button: &str) -> bool {
    button == "auc"
        || button.starts_with("choose:")
        || button.starts_with("openorder:")
        || button.starts_with("auccreate:")
        || button.starts_with("aucsetcost:")
        || button.starts_with("aucsetnum:")
        || button.starts_with("aucminbet:")
        || button.starts_with("aucbet:")
}

pub async fn handle_auction_button(
    state: &Arc<GameState>,
    tx: &dyn PacketSink,
    pid: PlayerId,
    button: &str,
) -> bool {
    if button == "auc" {
        market_gui::handle_market_tab_switch(state, tx, pid, "auc").await;
        return true;
    }
    if let Some(rest) = button.strip_prefix("choose:") {
        // Клик item-грида аукциона (клиент хардкодит InvButton="choose").
        match rest.parse::<i32>() {
            Ok(item) => crate::game::logic::auction_gui::open_item_auc(state, tx, pid, item).await,
            Err(e) => {
                tracing::warn!(player_id = %pid, action = button, error = ?e, "Invalid auction choose action");
                market_gui::send_market_action_error(tx);
            }
        }
        return true;
    }
    if let Some(rest) = button.strip_prefix("openorder:") {
        match rest.parse::<i32>() {
            Ok(id) => crate::game::logic::auction_gui::open_order(state, tx, pid, id).await,
            Err(e) => {
                tracing::warn!(player_id = %pid, action = button, error = ?e, "Invalid auction openorder action");
                market_gui::send_market_action_error(tx);
            }
        }
        return true;
    }
    if let Some(rest) = button.strip_prefix("auccreate:") {
        match rest.parse::<i32>() {
            Ok(item) => crate::game::logic::auction_gui::open_order_creation(state, tx, pid, item),
            Err(e) => {
                tracing::warn!(player_id = %pid, action = button, error = ?e, "Invalid auction create action");
                market_gui::send_market_action_error(tx);
            }
        }
        return true;
    }
    if let Some(rest) = button.strip_prefix("aucsetcost:") {
        // aucsetcost:{item}:{cost}; невалидный cost → закрыть окно (1:1 C#).
        let parts: Vec<&str> = rest.splitn(2, ':').collect();
        if let [item, cost] = parts.as_slice() {
            match (item.parse::<i32>(), cost.parse::<i64>()) {
                (Ok(item), Ok(cost)) => {
                    crate::game::logic::auction_gui::open_order_creation_num(
                        state, tx, pid, item, cost,
                    );
                }
                _ => close_player_window(state, tx, pid),
            }
        } else {
            tracing::warn!(player_id = %pid, action = button, "Invalid auction setcost action");
            market_gui::send_market_action_error(tx);
        }
        return true;
    }
    if let Some(rest) = button.strip_prefix("aucsetnum:") {
        // aucsetnum:{item}:{cost}:{num}; невалидный num → закрыть окно (1:1 C#).
        let parts: Vec<&str> = rest.split(':').collect();
        if let [item, cost, num] = parts.as_slice() {
            match (item.parse::<i32>(), cost.parse::<i64>(), num.parse::<i32>()) {
                (Ok(item), Ok(cost), Ok(num)) => {
                    crate::game::logic::auction_gui::create_order(state, tx, pid, item, num, cost)
                        .await;
                }
                _ => close_player_window(state, tx, pid),
            }
        } else {
            tracing::warn!(player_id = %pid, action = button, "Invalid auction setnum action");
            market_gui::send_market_action_error(tx);
        }
        return true;
    }
    if let Some(rest) = button.strip_prefix("aucminbet:") {
        match rest.parse::<i32>() {
            Ok(id) => crate::game::logic::auction_gui::place_minimal_bet(state, tx, pid, id).await,
            Err(e) => {
                tracing::warn!(player_id = %pid, action = button, error = ?e, "Invalid auction minbet action");
                market_gui::send_market_action_error(tx);
            }
        }
        return true;
    }
    if let Some(rest) = button.strip_prefix("aucbet:") {
        // aucbet:{id}:{amount}; невалидная сумма → просто переоткрыть ордер (1:1 C#).
        let parts: Vec<&str> = rest.splitn(2, ':').collect();
        if let [id, amount] = parts.as_slice() {
            match (id.parse::<i32>(), amount.parse::<i64>()) {
                (Ok(id), Ok(amount)) => {
                    crate::game::logic::auction_gui::place_bet(state, tx, pid, id, amount).await;
                }
                (Ok(id), Err(_)) => {
                    crate::game::logic::auction_gui::open_order(state, tx, pid, id).await;
                }
                (Err(e), _) => {
                    tracing::warn!(player_id = %pid, action = button, error = ?e, "Invalid auction bet action");
                    market_gui::send_market_action_error(tx);
                }
            }
        } else {
            tracing::warn!(player_id = %pid, action = button, "Invalid auction bet action");
            market_gui::send_market_action_error(tx);
        }
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::buildings::{
        BuildingCrafting, BuildingFlags, BuildingOwnership, BuildingStats, BuildingStorage,
    };
    use crate::game::logic::gui::crafter_gui;
    use crate::game::logic::gui::market_gui;
    use crate::game::logic::gui::pack_gui;
    use crate::game::player::{PlayerFlags, PlayerInventory, PlayerStats, PlayerUI};
    use crate::test_support::{ServerTestHarness, ServerTestHarnessBuilder, drain_events};
    use std::sync::Arc;

    fn test_building_extra(
        charge: i32,
        max_charge: i32,
        hp: i32,
        max_hp: i32,
    ) -> crate::db::BuildingExtra {
        crate::db::BuildingExtra {
            charge,
            max_charge,
            cost: 0,
            hp,
            max_hp,
            money_inside: 0,
            crystals_inside: [0; 6],
            items_inside: std::collections::HashMap::new(),
            craft_recipe_id: None,
            craft_num: 0,
            craft_end_ts: 0,
            craft_ready: false,
            clanzone: 0,
        }
    }

    #[test]
    fn parse_rich_key_values_rejects_malformed_pairs() {
        let parsed = parse_rich_key_values("cost:10#clan:1").unwrap();
        assert_eq!(parsed.get("cost"), Some(&"10"));
        assert_eq!(parsed.get("clan"), Some(&"1"));
        assert_eq!(parse_rich_key_values("cost:10#clan:1#").unwrap(), parsed);
        assert!(parse_rich_key_values("cost").is_none());
        assert!(parse_rich_key_values("cost:").is_none());
        assert!(parse_rich_key_values(":10").is_none());
    }

    #[test]
    fn clans_pack_sync_open_does_not_require_tokio_reactor() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let test = runtime.block_on(make_craft_test_state("clans_sync_open", 10, 10));
        drop(runtime);

        let (tx, mut rx) = crate::net::session::outbox::channel();
        let view = crate::game::buildings::PackView {
            id: 1,
            pack_type: PackType::Clans,
            x: 10,
            y: 10,
            owner_id: PlayerId(test.player.id),
            clan_id: 0,
            charge: 0,
            max_charge: 0,
            hp: 1000,
            max_hp: 1000,
        };

        pack_gui::open_pack_gui(&test.state, &tx, PlayerId(test.player.id), &view);

        assert!(
            rx.try_recv().is_err(),
            "sync Clans open must not spawn async GUI work or send fallback packets"
        );
    }

    #[tokio::test]
    async fn clans_pack_operation_open_sends_clan_menu() {
        let test = make_craft_test_state("clans_pack_open", 20, 10).await;
        let pid = PlayerId(test.player.id);
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let extra = test_building_extra(0, 0, 1000, 1000);
        let spec = crate::game::BuildingInsertSpec {
            type_code: "D",
            pack_type: PackType::Clans,
            x: 20,
            y: 10,
            owner_id: pid,
            clan_id: 0,
            extra: &extra,
        };
        test.state.insert_building_runtime(&spec).await.unwrap();

        pack_gui::handle_pack_operation(&test.state, &tx, pid, "open:20:10").await;

        let events = drain_events(&mut rx);
        let Some((_, payload)) = events.iter().find(|(event, _)| event == "GU") else {
            panic!("Clans pack open must send GU clan menu, got {events:?}");
        };
        let json = std::str::from_utf8(payload).unwrap();
        assert!(json.contains("КЛАНЫ"), "unexpected clan menu: {json}");
        assert!(json.contains("clan_create"), "unexpected clan menu: {json}");
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
        let (tx, mut rx) = test.connect_with_outbox(1);
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
    }

    #[tokio::test]
    async fn programmator_create_button_clears_server_window_state() {
        let test = make_craft_test_state("prog_create_window_state", 10, 10).await;
        let (tx, mut rx) = test.connect_with_outbox(1);
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
    }

    #[tokio::test]
    async fn programmator_rename_confirms_with_update_packet_and_closes_horb() {
        let test = make_craft_test_state("prog_rename_window_state", 10, 10).await;
        let (tx, mut rx) = test.connect_with_outbox(1);
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
    }

    #[tokio::test]
    async fn settings_save_missing_player_flags_is_explicit_error_without_settings_mutation() {
        let test = make_craft_test_state("settings_missing_player_flags", 10, 10).await;
        let (tx, mut rx) = test.connect_with_outbox(1);
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

        crate::net::session::ui::settings::apply(&test.state, &tx, pid, "isca:5#mous:1#");

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
    }

    #[tokio::test]
    async fn craft_start_rejects_remote_player_without_deducting_resources() {
        let test = make_craft_test_state("remote", 5, 5).await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        crafter_gui::handle_craft_start(&test.state, &tx, test.player.id.into(), "0:1:10:10");

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert_eq!(events[0].1, "Недостаточно ресов#...".as_bytes());

        let state_after = craft_state(&test.state, 10, 10);
        assert_eq!(state_after, (None, 0, 0));

        let crystals = player_crystals(&test.state, test.player.id.into());
        assert_eq!(crystals[0], 100);
    }

    #[tokio::test]
    async fn craft_start_rejects_missing_crafting_component_without_deducting_resources() {
        let test = make_craft_test_state("missing_component", 10, 10).await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let entity = test.state.building_entity_at(10, 10).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity).remove::<BuildingCrafting>();
        }

        crafter_gui::handle_craft_start(&test.state, &tx, test.player.id.into(), "0:1:10:10");

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert_eq!(events[0].1, "КРАФТЕР#Некорректное действие.".as_bytes());

        let crystals = player_crystals(&test.state, test.player.id.into());
        assert_eq!(crystals[0], 100);
    }

    #[tokio::test]
    async fn craft_start_missing_player_flags_is_explicit_error_without_deducting_resources() {
        let test = make_craft_test_state("missing_player_flags", 10, 10).await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let entity = test.state.get_player_entity(test.player.id.into()).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity).remove::<PlayerFlags>();
        }

        crafter_gui::handle_craft_start(&test.state, &tx, test.player.id.into(), "0:1:10:10");

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert_eq!(
            events[0].1,
            "КРАФТЕР#Состояние крафтера недоступно.".as_bytes()
        );
        assert_eq!(player_crystals(&test.state, test.player.id.into())[0], 100);
        assert_eq!(craft_state(&test.state, 10, 10), (None, 0, 0));
    }

    #[tokio::test]
    async fn craft_start_on_crafter_origin_deducts_and_starts_recipe() {
        let test = make_craft_test_state("local", 10, 10).await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        crafter_gui::handle_craft_start(&test.state, &tx, test.player.id.into(), "0:1:10:10");

        let (recipe_id, num, end_ts) = craft_state(&test.state, 10, 10);
        assert_eq!(recipe_id, Some(0));
        assert_eq!(num, 1);
        assert!(end_ts > 0);

        let crystals = player_crystals(&test.state, test.player.id.into());
        assert_eq!(crystals[0], 50);
    }

    #[tokio::test]
    async fn craft_start_gui_command_returns_session_effect_and_building_save() {
        let test = make_craft_test_state("typed_start", 10, 10).await;
        let session_id = crate::game::SessionId::new(41);
        let mut receiver = test.connect(session_id.get());
        drain_events(&mut receiver);

        let effects = crate::game::logic::commands::apply_player_command(
            &test.state,
            test.player.id.into(),
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::parse("craft_start:0:1:10:10".to_owned()),
            },
        );

        assert!(receiver.try_recv().is_err());
        assert!(matches!(
            effects.events.as_slice(),
            [crate::game::GameEvent::SessionBatch { session_id: event_session, .. }]
                if *event_session == session_id
        ));
        assert!(matches!(
            effects.saves.as_slice(),
            [crate::game::SaveCommand::Building { row }]
                if row.craft_recipe_id == Some(0) && row.craft_num == 1
        ));
    }

    #[tokio::test]
    async fn craft_claim_gui_command_returns_typed_reward_and_cleared_building_save() {
        let test = make_craft_test_state("typed_claim", 10, 10).await;
        let session_id = crate::game::SessionId::new(42);
        let mut receiver = test.connect(session_id.get());
        drain_events(&mut receiver);

        let start = crate::game::logic::commands::apply_player_command(
            &test.state,
            test.player.id.into(),
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::parse("craft_start:0:1:10:10".to_owned()),
            },
        );
        assert!(matches!(
            start.saves.as_slice(),
            [crate::game::SaveCommand::Building { .. }]
        ));
        let entity = test.state.building_entity_at(10, 10).unwrap();
        let mut ecs = test.state.ecs.write();
        ecs.get_mut::<BuildingCrafting>(entity).unwrap().end_ts = 0;
        drop(ecs);
        drain_events(&mut receiver);

        let effects = crate::game::logic::commands::apply_player_command(
            &test.state,
            test.player.id.into(),
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::parse("craft_claim:10:10".to_owned()),
            },
        );

        assert!(receiver.try_recv().is_err());
        assert!(matches!(
            effects.saves.as_slice(),
            [crate::game::SaveCommand::Building { row }]
                if row.craft_recipe_id.is_none() && row.craft_num == 0
        ));
        assert!(matches!(
            effects.events.as_slice(),
            [crate::game::GameEvent::SessionBatch { packets, .. }]
                if packets.iter().any(|packet| {
                    openmines_protocol::Packet::try_decode(
                        &mut bytes::BytesMut::from(packet.as_slice()),
                    )
                    .is_ok_and(|decoded| decoded.is_some_and(|packet| packet.event_name == *b"IN"))
                })
        ));
    }

    #[tokio::test]
    async fn craft_claim_clears_crafter_before_second_claim_can_duplicate_reward() {
        let test = make_craft_test_state("claim_once", 10, 10).await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        crafter_gui::handle_craft_start(&test.state, &tx, test.player.id.into(), "0:1:10:10");
        {
            let entity = test.state.building_entity_at(10, 10).unwrap();
            let mut ecs = test.state.ecs.write();
            let mut craft = ecs.get_mut::<BuildingCrafting>(entity).unwrap();
            craft.end_ts = 0;
        }
        drain_events(&mut rx);

        crafter_gui::handle_craft_claim(&test.state, &tx, test.player.id.into(), "10:10");
        crafter_gui::handle_craft_claim(&test.state, &tx, test.player.id.into(), "10:10");

        assert_eq!(
            player_inventory_count(&test.state, test.player.id.into(), 0),
            1
        );
        assert_eq!(craft_state(&test.state, 10, 10), (None, 0, 0));
    }

    #[tokio::test]
    async fn craft_claim_missing_building_flags_is_explicit_error_without_reward_or_clear() {
        let test = make_craft_test_state("claim_missing_flags", 10, 10).await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        crafter_gui::handle_craft_start(&test.state, &tx, test.player.id.into(), "0:1:10:10");
        {
            let entity = test.state.building_entity_at(10, 10).unwrap();
            let mut ecs = test.state.ecs.write();
            let mut craft = ecs.get_mut::<BuildingCrafting>(entity).unwrap();
            craft.end_ts = 0;
            ecs.entity_mut(entity).remove::<BuildingFlags>();
        }
        drain_events(&mut rx);

        crafter_gui::handle_craft_claim(&test.state, &tx, test.player.id.into(), "10:10");

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
    }

    #[tokio::test]
    async fn market_sell_missing_building_flags_is_explicit_error_without_money_or_crystal_mutation()
     {
        let test = make_market_test_state("sell_missing_building_flags").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let building_entity = test.state.building_entity_at(10, 10).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(building_entity).remove::<BuildingFlags>();
        }
        let before_money = player_money(&test.state, test.player.id.into());
        let before_crystals = player_crystals(&test.state, test.player.id.into());

        market_gui::do_market_sell(
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
    }

    #[tokio::test]
    async fn market_buy_missing_player_flags_is_explicit_error_without_money_or_crystal_mutation() {
        let test = make_market_test_state("buy_missing_player_flags").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
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

        market_gui::handle_market_buy(&test.state, &tx, test.player.id.into(), "1:0:0:0:0:0");

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
    }

    #[tokio::test]
    async fn market_getprofit_missing_building_flags_is_explicit_error_without_profit_mutation() {
        let test = make_market_test_state("getprofit_missing_building_flags").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let player_entity = test.state.get_player_entity(test.player.id.into()).unwrap();
        let building_entity = test.state.building_entity_at(10, 10).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut ui = ecs.get_mut::<PlayerUI>(player_entity).unwrap();
            ui.current_window = Some("market:10:10:admin".to_string());
            let mut storage = ecs.get_mut::<BuildingStorage>(building_entity).unwrap();
            storage.money = 777;
            ecs.entity_mut(building_entity).remove::<BuildingFlags>();
        }
        let before_money = player_money(&test.state, test.player.id.into());

        market_gui::handle_market_getprofit(&test.state, &tx, test.player.id.into());

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
    }

    #[tokio::test]
    async fn pack_take_money_missing_player_flags_is_explicit_error_without_storage_mutation() {
        let test = make_market_test_state("pack_take_money_missing_player_flags").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let player_entity = test.state.get_player_entity(test.player.id.into()).unwrap();
        let building_entity = test.state.building_entity_at(10, 10).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut storage = ecs.get_mut::<BuildingStorage>(building_entity).unwrap();
            storage.money = 777;
            ecs.entity_mut(player_entity).remove::<PlayerFlags>();
        }
        let view = test.state.get_pack_at(10, 10).unwrap();
        let before_money = player_money(&test.state, test.player.id.into());

        pack_gui::handle_pack_take_money(&test.state, &tx, test.player.id.into(), &view);

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
    }

    #[tokio::test]
    async fn pack_take_crystals_missing_player_flags_is_explicit_error_without_storage_mutation() {
        let test = make_market_test_state("pack_take_crystals_missing_player_flags").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let player_entity = test.state.get_player_entity(test.player.id.into()).unwrap();
        let building_entity = test.state.building_entity_at(10, 10).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut storage = ecs.get_mut::<BuildingStorage>(building_entity).unwrap();
            storage.crystals = [7, 6, 5, 4, 3, 2];
            ecs.entity_mut(player_entity).remove::<PlayerFlags>();
        }
        let view = test.state.get_pack_at(10, 10).unwrap();
        let before_crystals = player_crystals(&test.state, test.player.id.into());

        pack_gui::handle_pack_take_crystals(&test.state, &tx, test.player.id.into(), &view);

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
    }

    #[tokio::test]
    async fn pack_withdrawal_gui_command_returns_typed_effect_and_building_save() {
        let test = make_market_test_state("typed_pack_withdrawal").await;
        let session_id = crate::game::SessionId::new(2);
        let (_tx, mut rx) = test.connect_with_outbox(session_id.get());
        drain_events(&mut rx);
        let building_entity = test.state.building_entity_at(10, 10).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.get_mut::<BuildingStorage>(building_entity)
                .unwrap()
                .money = 777;
        }
        let before_money = player_money(&test.state, test.player.id.into());

        let effects = crate::game::logic::commands::apply_player_command(
            &test.state,
            test.player.id.into(),
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::parse("pack_op:take_money:10:10".to_owned()),
            },
        );

        assert!(rx.try_recv().is_err());
        assert!(matches!(
            effects.saves.as_slice(),
            [crate::game::SaveCommand::Building { row }]
                if row.money_inside == 0
        ));
        assert!(matches!(
            effects.events.as_slice(),
            [crate::game::GameEvent::SessionBatch { session_id: event_session, packets, .. }]
                if *event_session == session_id
                    && packets.iter().any(|packet| {
                        openmines_protocol::Packet::try_decode(
                            &mut bytes::BytesMut::from(packet.as_slice()),
                        )
                        .is_ok_and(|decoded| decoded.is_some_and(|packet| packet.event_name == *b"P$"))
                    })
        ));
        assert_eq!(
            player_money(&test.state, test.player.id.into()),
            before_money + 777
        );
        assert_eq!(market_storage_money(&test.state, 10, 10), 0);
    }

    #[tokio::test]
    async fn pack_save_gui_command_returns_typed_effect_and_building_save() {
        let test = make_market_test_state("typed_pack_save").await;
        let session_id = crate::game::SessionId::new(3);
        let (_tx, mut rx) = test.connect_with_outbox(session_id.get());
        drain_events(&mut rx);
        let player_id = test.player.id.into();
        {
            let entity = test.state.get_player_entity(player_id).unwrap();
            let mut ecs = test.state.ecs.write();
            ecs.get_mut::<PlayerUI>(entity).unwrap().current_window = Some("pack:10:10".into());
        }

        let effects = crate::game::logic::commands::apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::parse("pack_save:cost:1234#clan:0#".to_owned()),
            },
        );

        assert!(rx.try_recv().is_err());
        assert!(matches!(
            effects.saves.as_slice(),
            [crate::game::SaveCommand::Building { row }]
                if row.cost == 1234 && row.clan_id == 0
        ));
        assert!(matches!(
            effects.events.as_slice(),
            [crate::game::GameEvent::SessionBatch { session_id: event_session, packets, .. }]
                if *event_session == session_id
                    && packets.iter().any(|packet| {
                        openmines_protocol::Packet::try_decode(
                            &mut bytes::BytesMut::from(packet.as_slice()),
                        )
                        .is_ok_and(|decoded| decoded.is_some_and(|packet| packet.event_name == *b"GU"))
                    })
        ));
        assert_eq!(
            test.state
                .query_building_opt(10, 10, |ecs, entity| {
                    Some(ecs.get::<BuildingStats>(entity)?.cost)
                })
                .unwrap(),
            1234
        );
    }

    #[tokio::test]
    async fn storage_transfer_missing_player_flags_is_explicit_error_without_crystal_mutation() {
        let test = make_storage_test_state("storage_transfer_missing_player_flags").await;
        let session_id = crate::game::SessionId::new(1);
        let (_tx, mut rx) = test.connect_with_outbox(session_id.get());
        drain_events(&mut rx);

        let player_entity = test.state.get_player_entity(test.player.id.into()).unwrap();
        let building_entity = test.state.building_entity_at(10, 10).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut ui = ecs.get_mut::<PlayerUI>(player_entity).unwrap();
            ui.current_window = Some("pack:10:10".to_string());
            let mut storage = ecs.get_mut::<BuildingStorage>(building_entity).unwrap();
            storage.crystals = [10, 0, 0, 0, 0, 0];
            ecs.entity_mut(player_entity).remove::<PlayerFlags>();
        }
        let before_player = player_crystals(&test.state, test.player.id.into());

        let effects = crate::game::logic::commands::apply_player_command(
            &test.state,
            test.player.id.into(),
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::parse("transfer:50:0:0:0:0:0".to_owned()),
            },
        );

        assert!(
            drain_events(&mut rx).is_empty(),
            "dispatch must not write wire"
        );
        let [crate::game::GameEvent::SessionBatch { packets, .. }] = effects.events.as_slice()
        else {
            panic!("missing storage state must return one error packet effect");
        };
        assert_eq!(packets.len(), 1);
        let mut packet_bytes = bytes::BytesMut::from(packets[0].as_slice());
        let packet = crate::protocol::Packet::try_decode(&mut packet_bytes)
            .unwrap()
            .unwrap();
        assert_eq!(packet.event_str(), "OK");
        assert_eq!(
            packet.payload,
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
    }

    #[tokio::test]
    async fn storage_transfer_returns_basket_then_refreshed_gui_effects() {
        let test = make_storage_test_state("storage_transfer_effects").await;
        let player_id = PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(1);
        let (_tx, mut rx) = test.connect_with_outbox(session_id.get());
        drain_events(&mut rx);
        let building_entity = test.state.building_entity_at(10, 10).unwrap();
        test.state
            .ecs
            .write()
            .get_mut::<BuildingStorage>(building_entity)
            .unwrap()
            .crystals = [25, 0, 0, 0, 0, 0];
        set_current_window(&test.state, player_id, Some("blds"));

        let open_effects = crate::game::logic::commands::apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::OpenPack { x: 10, y: 10 },
            },
        );
        assert!(matches!(
            open_effects.events.as_slice(),
            [crate::game::GameEvent::GuiView {
                view: crate::game::GuiView::Storage(_),
                ..
            }]
        ));
        assert_eq!(
            current_window(&test.state, player_id).as_deref(),
            Some("pack:10:10")
        );

        let effects = crate::game::logic::commands::apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::parse("transfer:50:0:0:0:0:0".to_owned()),
            },
        );

        assert!(
            drain_events(&mut rx).is_empty(),
            "dispatch must not write wire"
        );
        let [
            crate::game::GameEvent::SessionBatch { packets, .. },
            crate::game::GameEvent::GuiView {
                view: crate::game::GuiView::Storage(view),
                ..
            },
        ] = effects.events.as_slice()
        else {
            panic!("storage transfer must return basket before refreshed GUI");
        };
        let mut packet_bytes = bytes::BytesMut::from(packets[0].as_slice());
        let packet = crate::protocol::Packet::try_decode(&mut packet_bytes)
            .unwrap()
            .unwrap();
        assert_eq!(packet.event_str(), "@B");
        assert_eq!(packet.payload, b"75:0:0:0:0:0:1".as_slice());
        assert!(view.crystal_lines.iter().any(|line| line == "0:0:125:50:"));
        assert_eq!(player_crystals(&test.state, player_id), [75, 0, 0, 0, 0, 0]);
        assert_eq!(
            market_storage_crystals(&test.state, 10, 10),
            [50, 0, 0, 0, 0, 0]
        );
    }

    #[tokio::test]
    async fn teleport_gui_uses_list_rows_for_many_destinations_not_horb_buttons() {
        let test = make_teleport_test_state("tp_many_destinations", 8).await;
        let player_id = PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(1);
        let (_tx, mut rx) = test.connect_with_outbox(session_id.get());
        drain_events(&mut rx);
        set_current_window(&test.state, player_id, Some("blds"));

        let effects = crate::game::logic::commands::apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::OpenPack { x: 10, y: 10 },
            },
        );
        let [
            crate::game::GameEvent::GuiView {
                view: crate::game::GuiView::Teleport(view),
                ..
            },
        ] = effects.events.as_slice()
        else {
            panic!("teleport open must return one immutable GUI view");
        };
        crate::net::presentation::deliver_gui_view_for_test(
            &test.state,
            session_id,
            player_id,
            crate::game::GuiView::Teleport(view.clone()),
        );

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "GU");
        let payload = std::str::from_utf8(&events[0].1).unwrap();
        let json = payload.strip_prefix("horb:").unwrap_or(payload);
        let cfg: serde_json::Value = serde_json::from_str(json).unwrap();

        let buttons = cfg["buttons"].as_array().unwrap();
        assert_eq!(
            buttons.len() / 2,
            4,
            "teleport destinations must not be encoded as bottom HORB buttons"
        );
        let list = cfg["list"].as_array().unwrap();
        assert_eq!(list.len() / 3, 8);
        assert!(
            list.iter()
                .filter_map(serde_json::Value::as_str)
                .any(|value| value.starts_with("tp:")),
            "destination actions must stay clickable through list rows"
        );
    }

    #[tokio::test]
    async fn teleport_gui_command_returns_immutable_view_before_delivery() {
        let test = make_teleport_test_state("tp_immutable_view", 8).await;
        let player_id = PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(1);
        let (_tx, mut rx) = test.connect_with_outbox(session_id.get());
        drain_events(&mut rx);
        set_current_window(&test.state, player_id, Some("blds"));

        let effects = crate::game::logic::commands::apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::OpenPack { x: 10, y: 10 },
            },
        );

        assert!(
            drain_events(&mut rx).is_empty(),
            "dispatch must not send GUI wire"
        );
        assert_eq!(
            current_window(&test.state, player_id).as_deref(),
            Some("pack:10:10")
        );
        let [
            crate::game::GameEvent::GuiView {
                session_id: event_session,
                player_id: event_player,
                view: crate::game::GuiView::Teleport(view),
            },
        ] = effects.events.as_slice()
        else {
            panic!("teleport open must return one immutable GUI view");
        };
        assert_eq!(*event_session, session_id);
        assert_eq!(*event_player, player_id);
        assert_eq!(view.destinations.len(), 8);
        assert_eq!(view.map_tiles.len(), 17 * 17);
        assert!(
            view.destinations.iter().all(|position| position.0 != 1_200),
            "far teleport must not enter the bounded spatial snapshot"
        );
        assert!(
            !test
                .state
                .building_entities_in_chunk_snapshot(37, 0)
                .is_empty(),
            "fixture must contain an unrelated teleport outside the query radius"
        );

        crate::net::presentation::deliver_gui_view_for_test(
            &test.state,
            session_id,
            player_id,
            crate::game::GuiView::Teleport(view.clone()),
        );
        let delivered = drain_events(&mut rx);
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].0, "GU");
        let payload = std::str::from_utf8(&delivered[0].1).unwrap();
        assert!(
            payload.contains("=R#"),
            "Unity canvas rect delimiter must be #"
        );
    }

    #[tokio::test]
    async fn gui_open_pack_without_window_returns_close_effect() {
        let test = make_teleport_test_state("tp_without_window", 1).await;
        let player_id = PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(1);
        let (_tx, mut rx) = test.connect_with_outbox(session_id.get());
        drain_events(&mut rx);
        assert_eq!(current_window(&test.state, player_id), None);

        let effects = crate::game::logic::commands::apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::OpenPack { x: 10, y: 10 },
            },
        );

        assert!(drain_events(&mut rx).is_empty());
        assert!(matches!(
            effects.events.as_slice(),
            [crate::game::GameEvent::GuiView {
                session_id: event_session,
                player_id: event_player,
                view: crate::game::GuiView::Close,
            }] if *event_session == session_id && *event_player == player_id
        ));
    }

    #[tokio::test]
    async fn gui_exit_resets_window_state_and_returns_close_effect() {
        let test = make_teleport_test_state("gui_exit_effect", 1).await;
        let player_id = PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(1);
        let (_tx, mut rx) = test.connect_with_outbox(session_id.get());
        drain_events(&mut rx);
        set_current_window(&test.state, player_id, Some("pack:10:10"));
        let _ = test.state.modify_player(player_id, |ecs, entity| {
            ecs.get_mut::<PlayerInventory>(entity)?.selected = 3;
            Some(())
        });

        let effects = crate::game::logic::commands::apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::parse("exit".to_owned()),
            },
        );

        assert!(
            drain_events(&mut rx).is_empty(),
            "dispatch must not write GUI wire"
        );
        assert_eq!(current_window(&test.state, player_id), None);
        assert_eq!(
            test.state.query_player_opt(player_id, |ecs, entity| {
                Some(ecs.get::<PlayerInventory>(entity)?.selected)
            }),
            Some(-1)
        );
        assert!(matches!(
            effects.events.as_slice(),
            [crate::game::GameEvent::GuiView {
                session_id: event_session,
                player_id: event_player,
                view: crate::game::GuiView::Close,
            }] if *event_session == session_id && *event_player == player_id
        ));
    }

    #[tokio::test]
    async fn stale_teleport_gui_session_cannot_mutate_current_window() {
        let test = make_teleport_test_state("tp_stale_session", 1).await;
        let player_id = PlayerId(test.player.id);
        let old_session = crate::game::SessionId::new(1);
        let (_old_tx, mut old_rx) = test.connect_with_outbox(old_session.get());
        drain_events(&mut old_rx);
        set_current_window(&test.state, player_id, Some("blds"));

        let new_session = crate::game::SessionId::new(2);
        let (_new_tx, mut new_rx) = test.connect_with_outbox(new_session.get());
        drain_events(&mut new_rx);
        set_current_window(&test.state, player_id, Some("blds"));

        let effects = crate::game::logic::commands::apply_player_command(
            &test.state,
            player_id,
            old_session,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::OpenPack { x: 10, y: 10 },
            },
        );

        assert!(effects.events.is_empty());
        assert!(effects.saves.is_empty());
        assert_eq!(
            current_window(&test.state, player_id).as_deref(),
            Some("blds")
        );
        assert!(drain_events(&mut old_rx).is_empty());
        assert!(drain_events(&mut new_rx).is_empty());
    }

    #[tokio::test]
    async fn public_teleport_opens_for_non_owner_like_reference() {
        let test = make_teleport_test_state("tp_public_non_owner", 1).await;
        let player_id = PlayerId(test.player.id);
        let session_id = crate::game::SessionId::new(1);
        let (_tx, mut rx) = test.connect_with_outbox(session_id.get());
        drain_events(&mut rx);
        set_current_window(&test.state, player_id, Some("blds"));
        let source = test.state.building_entity_at(10, 10).unwrap();
        test.state
            .ecs
            .write()
            .get_mut::<BuildingOwnership>(source)
            .unwrap()
            .owner_id = PlayerId(999);

        let effects = crate::game::logic::commands::apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::OpenPack { x: 10, y: 10 },
            },
        );

        assert!(matches!(
            effects.events.as_slice(),
            [crate::game::GameEvent::GuiView {
                view: crate::game::GuiView::Teleport(_),
                ..
            }]
        ));
    }

    async fn make_craft_test_state(label: &str, player_x: i32, player_y: i32) -> ServerTestHarness {
        let mut builder =
            ServerTestHarnessBuilder::new(&format!("craft_start_{label}"), "craft-user").await;
        builder.player.x = player_x;
        builder.player.y = player_y;
        builder.player.crystals[0] = 100;

        let extra = test_building_extra(0, 0, 1000, 1000);
        let player_id = builder.player.id;
        builder
            .database()
            .insert_building("F", 10, 10, player_id, 0, &extra)
            .await
            .unwrap();
        builder.build().await
    }

    async fn make_teleport_test_state(label: &str, destination_count: i32) -> ServerTestHarness {
        let mut builder =
            ServerTestHarnessBuilder::new(&format!("teleport_{label}"), "tp-user").await;
        builder.world_chunks(40, 2);
        builder.player.x = 10;
        builder.player.y = 10;
        builder.player.money = 10_000;

        let extra = test_building_extra(100, 1000, 1000, 1000);
        let player_id = builder.player.id;
        builder
            .database()
            .insert_building("T", 10, 10, player_id, 0, &extra)
            .await
            .unwrap();
        for i in 0..destination_count {
            builder
                .database()
                .insert_building("T", 42 + i * 32, 10, player_id, 0, &extra)
                .await
                .unwrap();
        }
        builder
            .database()
            .insert_building("T", 1_200, 10, player_id, 0, &extra)
            .await
            .unwrap();
        builder.build().await
    }

    async fn make_market_test_state(label: &str) -> ServerTestHarness {
        let mut builder =
            ServerTestHarnessBuilder::new(&format!("market_{label}"), "market-user").await;
        builder.player.x = 10;
        builder.player.y = 10;
        builder.player.money = 10_000;
        builder.player.crystals[0] = 100;

        let extra = test_building_extra(0, 0, 1000, 1000);
        let player_id = builder.player.id;
        builder
            .database()
            .insert_building("M", 10, 10, player_id, 0, &extra)
            .await
            .unwrap();
        builder.build().await
    }

    async fn make_storage_test_state(label: &str) -> ServerTestHarness {
        let mut builder =
            ServerTestHarnessBuilder::new(&format!("storage_{label}"), "storage-user").await;
        builder.player.x = 10;
        builder.player.y = 10;
        builder.player.money = 10_000;
        builder.player.crystals[0] = 100;

        let extra = test_building_extra(0, 0, 1000, 1000);
        let player_id = builder.player.id;
        builder
            .database()
            .insert_building("L", 10, 10, player_id, 0, &extra)
            .await
            .unwrap();
        builder.build().await
    }

    fn craft_state(state: &Arc<GameState>, bx: i32, by: i32) -> (Option<i32>, i32, i64) {
        state
            .query_building_opt(bx, by, |ecs, entity| {
                let craft = ecs.get::<BuildingCrafting>(entity)?;
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
            .query_building_opt(bx, by, |ecs, entity| {
                Some(ecs.get::<BuildingStorage>(entity)?.money)
            })
            .unwrap()
    }

    fn market_storage_crystals(state: &Arc<GameState>, bx: i32, by: i32) -> [i64; 6] {
        state
            .query_building_opt(bx, by, |ecs, entity| {
                Some(ecs.get::<BuildingStorage>(entity)?.crystals)
            })
            .unwrap()
    }

    fn current_window(state: &Arc<GameState>, pid: PlayerId) -> Option<String> {
        state.query_player_opt(pid, |ecs, entity| {
            Some(ecs.get::<PlayerUI>(entity)?.current_window.clone())
        })?
    }

    fn set_current_window(state: &Arc<GameState>, pid: PlayerId, window: Option<&str>) {
        let _ = state.modify_player(pid, |ecs, entity| {
            ecs.get_mut::<PlayerUI>(entity)?.current_window = window.map(str::to_owned);
            Some(())
        });
    }
}
