use super::{
    Arc, CommandEffects, GameState, PlayerCommand, parse_pack_remove_button,
    parse_program_rename_button, spawn_paid_building_insert_task, spawn_program_editor_open_task,
    spawn_program_editor_rename_task, spawn_session_async_task,
};

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
    if let Some(payload) = button.strip_prefix("transfer:") {
        return apply_storage_transfer(state, session_id, player_id, payload);
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
        spawn_program_editor_open_task(state, tx.clone(), player_id, program_id);
        return CommandEffects::default();
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
    if let Some((program_id, name)) = parse_program_rename_button(&button) {
        spawn_program_editor_rename_task(state, tx.clone(), player_id, program_id, &name);
        return CommandEffects::default();
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

#[derive(Clone, Copy)]
enum GuiAsyncHandler {
    Auction,
    Clan,
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
    } else if crate::game::logic::gui::gui_buttons::is_clan_button(&button) {
        GuiAsyncHandler::Clan
    } else {
        GuiAsyncHandler::Other
    };
    let task_name = match handler {
        GuiAsyncHandler::Auction => "auction_gui",
        GuiAsyncHandler::Clan => "clan_gui",
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
            GuiAsyncHandler::Clan => {
                crate::game::logic::gui::gui_buttons::handle_clan_button(
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
