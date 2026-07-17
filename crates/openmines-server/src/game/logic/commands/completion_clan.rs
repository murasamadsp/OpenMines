use super::{Arc, CommandEffects, GameState, KernelContext};

type ClanRequest = crate::game::ClanCommandRequest;
type ClanResult = crate::game::ClanCommandResult;

pub(super) fn apply_clan_command_completion(
    state: &Arc<GameState>,
    request: &ClanRequest,
    result: ClanResult,
) -> CommandEffects {
    let context = KernelContext::new(state);
    let mut effects = match result {
        ClanResult::Created { clan_id } => apply_created(state, &context, request, clan_id),
        ClanResult::Left { clan_id, disbanded } => {
            apply_left(state, &context, request, clan_id, disbanded)
        }
        ClanResult::Kicked { clan_id, target_id } => {
            apply_kicked(&context, request, clan_id, target_id)
        }
        ClanResult::Joined { clan_id } => apply_joined(state, &context, request, clan_id),
        ClanResult::InviteDeclined => {
            menu_effect(request, None, crate::game::ClanMenuAction::Invites)
        }
        ClanResult::RequestAccepted { clan_id, target_id } => {
            apply_request_accepted(&context, request, clan_id, target_id)
        }
        ClanResult::RequestDeclined => apply_request_declined(state, &context, request),
        ClanResult::Promoted { clan_id, target_id } => {
            apply_promoted(&context, request, clan_id, target_id)
        }
        ClanResult::Invited { target_id } => apply_invited(state, &context, request, target_id),
        ClanResult::RequestSent => reply_effect(&context, "Клан", "Заявка отправлена"),
        ClanResult::Rejected { title, message } => {
            if request.create_reserved {
                context.refund_clan_create(request.player_id);
            }
            reply_effect(&context, &title, &message)
        }
        ClanResult::PermanentFailure { message } => {
            tracing::error!(player_id = %request.player_id, error = %message, "Clan command persistence failed permanently");
            if request.create_reserved {
                context.refund_clan_create(request.player_id);
            }
            reply_effect(&context, "Ошибка", "Ошибка БД")
        }
    };
    if state.sessions.session_for_player(request.player_id) == Some(request.session_id) {
        for event in &mut effects.events {
            if let crate::game::GameEvent::SessionBatch {
                session_id,
                player_id,
                ..
            } = event
                && *session_id == crate::game::SessionId::new(0)
            {
                *session_id = request.session_id;
                *player_id = request.player_id;
            }
        }
    } else {
        effects.events.retain(|event| {
            !matches!(event, crate::game::GameEvent::SessionBatch { session_id, .. } if *session_id == crate::game::SessionId::new(0))
        });
    }
    effects
}

fn apply_created(
    state: &GameState,
    context: &KernelContext<'_>,
    request: &ClanRequest,
    clan_id: i32,
) -> CommandEffects {
    let money = context
        .modify_player(request.player_id, |ecs, entity| {
            let mut player_stats = ecs.get_mut::<crate::game::player::PlayerStats>(entity)?;
            player_stats.clan_id = Some(clan_id);
            player_stats.clan_rank = crate::db::ClanRank::Leader as i32;
            Some((player_stats.money, player_stats.creds))
        })
        .flatten();
    let Some((money, creds)) = money else {
        tracing::error!(player_id = %request.player_id, clan_id, "Clan created in DB but player state is unavailable");
        return CommandEffects::default();
    };
    let money_packet = crate::protocol::packets::money(money, creds);
    let clan_packet = crate::protocol::packets::clan_show(clan_id);
    let mut effects = reply_effect(context, "Клан", "Клан успешно создан!");
    effects.events.push(crate::game::GameEvent::SessionBatch {
        session_id: request.session_id,
        player_id: request.player_id,
        packets: vec![
            crate::net::session::wire::make_u_packet_bytes(money_packet.0, &money_packet.1),
            crate::net::session::wire::make_u_packet_bytes(clan_packet.0, &clan_packet.1),
        ],
    });
    if state.sessions.session_for_player(request.player_id) != Some(request.session_id) {
        effects.events.clear();
    }
    effects
}

fn apply_left(
    state: &GameState,
    context: &KernelContext<'_>,
    request: &ClanRequest,
    clan_id: i32,
    disbanded: bool,
) -> CommandEffects {
    let target_ids = if disbanded {
        state.active_player_ids()
    } else {
        vec![request.player_id]
    };
    let mut effects = reply_effect(
        context,
        "Клан",
        if disbanded {
            "Клан расформирован"
        } else {
            "Вы покинули клан"
        },
    );
    for target_id in target_ids {
        let cleared = context
            .modify_player(target_id, |ecs, entity| {
                let mut player_stats = ecs.get_mut::<crate::game::player::PlayerStats>(entity)?;
                if player_stats.clan_id != Some(clan_id) {
                    return None;
                }
                player_stats.clan_id = None;
                player_stats.clan_rank = crate::db::ClanRank::None as i32;
                ecs.get_mut::<crate::game::player::PlayerFlags>(entity)?
                    .dirty = true;
                ecs.get::<crate::game::player::PlayerConnection>(entity)
                    .map(|connection| connection.session_id)
            })
            .flatten();
        if let Some(session_id) = cleared {
            push_clan_hidden(&mut effects, session_id, target_id);
        }
    }
    effects
}

fn apply_kicked(
    context: &KernelContext<'_>,
    request: &ClanRequest,
    clan_id: i32,
    target_id: crate::game::PlayerId,
) -> CommandEffects {
    let cleared = context
        .modify_player(target_id, |ecs, entity| {
            let mut player_stats = ecs.get_mut::<crate::game::player::PlayerStats>(entity)?;
            if player_stats.clan_id != Some(clan_id) {
                return None;
            }
            player_stats.clan_id = None;
            player_stats.clan_rank = crate::db::ClanRank::None as i32;
            ecs.get_mut::<crate::game::player::PlayerFlags>(entity)?
                .dirty = true;
            ecs.get::<crate::game::player::PlayerConnection>(entity)
                .map(|connection| connection.session_id)
        })
        .flatten();
    let mut effects = reply_effect(context, "Клан", "Игрок исключён из клана");
    if let Some(session_id) = cleared {
        push_clan_hidden(&mut effects, session_id, target_id);
    }
    let _ = request;
    effects
}

fn apply_joined(
    state: &GameState,
    context: &KernelContext<'_>,
    request: &ClanRequest,
    clan_id: i32,
) -> CommandEffects {
    let joined = context
        .modify_player(request.player_id, |ecs, entity| {
            let mut player_stats = ecs.get_mut::<crate::game::player::PlayerStats>(entity)?;
            player_stats.clan_id = Some(clan_id);
            player_stats.clan_rank = crate::db::ClanRank::Member as i32;
            ecs.get_mut::<crate::game::player::PlayerFlags>(entity)?
                .dirty = true;
            Some(())
        })
        .flatten()
        .is_some();
    if !joined || state.sessions.session_for_player(request.player_id) != Some(request.session_id) {
        return CommandEffects::default();
    }
    let shown = crate::protocol::packets::clan_show(clan_id);
    let mut effects = reply_effect(context, "Клан", "Вы вступили в клан!");
    effects.events.push(crate::game::GameEvent::SessionBatch {
        session_id: request.session_id,
        player_id: request.player_id,
        packets: vec![crate::net::session::wire::make_u_packet_bytes(
            shown.0, &shown.1,
        )],
    });
    effects
}

fn apply_request_accepted(
    context: &KernelContext<'_>,
    request: &ClanRequest,
    clan_id: i32,
    target_id: crate::game::PlayerId,
) -> CommandEffects {
    if let Some(session_id) = context
        .modify_player(target_id, |ecs, entity| {
            let mut player_stats = ecs.get_mut::<crate::game::player::PlayerStats>(entity)?;
            player_stats.clan_id = Some(clan_id);
            player_stats.clan_rank = crate::db::ClanRank::Member as i32;
            ecs.get_mut::<crate::game::player::PlayerFlags>(entity)?
                .dirty = true;
            ecs.get::<crate::game::player::PlayerConnection>(entity)
                .map(|connection| connection.session_id)
        })
        .flatten()
    {
        let shown = crate::protocol::packets::clan_show(clan_id);
        let mut effects = menu_effect(
            request,
            Some(clan_id),
            crate::game::ClanMenuAction::Requests,
        );
        effects.events.push(crate::game::GameEvent::SessionBatch {
            session_id,
            player_id: target_id,
            packets: vec![crate::net::session::wire::make_u_packet_bytes(
                shown.0, &shown.1,
            )],
        });
        return effects;
    }
    menu_effect(
        request,
        Some(clan_id),
        crate::game::ClanMenuAction::Requests,
    )
}

fn apply_request_declined(
    state: &GameState,
    _context: &KernelContext<'_>,
    request: &ClanRequest,
) -> CommandEffects {
    let clan_id = state.query_player_opt(request.player_id, |ecs, entity| {
        ecs.get::<crate::game::player::PlayerStats>(entity)
            .and_then(|player_stats| player_stats.clan_id)
    });
    menu_effect(request, clan_id, crate::game::ClanMenuAction::Requests)
}

fn apply_promoted(
    context: &KernelContext<'_>,
    request: &ClanRequest,
    clan_id: i32,
    target_id: crate::game::PlayerId,
) -> CommandEffects {
    context.modify_player(target_id, |ecs, entity| {
        let mut player_stats = ecs.get_mut::<crate::game::player::PlayerStats>(entity)?;
        if player_stats.clan_id != Some(clan_id) {
            return None;
        }
        player_stats.clan_rank = crate::db::ClanRank::Officer as i32;
        ecs.get_mut::<crate::game::player::PlayerFlags>(entity)?
            .dirty = true;
        Some(())
    });
    menu_effect(request, Some(clan_id), crate::game::ClanMenuAction::Members)
}

fn apply_invited(
    state: &GameState,
    context: &KernelContext<'_>,
    request: &ClanRequest,
    target_id: crate::game::PlayerId,
) -> CommandEffects {
    let target_session = state.query_player_opt(target_id, |ecs, entity| {
        ecs.get::<crate::game::player::PlayerConnection>(entity)
            .map(|connection| connection.session_id)
    });
    let mut effects = reply_effect(context, "Клан", "Приглашение отправлено");
    if let Some(session_id) = target_session
        && state.sessions.session_for_player(target_id) == Some(session_id)
    {
        let packet = crate::protocol::packets::ok_message("Клан", "Вас пригласили в клан!");
        effects.events.push(crate::game::GameEvent::SessionBatch {
            session_id,
            player_id: target_id,
            packets: vec![crate::net::session::wire::make_u_packet_bytes(
                packet.0, &packet.1,
            )],
        });
    }
    let _ = request;
    effects
}

fn menu_effect(
    request: &ClanRequest,
    player_clan_id: Option<i32>,
    action: crate::game::ClanMenuAction,
) -> CommandEffects {
    CommandEffects {
        events: Vec::new(),
        saves: vec![crate::game::SaveCommand::ClanMenu {
            request: crate::game::ClanMenuRequest {
                player_id: request.player_id,
                session_id: request.session_id,
                player_clan_id,
                action,
                invite_candidates: Vec::new(),
            },
        }],
        broadcasts: Vec::new(),
    }
}

fn reply_effect(context: &KernelContext, title: &str, message: &str) -> CommandEffects {
    context.slash_ok_effect(
        crate::game::SessionId::new(0),
        crate::game::PlayerId::from(0),
        title,
        message,
    )
}

fn push_clan_hidden(
    effects: &mut CommandEffects,
    session_id: crate::game::SessionId,
    player_id: crate::game::PlayerId,
) {
    let hidden = crate::protocol::packets::clan_hide();
    effects.events.push(crate::game::GameEvent::SessionBatch {
        session_id,
        player_id,
        packets: vec![crate::net::session::wire::make_u_packet_bytes(
            hidden.0, &hidden.1,
        )],
    });
}
