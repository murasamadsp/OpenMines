use super::{
    Arc, CommandEffects, GameState, PlayerCommand, parse_pack_remove_button,
    parse_program_rename_button, spawn_paid_building_insert_task, spawn_session_async_task,
};
use crate::game::logic::horb::HorbDelivery;

pub(super) fn apply_presentation_command(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    command: &PlayerCommand,
) {
    match command {
        crate::game::PlayerCommand::AdminAction => {
            if let Some(tx) = state.player_sender(player_id) {
                crate::game::logic::commands_social::handle_admin_action(state, &tx, player_id);
            }
        }
        _ => unreachable!("non-presentation command routed to presentation command handler"),
    }
}

pub(super) fn apply_gui_command(
    state: &Arc<GameState>,
    session_id: crate::game::SessionId,
    player_id: crate::game::PlayerId,
    command: crate::game::GuiCommand,
) -> CommandEffects {
    if state
        .active_player_entity_for_session(player_id, session_id)
        .is_none()
    {
        return CommandEffects::default();
    }
    let Some(tx) = state.sessions.outbox_for_session(session_id) else {
        return CommandEffects::default();
    };
    if !state.check_gui_rate(player_id) {
        tracing::debug!(player_id = %player_id, "gui rate limited (GUI_)");
        return CommandEffects::default();
    }

    let button = match command {
        crate::game::GuiCommand::Button {
            kind: crate::game::logic::contracts::GuiButtonKind::Close,
            ..
        } => {
            if state
                .modify_player(player_id, |ecs, entity| {
                    if let Some(mut ui) = ecs.get_mut::<crate::game::player::PlayerUI>(entity) {
                        ui.current_window = None;
                    }
                    if let Some(mut inventory) =
                        ecs.get_mut::<crate::game::player::PlayerInventory>(entity)
                    {
                        inventory.selected = -1;
                    }
                    Some(())
                })
                .flatten()
                .is_none()
            {
                return CommandEffects::default();
            }
            return gui_view_effects(session_id, player_id, crate::game::GuiView::Close);
        }
        crate::game::GuiCommand::OpenPack { x, y } => {
            return apply_open_pack_gui(state, &tx, session_id, player_id, x, y);
        }
        crate::game::GuiCommand::Button { raw, .. } => raw,
    };

    apply_gui_button_command(state, &tx, session_id, player_id, button)
}

fn apply_open_pack_gui(
    state: &Arc<GameState>,
    tx: &crate::net::session::outbox::Outbox,
    session_id: crate::game::SessionId,
    player_id: crate::game::PlayerId,
    x: i32,
    y: i32,
) -> CommandEffects {
    let has_window = state
        .query_player_opt(player_id, |ecs, entity| {
            ecs.get::<crate::game::player::PlayerUI>(entity)
                .map(|ui| ui.current_window.is_some())
        })
        .unwrap_or(false);
    if !has_window {
        return gui_view_effects(session_id, player_id, crate::game::GuiView::Close);
    }
    match state.get_pack_at(x, y).map(|view| view.pack_type) {
        Some(crate::game::PackType::Teleport) => {
            let Some(view) =
                crate::game::logic::gui_views::teleport::prepare_view(state, player_id, x, y)
            else {
                return CommandEffects::default();
            };
            if !crate::game::logic::gui_views::teleport::activate_window(state, player_id, x, y) {
                return CommandEffects::default();
            }
            gui_view_effects(session_id, player_id, crate::game::GuiView::Teleport(view))
        }
        Some(crate::game::PackType::Spot) => {
            let Some(view) =
                crate::game::logic::gui_views::spot::prepare_view(state, player_id, x, y)
            else {
                return gui_view_effects(session_id, player_id, crate::game::GuiView::Close);
            };
            if !crate::game::logic::gui_views::spot::activate_window(state, player_id, x, y) {
                return CommandEffects::default();
            }
            gui_view_effects(session_id, player_id, crate::game::GuiView::Spot(view))
        }
        Some(crate::game::PackType::Storage) => {
            let Some(view) =
                crate::game::logic::gui_views::storage::prepare_view(state, player_id, x, y)
            else {
                return gui_view_effects(session_id, player_id, crate::game::GuiView::Close);
            };
            if !crate::game::logic::gui_views::storage::activate_window(state, player_id, x, y) {
                return CommandEffects::default();
            }
            gui_view_effects(session_id, player_id, crate::game::GuiView::Storage(view))
        }
        Some(crate::game::PackType::Clans) => {
            clan_pack_menu_effects(state, player_id, session_id, x, y)
        }
        _ => apply_gui_button_command(
            state,
            tx,
            session_id,
            player_id,
            format!("pack_op:open:{x}:{y}"),
        ),
    }
}

fn gui_view_effects(
    session_id: crate::game::SessionId,
    player_id: crate::game::PlayerId,
    view: crate::game::GuiView,
) -> CommandEffects {
    CommandEffects {
        events: vec![crate::game::GameEvent::GuiView {
            session_id,
            player_id,
            view,
        }],
        saves: Vec::new(),
        broadcasts: Vec::new(),
    }
}

fn apply_gui_button_command(
    state: &Arc<GameState>,
    tx: &crate::net::session::outbox::Outbox,
    session_id: crate::game::SessionId,
    player_id: crate::game::PlayerId,
    button: String,
) -> CommandEffects {
    if let Some(action) = clan_mutation_action_for_button(&button) {
        return clan_command_effects(player_id, session_id, action);
    }
    if let Some(action) = clan_menu_action_for_button(&button) {
        return clan_menu_effects(state, player_id, session_id, action);
    }
    // All implemented clan GUI buttons are handled above by typed commands.
    // Keep unknown clan actions out of the legacy async dispatcher: that path
    // still performs direct ECS/session work and must not become a migration
    // escape hatch for a new wire button.
    if is_migrated_clan_button(&button) {
        return CommandEffects::default();
    }
    if button == "prog" {
        return CommandEffects {
            events: Vec::new(),
            saves: vec![crate::game::SaveCommand::ProgramMenu {
                request: crate::game::ProgramMenuRequest {
                    player_id,
                    session_id,
                },
            }],
            broadcasts: Vec::new(),
        };
    }
    if button == "auc" {
        let Some((building_x, building_y, _)) =
            crate::game::logic::gui::market_gui::resolve_market_window(state, player_id)
        else {
            return CommandEffects::default();
        };
        return CommandEffects {
            events: Vec::new(),
            saves: vec![crate::game::SaveCommand::AuctionGrid {
                request: crate::game::AuctionGridRequest {
                    player_id,
                    session_id,
                    building_x,
                    building_y,
                },
            }],
            broadcasts: Vec::new(),
        };
    }
    if let Some(item_id) = button
        .strip_prefix("choose:")
        .and_then(|raw| raw.parse::<i32>().ok())
    {
        let Some((building_x, building_y, _)) =
            crate::game::logic::gui::market_gui::resolve_market_window(state, player_id)
        else {
            return CommandEffects::default();
        };
        return CommandEffects {
            events: Vec::new(),
            saves: vec![crate::game::SaveCommand::AuctionItemOrders {
                request: crate::game::AuctionItemOrdersRequest {
                    player_id,
                    session_id,
                    building_x,
                    building_y,
                    item_id,
                },
            }],
            broadcasts: Vec::new(),
        };
    }
    if let Some(order_id) = button
        .strip_prefix("openorder:")
        .and_then(|raw| raw.parse::<i32>().ok())
    {
        let Some((building_x, building_y, _)) =
            crate::game::logic::gui::market_gui::resolve_market_window(state, player_id)
        else {
            return CommandEffects::default();
        };
        return CommandEffects {
            events: Vec::new(),
            saves: vec![crate::game::SaveCommand::AuctionOrder {
                request: crate::game::AuctionOrderRequest {
                    player_id,
                    session_id,
                    building_x,
                    building_y,
                    order_id,
                },
            }],
            broadcasts: Vec::new(),
        };
    }
    if let Some(raw) = button.strip_prefix("auccreate:")
        && let Ok(item_id) = raw.parse::<i32>()
    {
        return auction_presentation_effects(
            state,
            session_id,
            player_id,
            crate::game::logic::auction_gui::auc_order_creation_page(item_id),
        );
    }
    if let Some(raw) = button.strip_prefix("aucsetcost:") {
        let parts: Vec<&str> = raw.split(':').collect();
        if parts.len() == 2
            && let (Ok(item_id), Ok(cost)) = (parts[0].parse::<i32>(), parts[1].parse::<i64>())
        {
            return auction_presentation_effects(
                state,
                session_id,
                player_id,
                crate::game::logic::auction_gui::auc_order_creation_num_page(item_id, cost),
            );
        }
    }
    if let Some(raw) = button.strip_prefix("aucminbet:") {
        let Ok(order_id) = raw.parse::<i32>() else {
            return CommandEffects::default();
        };
        return auction_bet_effects(state, player_id, session_id, order_id, None);
    }
    if let Some(raw) = button.strip_prefix("aucbet:") {
        let Some((raw_id, raw_amount)) = raw.split_once(':') else {
            return CommandEffects::default();
        };
        let Ok(order_id) = raw_id.parse::<i32>() else {
            return CommandEffects::default();
        };
        let Some(amount) = raw_amount.parse::<i64>().ok() else {
            return open_auction_order_effects(state, player_id, session_id, order_id);
        };
        return auction_bet_effects(state, player_id, session_id, order_id, Some(amount));
    }
    if let Some(raw) = button.strip_prefix("aucsetnum:") {
        let parts: Vec<&str> = raw.split(':').collect();
        let Some((item_id, cost, num)) = (parts.len() == 3)
            .then(|| {
                Some((
                    parts[0].parse::<i32>().ok()?,
                    parts[1].parse::<i64>().ok()?,
                    parts[2].parse::<i32>().ok()?,
                ))
            })
            .flatten()
        else {
            let close = crate::protocol::packets::gu_close();
            return CommandEffects {
                events: vec![crate::game::GameEvent::SessionBatch {
                    session_id,
                    player_id,
                    packets: vec![crate::net::session::wire::make_u_packet_bytes(
                        close.0, &close.1,
                    )],
                }],
                saves: Vec::new(),
                broadcasts: Vec::new(),
            };
        };
        let Some((building_x, building_y, _)) =
            crate::game::logic::gui::market_gui::resolve_market_window(state, player_id)
        else {
            return CommandEffects::default();
        };
        let Some(inventory_packets) = state
            .modify_player(player_id, |ecs, entity| {
                let batch = crate::net::session::wire::PacketBatch::default();
                {
                    let mut inventory =
                        ecs.get_mut::<crate::game::player::PlayerInventory>(entity)?;
                    let have = inventory.items.get(&item_id).copied().unwrap_or_default();
                    if num <= 0 || have < num {
                        return Some(None);
                    }
                    *inventory.items.entry(item_id).or_default() -= num;
                    crate::net::session::outbound::inventory_sync::send_inventory(
                        &batch,
                        &mut inventory,
                    );
                }
                let mut flags = ecs.get_mut::<crate::game::player::PlayerFlags>(entity)?;
                flags.dirty = true;
                Some(Some(batch.into_packets()))
            })
            .flatten()
            .flatten()
        else {
            let close = crate::protocol::packets::gu_close();
            return CommandEffects {
                events: vec![crate::game::GameEvent::SessionBatch {
                    session_id,
                    player_id,
                    packets: vec![crate::net::session::wire::make_u_packet_bytes(
                        close.0, &close.1,
                    )],
                }],
                saves: Vec::new(),
                broadcasts: Vec::new(),
            };
        };
        return CommandEffects {
            events: vec![crate::game::GameEvent::SessionBatch {
                session_id,
                player_id,
                packets: inventory_packets,
            }],
            saves: vec![crate::game::SaveCommand::AuctionOrderCreate {
                request: crate::game::AuctionOrderCreateRequest {
                    player_id,
                    session_id,
                    building_x,
                    building_y,
                    item_id,
                    num,
                    cost,
                },
            }],
            broadcasts: Vec::new(),
        };
    }
    if let Some(payload) = button.strip_prefix("transfer:") {
        return apply_storage_transfer(state, session_id, player_id, payload);
    }
    if let Some(rest) = button.strip_prefix("pack_op:take_money:") {
        return apply_pack_withdrawal(state, session_id, player_id, rest, false);
    }
    if let Some(rest) = button.strip_prefix("pack_op:take_crys:") {
        return apply_pack_withdrawal(state, session_id, player_id, rest, true);
    }
    if let Some(type_code) = button.strip_prefix("bld_place:") {
        if let Some(placement) = crate::game::logic::buildings::prepare_paid_building_placement(
            state, tx, player_id, type_code,
        ) {
            spawn_paid_building_insert_task(state, tx.clone(), placement);
        }
        return CommandEffects::default();
    }
    if let Some((x, y)) = parse_pack_remove_button(&button) {
        if !state.enqueue_command(
            player_id,
            session_id,
            crate::game::GameCommand::Player(crate::game::PlayerCommand::RemovePack {
                remove: crate::game::RemovePack {
                    x,
                    y,
                    cause: crate::game::BuildingDeleteCause::PlayerRequest(
                        crate::game::BuildingDeleteOrigin {
                            session_id,
                            player_id,
                        },
                    ),
                },
            }),
        ) {
            crate::net::session::wire::send_u_packet(
                tx,
                "OK",
                &crate::protocol::packets::ok_message(
                    "СЕРВЕР",
                    "Сервер перегружен, повторите действие.",
                )
                .1,
            );
        }
        return CommandEffects::default();
    }
    if let Some(program_id) = button
        .strip_prefix("openprog:")
        .and_then(|rest| rest.parse::<i32>().ok())
    {
        return CommandEffects {
            events: Vec::new(),
            saves: vec![crate::game::SaveCommand::ProgramOpen {
                request: crate::game::ProgramOpenRequest {
                    player_id,
                    session_id,
                    program: program_id,
                },
            }],
            broadcasts: Vec::new(),
        };
    }
    if let Some(name) = button.strip_prefix("createprog:") {
        let name = name.trim();
        if name.is_empty() {
            return CommandEffects::default();
        }
        return CommandEffects {
            events: Vec::new(),
            saves: vec![crate::game::SaveCommand::ProgramCreate {
                request: crate::game::ProgramCreateRequest {
                    player_id,
                    session_id,
                    name: name.to_owned(),
                },
            }],
            broadcasts: Vec::new(),
        };
    }
    if let Some(args) = button.strip_prefix("craft_start:") {
        return apply_crafter_mutation(
            state,
            session_id,
            player_id,
            args,
            crate::game::logic::gui::crafter_gui::handle_craft_start,
        );
    }
    if let Some(args) = button.strip_prefix("craft_claim:") {
        return apply_crafter_mutation(
            state,
            session_id,
            player_id,
            args,
            crate::game::logic::gui::crafter_gui::handle_craft_claim,
        );
    }
    if let Some((program_id, name)) = parse_program_rename_button(&button) {
        return CommandEffects {
            events: Vec::new(),
            saves: vec![crate::game::SaveCommand::ProgramRename {
                request: crate::game::ProgramRenameRequest {
                    player_id,
                    session_id,
                    program_id,
                    name,
                },
            }],
            broadcasts: Vec::new(),
        };
    }
    if let Some(payload) = button.strip_prefix("save:") {
        return apply_settings_save(state, session_id, player_id, payload);
    }
    // Market mutations — route through typed command pipeline
    if let Some(slider_data) = button.strip_prefix("sell:") {
        if let Some((bx, by, _tab)) =
            crate::game::logic::gui::market_gui::resolve_market_window(state, player_id)
            && let Some(sliders) = crate::net::session::ui::crystal_form::parse_amounts(slider_data)
        {
            return super::apply_market_sell(state, player_id, session_id, &sliders, bx, by);
        }
        return CommandEffects::default();
    }
    if let Some(slider_data) = button.strip_prefix("buy:") {
        if let Some((bx, by, _tab)) =
            crate::game::logic::gui::market_gui::resolve_market_window(state, player_id)
            && let Some(sliders) = crate::net::session::ui::crystal_form::parse_amounts(slider_data)
        {
            return super::apply_market_buy(state, player_id, session_id, &sliders, bx, by);
        }
        return CommandEffects::default();
    }
    if button == "sellall" {
        if let Some((bx, by, _tab)) =
            crate::game::logic::gui::market_gui::resolve_market_window(state, player_id)
        {
            return super::apply_market_sell_all(state, player_id, session_id, bx, by);
        }
        return CommandEffects::default();
    }
    if button == "getprofit" {
        if let Some((bx, by, _tab)) =
            crate::game::logic::gui::market_gui::resolve_market_window(state, player_id)
        {
            return super::apply_market_get_profit(state, player_id, session_id, bx, by);
        }
        return CommandEffects::default();
    }
    // Pack operations — route through typed command pipeline
    if let Some(rest) = button.strip_prefix("resp_bind:") {
        let parts: Vec<&str> = rest.split(':').collect();
        if parts.len() == 2
            && let (Ok(x), Ok(y)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>())
        {
            return super::apply_resp_bind(state, player_id, session_id, x, y);
        }
        return CommandEffects::default();
    }
    if let Some(rest) = button.strip_prefix("resp_fill:") {
        let parts: Vec<&str> = rest.split(':').collect();
        if parts.len() == 3
            && let (Ok(x), Ok(y)) = (parts[1].parse::<i32>(), parts[2].parse::<i32>())
        {
            return super::apply_resp_fill(state, player_id, session_id, parts[0], x, y);
        }
        return CommandEffects::default();
    }
    if let Some(rest) = button.strip_prefix("gun_fill:") {
        let parts: Vec<&str> = rest.split(':').collect();
        if parts.len() == 3
            && let (Ok(x), Ok(y)) = (parts[1].parse::<i32>(), parts[2].parse::<i32>())
        {
            return super::apply_gun_fill(state, player_id, session_id, parts[0], x, y);
        }
        return CommandEffects::default();
    }
    if let Some(rest) = button.strip_prefix("resp_profit:") {
        let parts: Vec<&str> = rest.split(':').collect();
        if parts.len() == 2
            && let (Ok(x), Ok(y)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>())
        {
            return super::apply_resp_profit(state, player_id, session_id, x, y);
        }
        return CommandEffects::default();
    }
    if let Some(rest) = button.strip_prefix("resp_save:") {
        return super::apply_resp_save(state, player_id, session_id, rest);
    }
    if let Some(rest) = button.strip_prefix("tp:") {
        return super::apply_teleport(state, player_id, session_id, rest);
    }
    // Up building buttons — route through typed command pipeline
    if button.starts_with("skill:")
        || button == "upgrade"
        || button.starts_with("delete:")
        || button.starts_with("install:")
        || button == "buyslot"
    {
        return super::apply_up_button(state, player_id, session_id, &button);
    }
    if crate::game::logic::gui::gui_buttons::handle_gui_button_sync_fast_path(
        state, tx, player_id, &button,
    ) {
        return CommandEffects::default();
    }
    spawn_gui_async_task(state, tx.clone(), player_id, button);
    CommandEffects::default()
}

fn apply_crafter_mutation(
    state: &Arc<GameState>,
    session_id: crate::game::SessionId,
    player_id: crate::game::PlayerId,
    args: &str,
    handler: fn(
        &Arc<GameState>,
        &dyn crate::net::session::prelude::PacketSink,
        crate::game::PlayerId,
        &str,
    ),
) -> CommandEffects {
    let batch = crate::net::session::wire::PacketBatch::default();
    handler(state, &batch, player_id, args);

    let packets = batch.into_packets();
    let mut effects = CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: packets.clone(),
        }],
        saves: Vec::new(),
        broadcasts: Vec::new(),
    };

    let coordinates = args.split(':').collect::<Vec<_>>();
    let coordinates = if coordinates.len() >= 4 {
        (
            coordinates[2].parse::<i32>().ok(),
            coordinates[3].parse::<i32>().ok(),
        )
    } else if coordinates.len() >= 2 {
        (
            coordinates[0].parse::<i32>().ok(),
            coordinates[1].parse::<i32>().ok(),
        )
    } else {
        (None, None)
    };
    if let (Some(bx), Some(by)) = coordinates
        && let Some(entity) = state.building_entity_at(bx, by)
        && let Some(row) = crate::game::buildings::extract_building_row(
            &state.ecs_read_profiled("commands.crafter_snapshot"),
            entity,
        )
    {
        effects
            .saves
            .push(crate::game::SaveCommand::Building { row: Box::new(row) });
        if packets.iter().any(|packet| {
            openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(packet.as_slice()))
                .is_ok_and(|decoded| decoded.is_some_and(|packet| packet.event_name == *b"GU"))
        }) {
            effects
                .broadcasts
                .push(crate::game::BroadcastEffect::BlockUpdate(
                    crate::game::WorldPos(bx, by),
                ));
        }
    }
    effects
}

fn apply_settings_save(
    state: &Arc<GameState>,
    session_id: crate::game::SessionId,
    player_id: crate::game::PlayerId,
    payload: &str,
) -> CommandEffects {
    let batch = crate::net::session::wire::PacketBatch::default();
    match crate::game::logic::settings::save_settings(state, player_id, payload) {
        Ok(wire) => {
            crate::net::session::wire::send_u_packet(&batch, "#S", &wire);
            crate::net::session::ui::settings::open(state, &batch, player_id);
        }
        Err(crate::game::logic::settings::SettingsSaveError::MalformedPayload) => {
            tracing::warn!(player_id = %player_id, payload, "Malformed settings payload");
            send_settings_error(&batch, "Некорректный формат настроек.");
        }
        Err(crate::game::logic::settings::SettingsSaveError::InvalidInteger("isca")) => {
            tracing::warn!(player_id = %player_id, "Invalid isca setting");
            send_settings_error(&batch, "Некорректный масштаб интерфейса.");
        }
        Err(crate::game::logic::settings::SettingsSaveError::InvalidInteger("tsca")) => {
            tracing::warn!(player_id = %player_id, "Invalid tsca setting");
            send_settings_error(&batch, "Некорректный масштаб территории.");
        }
        Err(crate::game::logic::settings::SettingsSaveError::InvalidInteger(_)) => {
            tracing::warn!(player_id = %player_id, "Invalid integer setting");
            send_settings_error(&batch, "Некорректное значение настройки.");
        }
        Err(crate::game::logic::settings::SettingsSaveError::InvalidBool(_)) => {
            tracing::warn!(player_id = %player_id, "Invalid bool setting");
            send_settings_error(&batch, "Некорректное значение настройки.");
        }
        Err(crate::game::logic::settings::SettingsSaveError::MissingState) => {
            tracing::error!(player_id = %player_id, "Player settings state missing for save");
            send_settings_error(&batch, "Состояние настроек недоступно.");
        }
    }
    CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: batch.into_packets(),
        }],
        saves: Vec::new(),
        broadcasts: Vec::new(),
    }
}

fn send_settings_error(tx: &dyn crate::net::session::wire::PacketSink, message: &str) {
    let packet = crate::protocol::packets::ok_message("НАСТРОЙКИ", message);
    tx.send_packet(crate::net::session::wire::make_u_packet_bytes(
        packet.0, &packet.1,
    ));
}

fn auction_presentation_effects(
    state: &Arc<GameState>,
    session_id: crate::game::SessionId,
    player_id: crate::game::PlayerId,
    page: openmines_protocol::gui::Horb,
) -> CommandEffects {
    let Some((building_x, building_y, _)) =
        crate::game::logic::gui::market_gui::resolve_market_window(state, player_id)
    else {
        return CommandEffects::default();
    };
    let batch = crate::net::session::wire::PacketBatch::default();
    page.send(
        state,
        &batch,
        player_id,
        format!("market:{building_x}:{building_y}:auc"),
    );
    CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: batch.into_packets(),
        }],
        saves: Vec::new(),
        broadcasts: Vec::new(),
    }
}

fn auction_bet_effects(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    order_id: i32,
    requested_amount: Option<i64>,
) -> CommandEffects {
    let Some((building_x, building_y, _)) =
        crate::game::logic::gui::market_gui::resolve_market_window(state, player_id)
    else {
        return CommandEffects::default();
    };
    let Some(bidder_money) = state.query_player_opt(player_id, |ecs, entity| {
        ecs.get::<crate::game::player::PlayerStats>(entity)
            .map(|stats| stats.money)
    }) else {
        return CommandEffects::default();
    };
    CommandEffects {
        events: Vec::new(),
        saves: vec![crate::game::SaveCommand::AuctionBet {
            request: crate::game::AuctionBetRequest {
                player_id,
                session_id,
                building_x,
                building_y,
                order_id,
                requested_amount,
                bidder_money,
            },
        }],
        broadcasts: Vec::new(),
    }
}

fn open_auction_order_effects(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    order_id: i32,
) -> CommandEffects {
    let Some((building_x, building_y, _)) =
        crate::game::logic::gui::market_gui::resolve_market_window(state, player_id)
    else {
        return CommandEffects::default();
    };
    CommandEffects {
        events: Vec::new(),
        saves: vec![crate::game::SaveCommand::AuctionOrder {
            request: crate::game::AuctionOrderRequest {
                player_id,
                session_id,
                building_x,
                building_y,
                order_id,
            },
        }],
        broadcasts: Vec::new(),
    }
}

fn clan_mutation_action_for_button(button: &str) -> Option<crate::game::ClanAction> {
    if button == "clan_leave" {
        return Some(crate::game::ClanAction::Leave);
    }
    let (prefix, raw_id) = button.split_once(':')?;
    let clan_id = raw_id.parse().ok()?;
    match prefix {
        "clan_invite_accept" => Some(crate::game::ClanAction::AcceptInvite { clan_id }),
        "clan_invite_decline" => Some(crate::game::ClanAction::DeclineInvite { clan_id }),
        "clan_accept" => Some(crate::game::ClanAction::AcceptRequest {
            target_id: crate::game::PlayerId::from(clan_id),
        }),
        "clan_decline" => Some(crate::game::ClanAction::DeclineRequest {
            target_id: crate::game::PlayerId::from(clan_id),
        }),
        "clan_promote" => Some(crate::game::ClanAction::Promote {
            target_id: crate::game::PlayerId::from(clan_id),
        }),
        "clan_kick_id" => Some(crate::game::ClanAction::KickById {
            target_id: crate::game::PlayerId::from(clan_id),
        }),
        "clan_invite_send" => Some(crate::game::ClanAction::Invite {
            target_id: crate::game::PlayerId::from(clan_id),
        }),
        "clan_request" => Some(crate::game::ClanAction::Request { clan_id }),
        _ => None,
    }
}

fn clan_command_effects(
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    action: crate::game::ClanAction,
) -> CommandEffects {
    CommandEffects {
        events: Vec::new(),
        saves: vec![crate::game::SaveCommand::ClanCommand {
            request: crate::game::ClanCommandRequest {
                player_id,
                session_id,
                action,
                create_reserved: false,
            },
        }],
        broadcasts: Vec::new(),
    }
}

fn clan_menu_action_for_button(button: &str) -> Option<crate::game::ClanMenuAction> {
    match button {
        "clan_menu" | "clan_back" => Some(crate::game::ClanMenuAction::Main),
        "clan_members" => Some(crate::game::ClanMenuAction::Members),
        "clan_invite_list" => Some(crate::game::ClanMenuAction::InviteList),
        "clan_invites_view" => Some(crate::game::ClanMenuAction::Invites),
        "clan_requests" => Some(crate::game::ClanMenuAction::Requests),
        _ => button
            .strip_prefix("clan_view:")?
            .parse()
            .ok()
            .map(|clan_id| crate::game::ClanMenuAction::Preview { clan_id }),
    }
}

fn is_migrated_clan_button(button: &str) -> bool {
    matches!(
        button,
        "clan_menu"
            | "clan_back"
            | "clan_requests"
            | "clan_members"
            | "clan_invite_list"
            | "clan_invites_view"
            | "clan_leave"
    ) || button.starts_with("clan_view:")
        || button.starts_with("clan_invite_accept:")
        || button.starts_with("clan_invite_decline:")
        || button.starts_with("clan_accept:")
        || button.starts_with("clan_decline:")
        || button.starts_with("clan_promote:")
        || button.starts_with("clan_kick_id:")
        || button.starts_with("clan_invite_send:")
        || button.starts_with("clan_request:")
}

pub(super) fn clan_menu_effects(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    action: crate::game::ClanMenuAction,
) -> CommandEffects {
    let player_clan_id = state.query_player_opt(player_id, |ecs, entity| {
        ecs.get::<crate::game::player::PlayerStats>(entity)
            .and_then(|stats| stats.clan_id)
    });
    let invite_candidates = matches!(action, crate::game::ClanMenuAction::InviteList)
        .then(|| {
            state
                .active_player_ids()
                .into_iter()
                .filter(|&candidate_id| candidate_id != player_id)
                .filter_map(|candidate_id| {
                    state.query_player_opt(candidate_id, |ecs, entity| {
                        let player_stats = ecs.get::<crate::game::player::PlayerStats>(entity)?;
                        let metadata = ecs.get::<crate::game::player::PlayerMetadata>(entity)?;
                        player_stats
                            .clan_id
                            .is_none()
                            .then(|| (candidate_id.as_i32(), metadata.name.clone()))
                    })
                })
                .take(20)
                .collect()
        })
        .unwrap_or_default();
    CommandEffects {
        events: Vec::new(),
        saves: vec![crate::game::SaveCommand::ClanMenu {
            request: crate::game::ClanMenuRequest {
                player_id,
                session_id,
                player_clan_id,
                action,
                invite_candidates,
            },
        }],
        broadcasts: Vec::new(),
    }
}

fn clan_pack_menu_effects(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    x: i32,
    y: i32,
) -> CommandEffects {
    let Some(view) = state.get_pack_at(x, y) else {
        return CommandEffects::default();
    };
    if view.pack_type != crate::game::PackType::Clans {
        return CommandEffects::default();
    }
    let player = state.query_player_opt(player_id, |ecs, entity| {
        let position = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
        let player_stats = ecs.get::<crate::game::player::PlayerStats>(entity)?;
        Some((position.x, position.y, player_stats.clan_id.unwrap_or(0)))
    });
    let Some((player_x, player_y, player_clan_id)) = player else {
        return CommandEffects::default();
    };
    if crate::game::buildings::validate_pack_access(
        &view,
        (player_x, player_y),
        player_clan_id,
        player_id,
    )
    .is_err()
    {
        return CommandEffects::default();
    }
    clan_menu_effects(
        state,
        player_id,
        session_id,
        crate::game::ClanMenuAction::Main,
    )
}

fn apply_storage_transfer(
    state: &Arc<GameState>,
    session_id: crate::game::SessionId,
    player_id: crate::game::PlayerId,
    payload: &str,
) -> CommandEffects {
    match crate::game::logic::gui::storage::transfer(state, player_id, payload) {
        Ok(Some(transfer)) => {
            let basket = crate::protocol::packets::basket(&transfer.crystals, 1);
            CommandEffects {
                events: vec![
                    crate::game::GameEvent::SessionBatch {
                        session_id,
                        player_id,
                        packets: vec![crate::net::session::wire::make_u_packet_bytes(
                            basket.0, &basket.1,
                        )],
                    },
                    crate::game::GameEvent::GuiView {
                        session_id,
                        player_id,
                        view: crate::game::GuiView::Storage(transfer.view),
                    },
                ],
                saves: Vec::new(),
                broadcasts: Vec::new(),
            }
        }
        Ok(None) => CommandEffects::default(),
        Err(crate::game::logic::gui::storage::StorageTransferError::MissingState) => {
            let packet =
                crate::protocol::packets::ok_message("ЗДАНИЕ", "Состояние здания недоступно.");
            CommandEffects {
                events: vec![crate::game::GameEvent::SessionBatch {
                    session_id,
                    player_id,
                    packets: vec![crate::net::session::wire::make_u_packet_bytes(
                        packet.0, &packet.1,
                    )],
                }],
                saves: Vec::new(),
                broadcasts: Vec::new(),
            }
        }
    }
}

fn apply_pack_withdrawal(
    state: &Arc<GameState>,
    session_id: crate::game::SessionId,
    player_id: crate::game::PlayerId,
    coordinates: &str,
    crystals: bool,
) -> CommandEffects {
    let Some((raw_x, raw_y)) = coordinates.split_once(':') else {
        return pack_withdrawal_error(session_id, player_id, false);
    };
    let (Ok(x), Ok(y)) = (raw_x.parse::<i32>(), raw_y.parse::<i32>()) else {
        return pack_withdrawal_error(session_id, player_id, false);
    };
    if !crate::game::logic::pack_command::withdraw_state_ready(state, player_id, x, y) {
        return pack_withdrawal_error(session_id, player_id, true);
    }
    let Some(player_entity) = state.get_player_entity(player_id) else {
        return pack_withdrawal_error(session_id, player_id, true);
    };
    let Some(building_entity) = state.building_entity_at(x, y) else {
        return pack_withdrawal_error(session_id, player_id, true);
    };

    enum WithdrawalAmount {
        Money(i64),
        Crystals([i64; 6]),
    }
    let mut packets = Vec::new();
    let building_row = {
        let mut ecs = state.ecs_write_profiled("commands.pack_withdrawal");
        let amount = {
            let Some(mut storage) =
                ecs.get_mut::<crate::game::buildings::BuildingStorage>(building_entity)
            else {
                return pack_withdrawal_error(session_id, player_id, true);
            };
            if crystals {
                let amount = storage.crystals;
                storage.crystals = [0; 6];
                WithdrawalAmount::Crystals(amount)
            } else {
                let amount = storage.money;
                storage.money = 0;
                WithdrawalAmount::Money(amount)
            }
        };

        match amount {
            WithdrawalAmount::Crystals(amount) => {
                if amount.iter().any(|value| *value > 0) {
                    let now = {
                        let Some(mut player_stats) =
                            ecs.get_mut::<crate::game::player::PlayerStats>(player_entity)
                        else {
                            return pack_withdrawal_error(session_id, player_id, true);
                        };
                        for (current, delta) in player_stats.crystals.iter_mut().zip(amount) {
                            *current = current.saturating_add(delta);
                        }
                        player_stats.crystals
                    };
                    packets.push(crate::net::session::wire::make_u_packet_bytes(
                        "@B",
                        &crate::protocol::packets::basket(&now, 1).1,
                    ));
                    if let Some(mut flags) =
                        ecs.get_mut::<crate::game::player::PlayerFlags>(player_entity)
                    {
                        flags.dirty = true;
                    }
                }
            }
            WithdrawalAmount::Money(amount) if amount > 0 => {
                let Some((now, creds)) = ecs
                    .get_mut::<crate::game::player::PlayerStats>(player_entity)
                    .map(|mut stats| {
                        stats.money = stats.money.saturating_add(amount);
                        (stats.money, stats.creds)
                    })
                else {
                    return pack_withdrawal_error(session_id, player_id, true);
                };
                packets.push(crate::net::session::wire::make_u_packet_bytes(
                    "P$",
                    &crate::protocol::packets::money(now, creds).1,
                ));
                if let Some(mut flags) =
                    ecs.get_mut::<crate::game::player::PlayerFlags>(player_entity)
                {
                    flags.dirty = true;
                }
            }
            WithdrawalAmount::Money(_) => {}
        }

        let Some(mut flags) = ecs.get_mut::<crate::game::buildings::BuildingFlags>(building_entity)
        else {
            return pack_withdrawal_error(session_id, player_id, true);
        };
        flags.dirty = true;
        ecs.resource_mut::<crate::game::DirtyBuildings>()
            .0
            .insert(building_entity);
        let row = crate::game::buildings::extract_building_row(&ecs, building_entity);
        drop(ecs);
        row
    };

    let mut effects = CommandEffects {
        events: if packets.is_empty() {
            Vec::new()
        } else {
            vec![crate::game::GameEvent::SessionBatch {
                session_id,
                player_id,
                packets,
            }]
        },
        saves: Vec::new(),
        broadcasts: Vec::new(),
    };
    if let Some(row) = building_row {
        effects
            .saves
            .push(crate::game::SaveCommand::Building { row: Box::new(row) });
    }
    effects
}

fn pack_withdrawal_error(
    session_id: crate::game::SessionId,
    player_id: crate::game::PlayerId,
    state_error: bool,
) -> CommandEffects {
    let packet = if state_error {
        crate::protocol::packets::ok_message("ЗДАНИЕ", "Состояние здания недоступно.")
    } else {
        crate::protocol::packets::ok_message("ЗДАНИЕ", "Некорректное действие.")
    };
    CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: vec![crate::net::session::wire::make_u_packet_bytes(
                packet.0, &packet.1,
            )],
        }],
        ..CommandEffects::default()
    }
}

#[derive(Clone, Copy)]
enum GuiAsyncHandler {
    Auction,
    Other,
}

fn spawn_gui_async_task(
    state: &Arc<GameState>,
    tx: crate::net::session::outbox::Outbox,
    player_id: crate::game::PlayerId,
    button: String,
) {
    let handler = if crate::game::logic::gui::gui_buttons::is_auction_button(&button) {
        GuiAsyncHandler::Auction
    } else {
        GuiAsyncHandler::Other
    };
    let task_name = match handler {
        GuiAsyncHandler::Auction => "auction_gui",
        GuiAsyncHandler::Other => "other_gui_button",
    };
    let task_state = state.clone();
    spawn_session_async_task(state, task_name, async move {
        match handler {
            GuiAsyncHandler::Auction => {
                crate::game::logic::gui::gui_buttons::handle_auction_button(
                    &task_state,
                    &tx,
                    player_id,
                    &button,
                )
                .await;
            }
            GuiAsyncHandler::Other => {
                crate::game::logic::gui::gui_buttons::handle_gui_button(
                    &task_state,
                    &tx,
                    player_id,
                    &button,
                )
                .await;
            }
        }
    });
}
