#![allow(
    clippy::too_many_lines,
    clippy::needless_pass_by_value,
    clippy::option_if_let_else,
    clippy::assigning_clones,
    clippy::items_after_statements,
    clippy::used_underscore_binding,
    clippy::semicolon_if_nothing_returned,
    clippy::missing_panics_doc
)]
use super::{Arc, CommandEffects, GameState, KernelContext, PlayerCommand};
use openmines_protocol::gui::{Button, Horb};
use std::fmt::Write;

pub fn apply_persistence_completion(
    state: &Arc<GameState>,
    completion: crate::game::PersistenceCompletion,
) -> CommandEffects {
    match completion {
        crate::game::PersistenceCompletion::ProgramMenuLoaded { request, result } => {
            let mut effects = CommandEffects::default();
            if state.sessions.session_for_player(request.player_id) != Some(request.session_id) {
                return effects;
            }
            match result {
                crate::game::ProgramMenuResult::Loaded { programs } => {
                    use openmines_protocol::gui::{Button, Horb, ListRow};
                    let mut window = Horb::new("ПРОГРАММАТОР");
                    if programs.is_empty() {
                        window = window
                            .text("Нет программ")
                            .button(Button::new("СОЗДАТЬ ПРОГРАММУ", "createprog"));
                    } else {
                        for program in programs {
                            window = window.list_row(ListRow::new(
                                program.name,
                                "ОТКРЫТЬ",
                                format!("openprog:{}", program.id),
                            ));
                        }
                        window = window.button(Button::new("Создать", "createprog"));
                    }
                    state.modify_player(request.player_id, |ecs, entity| {
                        if let Some(mut ui) = ecs.get_mut::<crate::game::player::PlayerUI>(entity) {
                            ui.current_window = Some("prog".to_string());
                        }
                    });
                    effects.events.push(crate::game::GameEvent::SessionBatch {
                        session_id: request.session_id,
                        player_id: request.player_id,
                        packets: vec![crate::net::session::wire::make_u_packet_bytes(
                            "GU",
                            &window.payload(),
                        )],
                    });
                }
                crate::game::ProgramMenuResult::PermanentFailure { message } => {
                    tracing::error!(player_id = %request.player_id, error = %message, "Program menu load failed permanently");
                    effects.append(KernelContext::new(state).slash_ok_effect(
                        request.session_id,
                        request.player_id,
                        "Ошибка",
                        "Ошибка БД",
                    ));
                }
            }
            effects
        }
        crate::game::PersistenceCompletion::ProgramCopied { request, result } => {
            if state.sessions.session_for_player(request.player) != Some(request.session) {
                return CommandEffects::default();
            }
            match result {
                crate::game::ProgramCopyResult::Copied => CommandEffects {
                    events: Vec::new(),
                    saves: vec![crate::game::SaveCommand::ProgramMenu {
                        request: crate::game::ProgramMenuRequest {
                            player_id: request.player,
                            session_id: request.session,
                        },
                    }],
                    broadcasts: Vec::new(),
                },
                crate::game::ProgramCopyResult::Rejected => KernelContext::new(state)
                    .slash_ok_effect(
                        request.session,
                        request.player,
                        "ПРОГРАММАТОР",
                        "Программа недоступна.",
                    ),
                crate::game::ProgramCopyResult::PermanentFailure { message } => {
                    tracing::error!(
                        player_id = %request.player,
                        program_id = request.program,
                        error = %message,
                        "Program copy permanently rejected by persistence"
                    );
                    KernelContext::new(state).slash_ok_effect(
                        request.session,
                        request.player,
                        "ПРОГРАММАТОР",
                        "Не удалось скопировать программу.",
                    )
                }
            }
        }
        crate::game::PersistenceCompletion::BuildingMenuLoaded { request, result } => {
            if state.sessions.session_for_player(request.player_id) != Some(request.session_id) {
                return CommandEffects::default();
            }
            match result {
                crate::game::BuildingMenuResult::Loaded { buildings } => {
                    use openmines_protocol::gui::{Horb, ListRow};
                    let mut window = Horb::new("Мои здания");
                    if buildings.is_empty() {
                        window = window.text("(нет построек)");
                    } else {
                        for building in buildings {
                            window = window.list_row(ListRow::new(
                                format!("{} {}:{}", building.type_code, building.x, building.y),
                                String::new(),
                                String::new(),
                            ));
                        }
                    }
                    window = window.close_button();
                    state.modify_player(request.player_id, |ecs, entity| {
                        if let Some(mut ui) = ecs.get_mut::<crate::game::player::PlayerUI>(entity) {
                            ui.current_window = Some("blds".to_string());
                        }
                    });
                    CommandEffects {
                        events: vec![crate::game::GameEvent::SessionBatch {
                            session_id: request.session_id,
                            player_id: request.player_id,
                            packets: vec![crate::net::session::wire::make_u_packet_bytes(
                                "GU",
                                &window.payload(),
                            )],
                        }],
                        saves: Vec::new(),
                        broadcasts: Vec::new(),
                    }
                }
                crate::game::BuildingMenuResult::PermanentFailure { message } => {
                    tracing::error!(player_id = %request.player_id, error = %message, "Building menu load failed permanently");
                    KernelContext::new(state).slash_ok_effect(
                        request.session_id,
                        request.player_id,
                        "Ошибка",
                        "Ошибка БД",
                    )
                }
            }
        }
        crate::game::PersistenceCompletion::ProgramSaved { request, result } => {
            if state.sessions.session_for_player(request.player_id) != Some(request.session_id) {
                return CommandEffects::default();
            }
            let Some(tx) = state.sessions.outbox_for_session(request.session_id) else {
                return CommandEffects::default();
            };
            match result {
                crate::game::ProgramSaveResult::Saved { program_name } => {
                    crate::net::session::social::misc::apply_saved_program_to_tick_state(
                        state,
                        &tx,
                        request.player_id,
                        request.program_id,
                        &program_name,
                        &request.source,
                    );
                }
                crate::game::ProgramSaveResult::Rejected => {
                    tracing::warn!(
                        player_id = %request.player_id,
                        program_id = request.program_id,
                        "Program save rejected: missing or foreign row"
                    );
                    crate::net::session::wire::send_u_packet(
                        &tx,
                        "OK",
                        &crate::protocol::packets::ok_message(
                            "ПРОГРАММАТОР",
                            "Не удалось сохранить программу.",
                        )
                        .1,
                    );
                }
                crate::game::ProgramSaveResult::PermanentFailure { message } => {
                    tracing::error!(
                        player_id = %request.player_id,
                        program_id = request.program_id,
                        error = message,
                        "Program save permanently rejected by persistence"
                    );
                    crate::net::session::wire::send_u_packet(
                        &tx,
                        "OK",
                        &crate::protocol::packets::ok_message(
                            "ПРОГРАММАТОР",
                            "Не удалось сохранить программу.",
                        )
                        .1,
                    );
                }
            }
            CommandEffects::default()
        }
        crate::game::PersistenceCompletion::ProgramCreated { request, result } => {
            if state.sessions.session_for_player(request.player_id) != Some(request.session_id) {
                return CommandEffects::default();
            }
            match result {
                crate::game::ProgramCreateResult::Created { program_id } => {
                    apply_program_editor_completion(
                        state,
                        request.session_id,
                        request.player_id,
                        PlayerCommand::ApplyProgramEditorOpen {
                            program_id,
                            program_name: request.name,
                            source: String::new(),
                        },
                    );
                }
                crate::game::ProgramCreateResult::PermanentFailure { message } => {
                    tracing::error!(player_id = %request.player_id, error = message, "Program create permanently rejected by persistence");
                    if let Some(tx) = state.sessions.outbox_for_session(request.session_id) {
                        crate::net::session::ui::gui_buttons::send_programmator_action_error(
                            &tx,
                            "Не удалось создать программу.",
                        );
                    }
                }
            }
            CommandEffects::default()
        }
        crate::game::PersistenceCompletion::BuildingDeleted { request, result } => {
            adapt_building_delete_completion(crate::game::logic::building_delete::apply_completion(
                state, request, result,
            ))
        }
        crate::game::PersistenceCompletion::ChatColorCycled { request, result } => {
            if state.sessions.session_for_player(request.player_id) != Some(request.session_id) {
                return CommandEffects::default();
            }
            let Some(tx) = state.sessions.outbox_for_session(request.session_id) else {
                return CommandEffects::default();
            };
            match result {
                crate::game::ChatColorCycleResult::Cycled { color } => {
                    crate::net::session::wire::send_u_packet(
                        &tx,
                        "mC",
                        &crate::protocol::packets::chat_color(color).1,
                    );
                }
                crate::game::ChatColorCycleResult::Rejected => {
                    tracing::warn!(player_id = %request.player_id, "Chat color cycle rejected: player is missing");
                }
                crate::game::ChatColorCycleResult::PermanentFailure { message } => {
                    tracing::error!(player_id = %request.player_id, error = message, "Chat color cycle failed");
                    crate::net::session::wire::send_u_packet(
                        &tx,
                        "OK",
                        &crate::protocol::packets::ok_message("Ошибка", "Ошибка БД").1,
                    );
                }
            }
            CommandEffects::default()
        }
        crate::game::PersistenceCompletion::ChatResynced { request, result } => {
            if state.sessions.session_for_player(request.player_id) != Some(request.session_id) {
                return CommandEffects::default();
            }
            let Some(tx) = state.sessions.outbox_for_session(request.session_id) else {
                return CommandEffects::default();
            };
            match result {
                crate::game::ChatResyncResult::Success {
                    channel_name,
                    messages,
                } => {
                    state.modify_player(request.player_id, |w, e| {
                        if let Some(mut ui) = w.get_mut::<crate::game::player::PlayerUI>(e) {
                            ui.current_chat.clone_from(&request.channel_tag);
                        }
                    });

                    let mo =
                        crate::protocol::packets::chat_current(&request.channel_tag, &channel_name);
                    let mu =
                        crate::protocol::packets::chat_messages(&request.channel_tag, &messages);
                    crate::net::session::wire::send_u_packet(&tx, "mO", &mo.1);
                    crate::net::session::wire::send_u_packet(&tx, "mU", &mu.1);
                }
                crate::game::ChatResyncResult::AccessDenied => {
                    tracing::warn!(player_id = %request.player_id, chat_tag = %request.channel_tag, "Resync access denied");
                }
                crate::game::ChatResyncResult::PermanentFailure { message } => {
                    tracing::error!(player_id = %request.player_id, error = %message, "Resync failed permanently");
                    crate::net::session::wire::send_u_packet(
                        &tx,
                        "OK",
                        &crate::protocol::packets::ok_message(
                            "ЧАТ",
                            "Не удалось прочитать данные чата.",
                        )
                        .1,
                    );
                }
            }
            CommandEffects::default()
        }
        crate::game::PersistenceCompletion::ChatMenuLoaded { request, result } => {
            if state.sessions.session_for_player(request.player_id) != Some(request.session_id) {
                return CommandEffects::default();
            }
            let Some(tx) = state.sessions.outbox_for_session(request.session_id) else {
                return CommandEffects::default();
            };
            match result {
                crate::game::ChatMenuResult::Success { mut channels } => {
                    let mut entries: Vec<(String, bool, String, String)> = {
                        let global_channels = state.chat_channels.read();
                        global_channels
                            .iter()
                            .filter(|c| c.global)
                            .map(|c| {
                                let preview = c
                                    .messages
                                    .back()
                                    .map(|m| format!("{}: {}", m.nickname, m.text))
                                    .unwrap_or_default();
                                (c.tag.clone(), false, c.name.clone(), preview)
                            })
                            .collect()
                    };

                    entries.append(&mut channels);

                    let ml = crate::protocol::packets::chat_list(&entries);
                    let mn = crate::protocol::packets::chat_notification(0);
                    crate::net::session::wire::send_u_packet(&tx, "mL", &ml.1);
                    crate::net::session::wire::send_u_packet(&tx, "mN", &mn.1);
                }
                crate::game::ChatMenuResult::PermanentFailure { message } => {
                    tracing::error!(player_id = %request.player_id, error = %message, "Menu load failed permanently");
                    crate::net::session::wire::send_u_packet(
                        &tx,
                        "OK",
                        &crate::protocol::packets::ok_message(
                            "ЧАТ",
                            "Не удалось прочитать данные чата.",
                        )
                        .1,
                    );
                }
            }
            CommandEffects::default()
        }
        crate::game::PersistenceCompletion::ChatPrivateOpened { request, result } => {
            if state.sessions.session_for_player(request.player_id) != Some(request.session_id) {
                return CommandEffects::default();
            }
            let Some(tx) = state.sessions.outbox_for_session(request.session_id) else {
                return CommandEffects::default();
            };
            match result {
                crate::game::ChatPrivateResult::Success {
                    target_name,
                    channel_tag,
                    messages,
                } => {
                    state.modify_player(request.player_id, |w, e| {
                        if let Some(mut ui) = w.get_mut::<crate::game::player::PlayerUI>(e) {
                            ui.current_chat.clone_from(&channel_tag);
                        }
                    });

                    let mo = crate::protocol::packets::chat_current(&channel_tag, &target_name);
                    let mu = crate::protocol::packets::chat_messages(&channel_tag, &messages);
                    crate::net::session::wire::send_u_packet(&tx, "mO", &mo.1);
                    crate::net::session::wire::send_u_packet(&tx, "mU", &mu.1);
                }
                crate::game::ChatPrivateResult::TargetNotFound => {
                    tracing::warn!(player_id = %request.player_id, target = ?request.target_uid, "Private chat target not found");
                }
                crate::game::ChatPrivateResult::PermanentFailure { message } => {
                    tracing::error!(player_id = %request.player_id, error = %message, "Private chat open failed permanently");
                    crate::net::session::wire::send_u_packet(
                        &tx,
                        "OK",
                        &crate::protocol::packets::ok_message("Ошибка", "Ошибка БД").1,
                    );
                }
            }
            CommandEffects::default()
        }
        crate::game::PersistenceCompletion::WhoisLoaded { request, result } => {
            if state.sessions.session_for_player(request.player_id) != Some(request.session_id) {
                return CommandEffects::default();
            }
            match result {
                crate::game::WhoisResult::Loaded { names } => {
                    let payload = names
                        .into_iter()
                        .map(|(id, name)| format!("{id}:{name}"))
                        .collect::<Vec<_>>()
                        .join(",")
                        .into_bytes();
                    CommandEffects {
                        events: vec![crate::game::GameEvent::SessionBatch {
                            session_id: request.session_id,
                            player_id: request.player_id,
                            packets: vec![crate::net::session::wire::make_u_packet_bytes(
                                "NL", &payload,
                            )],
                        }],
                        saves: Vec::new(),
                        broadcasts: Vec::new(),
                    }
                }
                crate::game::WhoisResult::PermanentFailure { message } => {
                    tracing::error!(player_id = %request.player_id, error = %message, "Whois load failed permanently");
                    KernelContext::new(state).slash_ok_effect(
                        request.session_id,
                        request.player_id,
                        "НИКИ",
                        "Не удалось прочитать имя игрока.",
                    )
                }
            }
        }
        crate::game::PersistenceCompletion::ClanMenuLoaded { request, result } => {
            if state.sessions.session_for_player(request.player_id) != Some(request.session_id) {
                return CommandEffects::default();
            }
            let window = match result {
                crate::game::ClanMenuResult::Browse { invites, clans } => {
                    let mut window = Horb::new("КЛАНЫ").text("Выберите клан или создайте свой");
                    if !invites.is_empty() {
                        window = window.button(Button::new(
                            format!("Приглашения ({})", invites.len()),
                            "clan_invites_view",
                        ));
                    }
                    window = window.button(Button::new("Создать клан (1000 кр.)", "clan_create"));
                    for clan in clans {
                        window = window.button(Button::new(
                            format!("{} [{}] ({} чел.)", clan.name, clan.abr, clan.member_count),
                            format!("clan_view:{}", clan.id),
                        ));
                    }
                    window.close_button()
                }
                crate::game::ClanMenuResult::Info {
                    clan,
                    owner_name,
                    player_rank,
                    request_count,
                    can_leave,
                } => {
                    let rank = crate::db::ClanRank::from_db(player_rank);
                    let text = format!(
                        "Клан: {} [{}]\nУчастники: {}\nВладелец: {}\nВаш ранг: {:?}",
                        clan.name, clan.abr, clan.member_count, owner_name, rank
                    );
                    let mut window = Horb::new(clan.name)
                        .text(text)
                        .button(Button::new("Участники", "clan_members"));
                    if let Some(request_count) = request_count {
                        window = window
                            .button(Button::new(
                                format!("Заявки ({request_count})"),
                                "clan_requests",
                            ))
                            .button(Button::new("Пригласить игрока", "clan_invite_list"));
                    }
                    if can_leave {
                        window = window.button(Button::new("Покинуть клан", "clan_leave"));
                    }
                    window.close_button()
                }
                crate::game::ClanMenuResult::Preview {
                    clan,
                    owner_name,
                    can_request_join,
                } => {
                    let text = format!(
                        "Клан: {} [{}]\nУчастники: {}\nВладелец: {}",
                        clan.name, clan.abr, clan.member_count, owner_name
                    );
                    let mut window = Horb::new(clan.name).text(text);
                    if can_request_join {
                        window = window.button(Button::new(
                            "Подать заявку",
                            format!("clan_request:{}", clan.id),
                        ));
                    }
                    window
                        .button(Button::new("Назад", "clan_back"))
                        .close_button()
                }
                crate::game::ClanMenuResult::Members {
                    members,
                    player_rank,
                } => {
                    let player_rank = crate::db::ClanRank::from_db(player_rank);
                    let mut text = String::from("Участники клана:\n");
                    let mut window = Horb::new("Участники");
                    for member in members {
                        let rank = crate::db::ClanRank::from_db(member.rank);
                        let _ = writeln!(text, "- {} ({rank:?})", member.name);
                        if member.player_id != request.player_id.as_i32() {
                            if player_rank == crate::db::ClanRank::Leader
                                && rank == crate::db::ClanRank::Member
                            {
                                window = window.button(Button::new(
                                    format!("Повысить {}", member.name),
                                    format!("clan_promote:{}", member.player_id),
                                ));
                            }
                            if player_rank > rank {
                                window = window.button(Button::new(
                                    format!("Исключить {}", member.name),
                                    format!("clan_kick_id:{}", member.player_id),
                                ));
                            }
                        }
                    }
                    window
                        .text(text)
                        .button(Button::new("Назад", "clan_back"))
                        .close_button()
                }
                crate::game::ClanMenuResult::InviteList {
                    allowed,
                    candidates,
                } => {
                    if !allowed {
                        return KernelContext::new(state).slash_ok_effect(
                            request.session_id,
                            request.player_id,
                            "Ошибка",
                            "Нет прав",
                        );
                    }
                    let mut window =
                        Horb::new("Пригласить").text("Выберите игрока для приглашения в клан:");
                    let no_candidates = candidates.is_empty();
                    for (candidate_id, candidate_name) in candidates {
                        window = window.button(Button::new(
                            format!("Пригласить {candidate_name}"),
                            format!("clan_invite_send:{candidate_id}"),
                        ));
                    }
                    if no_candidates {
                        window = window.button(Button::new("Никого нет рядом без клана", "noop"));
                    }
                    window
                        .button(Button::new("Назад", "clan_back"))
                        .close_button()
                }
                crate::game::ClanMenuResult::Invites { invites } => {
                    let mut window =
                        Horb::new("Приглашения").text("Вас пригласили в следующие кланы:");
                    for (clan_id, clan_name) in invites {
                        window = window
                            .button(Button::new(
                                format!("Принять {clan_name}"),
                                format!("clan_invite_accept:{clan_id}"),
                            ))
                            .button(Button::new(
                                format!("Отклонить {clan_name}"),
                                format!("clan_invite_decline:{clan_id}"),
                            ));
                    }
                    window
                        .button(Button::new("Назад", "clan_back"))
                        .close_button()
                }
                crate::game::ClanMenuResult::Requests { allowed, requests } => {
                    if !allowed {
                        return KernelContext::new(state).slash_ok_effect(
                            request.session_id,
                            request.player_id,
                            "Ошибка",
                            "Нет прав",
                        );
                    }
                    let mut window = Horb::new("Заявки").text("Заявки в клан:");
                    for (player_id, player_name) in requests {
                        window = window
                            .button(Button::new(
                                format!("{player_name} - Принять"),
                                format!("clan_accept:{player_id}"),
                            ))
                            .button(Button::new(
                                format!("{player_name} - Отклонить"),
                                format!("clan_decline:{player_id}"),
                            ));
                    }
                    window
                        .button(Button::new("Назад", "clan_back"))
                        .close_button()
                }
                crate::game::ClanMenuResult::NotFound => {
                    return KernelContext::new(state).slash_ok_effect(
                        request.session_id,
                        request.player_id,
                        "Ошибка",
                        "Клан не найден",
                    );
                }
                crate::game::ClanMenuResult::PermanentFailure { message } => {
                    tracing::error!(player_id = %request.player_id, error = %message, "Clan menu load failed permanently");
                    return KernelContext::new(state).slash_ok_effect(
                        request.session_id,
                        request.player_id,
                        "Ошибка",
                        "Ошибка БД",
                    );
                }
            };
            state.modify_player(request.player_id, |ecs, entity| {
                if let Some(mut ui) = ecs.get_mut::<crate::game::player::PlayerUI>(entity) {
                    ui.current_window = Some("clan".to_string());
                }
            });
            CommandEffects {
                events: vec![crate::game::GameEvent::SessionBatch {
                    session_id: request.session_id,
                    player_id: request.player_id,
                    packets: vec![crate::net::session::wire::make_u_packet_bytes(
                        "GU",
                        &window.payload(),
                    )],
                }],
                saves: Vec::new(),
                broadcasts: Vec::new(),
            }
        }
        crate::game::PersistenceCompletion::AdminMoneyAllApplied { request, result } => {
            let mut effects = CommandEffects::default();
            match result {
                crate::game::AdminMoneyAllResult::Applied { affected_players } => {
                    for target_id in state.active_player_ids() {
                        let update = state
                            .modify_player(target_id, |ecs, entity| {
                                let session_id = ecs
                                    .get::<crate::game::player::PlayerConnection>(entity)?
                                    .session_id;
                                let mut player_stats =
                                    ecs.get_mut::<crate::game::player::PlayerStats>(entity)?;
                                player_stats.money =
                                    player_stats.money.saturating_add(request.amount);
                                let money = player_stats.money;
                                let creds = player_stats.creds;
                                ecs.get_mut::<crate::game::player::PlayerFlags>(entity)?
                                    .dirty = true;
                                Some((session_id, money, creds))
                            })
                            .flatten();
                        if let Some((session_id, money, creds)) = update {
                            let packet = crate::protocol::packets::money(money, creds);
                            effects
                                .broadcasts
                                .push(crate::game::BroadcastEffect::Direct {
                                    session_id,
                                    data: crate::net::session::wire::make_u_packet_bytes(
                                        packet.0, &packet.1,
                                    ),
                                });
                        }
                    }
                    if state.sessions.session_for_player(request.player_id)
                        == Some(request.session_id)
                    {
                        let packet = crate::protocol::packets::ok_message(
                            "Банк",
                            &format!(
                                "Выдано $ {} всем игрокам ({affected_players})",
                                request.amount
                            ),
                        );
                        effects.events.push(crate::game::GameEvent::SessionBatch {
                            session_id: request.session_id,
                            player_id: request.player_id,
                            packets: vec![crate::net::session::wire::make_u_packet_bytes(
                                packet.0, &packet.1,
                            )],
                        });
                    }
                }
                crate::game::AdminMoneyAllResult::PermanentFailure { message } => {
                    tracing::error!(player_id = %request.player_id, amount = request.amount, error = %message, "Admin moneyall persistence failed");
                    if state.sessions.session_for_player(request.player_id)
                        == Some(request.session_id)
                    {
                        let packet = crate::protocol::packets::ok_message(
                            "Ошибка",
                            "Не удалось выдать деньги всем игрокам",
                        );
                        effects.events.push(crate::game::GameEvent::SessionBatch {
                            session_id: request.session_id,
                            player_id: request.player_id,
                            packets: vec![crate::net::session::wire::make_u_packet_bytes(
                                packet.0, &packet.1,
                            )],
                        });
                    }
                }
            }
            effects
        }
        crate::game::PersistenceCompletion::AdminRoleApplied { request, result } => {
            let (title, message) = match result {
                crate::game::AdminRoleResult::Applied {
                    target_id,
                    target_name,
                } => {
                    state.modify_player(target_id, |ecs, entity| {
                        if let Some(mut stats) =
                            ecs.get_mut::<crate::game::player::PlayerStats>(entity)
                        {
                            stats.role = request.role as i32;
                        }
                    });
                    let role_name = match request.role {
                        crate::db::Role::Admin => "Admin",
                        crate::db::Role::Moderator => "Mod",
                        crate::db::Role::Player => "Player",
                    };
                    (
                        "Роль",
                        format!("Игроку {target_name} установлена роль {role_name}"),
                    )
                }
                crate::game::AdminRoleResult::TargetNotFound { target_name } => {
                    ("Ошибка", format!("Игрок '{target_name}' не найден"))
                }
                crate::game::AdminRoleResult::PermanentFailure { message } => {
                    tracing::error!(player_id = %request.player_id, error = %message, "Admin role persistence failed");
                    ("Ошибка", message)
                }
            };
            if state.sessions.session_for_player(request.player_id) == Some(request.session_id) {
                KernelContext::new(state).slash_ok_effect(
                    request.session_id,
                    request.player_id,
                    title,
                    &message,
                )
            } else {
                CommandEffects::default()
            }
        }
        crate::game::PersistenceCompletion::AdminSkillApplied { request, result } => {
            let mut effects = CommandEffects::default();
            match result {
                crate::game::AdminSkillResult::Saved => {
                    if request.target_session_id.is_some_and(|session_id| {
                        state.sessions.session_for_player(request.target_id) == Some(session_id)
                    }) {
                        effects.events.push(crate::game::GameEvent::SessionBatch {
                            session_id: request.target_session_id.expect("checked target session"),
                            player_id: request.target_id,
                            packets: request.packets,
                        });
                    }
                    if state.sessions.session_for_player(request.player_id)
                        == Some(request.session_id)
                    {
                        let packet = crate::protocol::packets::ok_message(
                            "Скилл",
                            &format!(
                                "{}: {} level {} slot {} exp {}",
                                request.target_name,
                                request.skill_code,
                                request.level,
                                request.slot,
                                request.exp
                            ),
                        );
                        effects.events.push(crate::game::GameEvent::SessionBatch {
                            session_id: request.session_id,
                            player_id: request.player_id,
                            packets: vec![crate::net::session::wire::make_u_packet_bytes(
                                packet.0, &packet.1,
                            )],
                        });
                    }
                }
                crate::game::AdminSkillResult::PermanentFailure { message } => {
                    tracing::error!(player_id = %request.player_id, target_id = %request.target_id, error = %message, "Admin skill persistence failed");
                    if state.sessions.session_for_player(request.player_id)
                        == Some(request.session_id)
                    {
                        let packet = crate::protocol::packets::ok_message(
                            "Скилл",
                            "Скилл изменён в текущей сессии, но не сохранён в БД.",
                        );
                        effects.events.push(crate::game::GameEvent::SessionBatch {
                            session_id: request.session_id,
                            player_id: request.player_id,
                            packets: vec![crate::net::session::wire::make_u_packet_bytes(
                                packet.0, &packet.1,
                            )],
                        });
                    }
                }
            }
            effects
        }
        crate::game::PersistenceCompletion::ClanCommandApplied { request, result } => {
            super::completion_clan::apply_clan_command_completion(state, &request, result)
        }
    }
}

pub(super) fn apply_remove_pack(
    state: &Arc<GameState>,
    remove: crate::game::RemovePack,
    sequence: crate::game::CommandSeq,
) -> CommandEffects {
    match crate::game::logic::building_delete::admit(state, remove, sequence.into()) {
        Ok(request) => CommandEffects {
            events: Vec::new(),
            saves: vec![crate::game::SaveCommand::BuildingDelete { request }],
            broadcasts: Vec::new(),
        },
        Err(error) => building_delete_error_effects(remove.cause.origin(), error),
    }
}

fn adapt_building_delete_completion(
    completion: crate::game::logic::building_delete::BuildingDeleteCompletion,
) -> CommandEffects {
    use crate::game::logic::building_delete::BuildingDeleteCompletion;

    match completion {
        BuildingDeleteCompletion::Applied(mut applied) => {
            let mut broadcasts = applied
                .changed_cells
                .into_iter()
                .map(crate::game::BroadcastEffect::CellUpdate)
                .collect::<Vec<_>>();
            broadcasts.push(crate::game::BroadcastEffect::BlockUpdate(
                (applied.view.x, applied.view.y).into(),
            ));
            let close = crate::protocol::packets::gu_close();
            broadcasts.extend(applied.closed_sessions.into_iter().map(|session_id| {
                crate::game::BroadcastEffect::Direct {
                    session_id,
                    data: crate::net::session::wire::make_u_packet_bytes(close.0, &close.1),
                }
            }));
            if let Some(position) = applied.box_position {
                broadcasts.push(crate::game::BroadcastEffect::CellUpdate(position));
            }
            if let Some(mut inventory_drop) = applied.inventory_drop.take()
                && let Some(session_id) = inventory_drop.session_id
            {
                let bubble = crate::protocol::packets::hb_chat(
                    0,
                    crate::net::session::util::net_u16_nonneg(inventory_drop.position.0),
                    crate::net::session::util::net_u16_nonneg(inventory_drop.position.1),
                    "ШПАААК ВЫПАЛ",
                );
                broadcasts.push(crate::game::BroadcastEffect::Direct {
                    session_id,
                    data: crate::net::session::wire::encode_hb_bundle(
                        &crate::protocol::packets::hb_bundle(&[bubble]).1,
                    ),
                });
                let packet =
                    crate::game::logic::inventory::inventory_packet(&mut inventory_drop.inventory);
                broadcasts.push(crate::game::BroadcastEffect::Direct {
                    session_id,
                    data: crate::net::session::wire::make_u_packet_bytes(packet.0, &packet.1),
                });
            }
            CommandEffects {
                events: Vec::new(),
                saves: Vec::new(),
                broadcasts,
            }
        }
        BuildingDeleteCompletion::Rejected { origin, error } => {
            building_delete_error_effects(origin, error)
        }
        BuildingDeleteCompletion::Stale => {
            tracing::error!("Stale building-delete completion ignored");
            CommandEffects::default()
        }
    }
}

fn building_delete_error_effects(
    origin: Option<crate::game::BuildingDeleteOrigin>,
    error: crate::game::logic::building_delete::BuildingDeleteError,
) -> CommandEffects {
    let Some(origin) = origin else {
        tracing::error!(?error, "Building delete rejected without an origin session");
        return CommandEffects::default();
    };
    let packet = crate::protocol::packets::ok_message("Ошибка", error.message());
    CommandEffects {
        events: Vec::new(),
        saves: Vec::new(),
        broadcasts: vec![crate::game::BroadcastEffect::Direct {
            session_id: origin.session_id,
            data: crate::net::session::wire::make_u_packet_bytes(packet.0, &packet.1),
        }],
    }
}

pub(super) fn apply_building_completion(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    command: PlayerCommand,
) -> CommandEffects {
    let effects = CommandEffects::default();
    match command {
        crate::game::PlayerCommand::ApplyInventoryBuildingPlaced { placement, db_id } => {
            let Some(tx) = state.sessions.outbox_for_session(session_id) else {
                return effects;
            };
            crate::game::logic::heal_inventory::apply_inventory_building_placed(
                state, &tx, &placement, db_id,
            );
        }
        crate::game::PlayerCommand::ApplyPaidBuildingPlaced { placement, db_id } => {
            let Some(tx) = state.sessions.outbox_for_session(session_id) else {
                return effects;
            };
            crate::game::logic::buildings::apply_paid_building_placed(
                state, &tx, &placement, db_id,
            );
        }
        crate::game::PlayerCommand::RefundPaidBuildingPlacement { cost } => {
            let Some(tx) = state.sessions.outbox_for_session(session_id) else {
                return effects;
            };
            crate::game::logic::buildings::refund_paid_building_placement(
                state, &tx, player_id, cost,
            );
        }
        _ => unreachable!("non-building command routed to building completion handler"),
    }
    effects
}

pub(super) fn apply_program_editor_completion(
    state: &Arc<GameState>,
    session_id: crate::game::SessionId,
    player_id: crate::game::PlayerId,
    command: PlayerCommand,
) {
    match command {
        crate::game::PlayerCommand::ApplyProgramEditorOpen {
            program_id,
            program_name,
            source,
        } => {
            let Some(tx) = state.sessions.outbox_for_session(session_id) else {
                return;
            };
            crate::net::session::ui::programmer::apply_editor_open(
                state,
                &tx,
                player_id,
                program_id,
                &program_name,
                &source,
            );
        }
        crate::game::PlayerCommand::ApplyProgramEditorRename {
            program_id,
            program_name,
            source,
        } => {
            let Some(tx) = state.sessions.outbox_for_session(session_id) else {
                return;
            };
            crate::net::session::ui::programmer::apply_editor_rename(
                state,
                &tx,
                player_id,
                program_id,
                &program_name,
                &source,
            );
        }
        _ => unreachable!("non-editor command routed to editor completion handler"),
    }
}
