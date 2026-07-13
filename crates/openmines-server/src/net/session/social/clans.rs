//! Меню и действия клана.
#![allow(clippy::uninlined_format_args)]
use crate::net::session::prelude::*;
use crate::net::session::ui::horb::{Button, Horb};

// ─── Clans ─────────────────────────────────────────────────────────────

fn send_clan_state_error(tx: &Outbox) {
    send_clan_ok(tx, "КЛАН", "Состояние игрока недоступно.");
}

fn online_player_state_ready(state: &Arc<GameState>, pid: PlayerId) -> bool {
    if state.get_player_entity(pid).is_none() {
        return true;
    }
    state
        .query_player(pid, |ecs, entity| {
            ecs.get::<crate::game::PlayerStats>(entity).is_some()
                && ecs.get::<crate::game::PlayerFlags>(entity).is_some()
        })
        .unwrap_or(false)
}

fn ensure_online_player_state_ready(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId) -> bool {
    if online_player_state_ready(state, pid) {
        true
    } else {
        send_clan_state_error(tx);
        false
    }
}

async fn load_clan_members_or_error(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    clan_id: i32,
    context: &str,
) -> Option<Vec<(i32, String, i32)>> {
    match state.db.get_clan_members(clan_id).await {
        Ok(members) => Some(members),
        Err(e) => {
            tracing::error!(player_id = %pid, clan_id, error = ?e, "{context}");
            send_clan_ok(tx, "Ошибка", "Ошибка БД");
            None
        }
    }
}

fn clan_rank_for(members: &[(i32, String, i32)], pid: PlayerId) -> crate::db::ClanRank {
    members
        .iter()
        .find(|(id, _, _)| *id == pid)
        .map(|(_, _, r)| crate::db::ClanRank::from_db(*r))
        .unwrap_or(crate::db::ClanRank::None)
}

pub async fn handle_clan_menu(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId) {
    let clan_id = player_clan_id(state, pid);
    if let Some(cid) = clan_id {
        handle_clan_info_view(state, tx, pid, cid).await;
    } else {
        let invites = match state.db.get_player_invites(pid.into()).await {
            Ok(invites) => invites,
            Err(e) => {
                tracing::error!(player_id = %pid, error = ?e, "Failed to load clan invites");
                send_clan_ok(tx, "Ошибка", "Ошибка БД");
                return;
            }
        };
        let mut win = Horb::new("КЛАНЫ").text("Выберите клан или создайте свой");

        if !invites.is_empty() {
            win = win.button(Button::new(
                format!("Приглашения ({})", invites.len()),
                "clan_invites_view",
            ));
        }

        win = win.button(Button::new("Создать клан (1000 кр.)", "clan_create"));
        let clans = match state.db.list_clans().await {
            Ok(clans) => clans,
            Err(e) => {
                tracing::error!(player_id = %pid, error = ?e, "Failed to load clans");
                send_clan_ok(tx, "Ошибка", "Ошибка БД");
                return;
            }
        };
        for clan in &clans {
            let members = match state.db.get_clan_members(clan.id).await {
                Ok(members) => members,
                Err(e) => {
                    tracing::error!(player_id = %pid, clan_id = clan.id, error = ?e, "Failed to load clan members");
                    send_clan_ok(tx, "Ошибка", "Ошибка БД");
                    return;
                }
            };
            let label = format!("{} [{}] ({} чел.)", clan.name, clan.abr, members.len());
            win = win.button(Button::new(label, format!("clan_view:{}", clan.id)));
        }
        win.close_button().send(state, tx, pid, "clan");
    }
}

pub async fn handle_clan_info_view(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    clan_id: i32,
) {
    let clan = match state.db.get_clan(clan_id).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            send_clan_ok(tx, "Ошибка", "Клан не найден");
            return;
        }
        Err(e) => {
            tracing::error!(player_id = %pid, clan_id, error = ?e, "Failed to load clan");
            send_clan_ok(tx, "Ошибка", "Ошибка БД");
            return;
        }
    };
    let Some(members) =
        load_clan_members_or_error(state, tx, pid, clan_id, "Failed to load clan members").await
    else {
        return;
    };
    let player_rank = clan_rank_for(&members, pid);

    let owner_name = members
        .iter()
        .find(|(id, _, _)| *id == clan.owner_id)
        .map(|(_, n, _)| n.as_str())
        .unwrap_or("?");

    let text = format!(
        "Клан: {} [{}]\nУчастники: {}\nВладелец: {}\nВаш ранг: {:?}",
        clan.name,
        clan.abr,
        members.len(),
        owner_name,
        player_rank
    );
    let mut win = Horb::new(clan.name.clone())
        .text(text)
        .button(Button::new("Участники", "clan_members"));

    if player_rank >= crate::db::ClanRank::Officer {
        let requests = match state.db.get_clan_requests(clan_id).await {
            Ok(requests) => requests,
            Err(e) => {
                tracing::error!(player_id = %pid, clan_id, error = ?e, "Failed to load clan requests");
                send_clan_ok(tx, "Ошибка", "Ошибка БД");
                return;
            }
        };
        win = win
            .button(Button::new(
                format!("Заявки ({})", requests.len()),
                "clan_requests",
            ))
            .button(Button::new("Пригласить игрока", "clan_invite_list"));
    }

    if !(clan.owner_id == pid && members.len() > 1) {
        win = win.button(Button::new("Покинуть клан", "clan_leave"));
    }

    win.close_button().send(state, tx, pid, "clan");
}

pub async fn handle_clan_preview(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, clan_id: i32) {
    let clan = match state.db.get_clan(clan_id).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            send_clan_ok(tx, "Ошибка", "Клан не найден");
            return;
        }
        Err(e) => {
            tracing::error!(player_id = %pid, clan_id, error = ?e, "Failed to load clan");
            send_clan_ok(tx, "Ошибка", "Ошибка БД");
            return;
        }
    };
    let Some(members) =
        load_clan_members_or_error(state, tx, pid, clan_id, "Failed to load clan members").await
    else {
        return;
    };
    let owner_name = members
        .iter()
        .find(|(id, _, _)| *id == clan.owner_id)
        .map(|(_, n, _)| n.as_str())
        .unwrap_or("?");

    let text = format!(
        "Клан: {} [{}]\nУчастники: {}\nВладелец: {}",
        clan.name,
        clan.abr,
        members.len(),
        owner_name
    );
    let mut win = Horb::new(clan.name.clone()).text(text);

    if player_clan_id(state, pid).is_none() {
        win = win.button(Button::new(
            "Подать заявку",
            format!("clan_request:{}", clan_id),
        ));
    }

    win.button(Button::new("Назад", "clan_back"))
        .close_button()
        .send(state, tx, pid, "clan");
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DebitResult {
    Ok { creds: i64, money: i64 },
    InsufficientFunds,
    StateError,
}

pub async fn handle_clan_create(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    name: &str,
    tag: &str,
) {
    if player_clan_id(state, pid).is_some() {
        send_clan_ok(tx, "Ошибка", "Вы уже в клане");
        return;
    }

    let debited = state
        .modify_player(pid, |ecs, entity| {
            if ecs.get::<crate::game::PlayerStats>(entity).is_none()
                || ecs.get::<crate::game::PlayerFlags>(entity).is_none()
            {
                return Some(DebitResult::StateError);
            }
            let (creds, money, ok) = {
                let mut s = ecs
                    .get_mut::<crate::game::PlayerStats>(entity)
                    .expect("PlayerStats checked before clan create mutation");
                if s.creds < 1000 {
                    (0, 0, false)
                } else {
                    s.creds -= 1000;
                    (s.creds, s.money, true)
                }
            };
            if ok {
                let mut flags = ecs
                    .get_mut::<crate::game::PlayerFlags>(entity)
                    .expect("PlayerFlags checked before clan create mutation");
                flags.dirty = true;
                Some(DebitResult::Ok { creds, money })
            } else {
                Some(DebitResult::InsufficientFunds)
            }
        })
        .flatten()
        .unwrap_or(DebitResult::StateError);

    let (new_creds, p_money) = match debited {
        DebitResult::Ok { creds, money } => (creds, money),
        DebitResult::InsufficientFunds => {
            send_clan_ok(tx, "Ошибка", "Недостаточно кредитов (нужно 1000)");
            return;
        }
        DebitResult::StateError => {
            send_clan_state_error(tx);
            return;
        }
    };

    let new_id = match state.db.pick_clan_id().await {
        Ok(Some(id)) => id,
        Ok(None) => {
            refund_clan_credits(state, pid);
            send_clan_ok(tx, "Ошибка", "Достигнут лимит кланов (218)");
            return;
        }
        Err(e) => {
            refund_clan_credits(state, pid);
            tracing::error!(error = ?e, "Failed to pick clan ID");
            return;
        }
    };

    match state.db.create_clan(new_id, name, tag, pid.into()).await {
        Ok(()) => {
            let applied = state
                .modify_player(pid, |ecs, entity| {
                    if ecs.get::<crate::game::PlayerStats>(entity).is_none() {
                        return false;
                    }
                    let mut s = ecs.get_mut::<crate::game::PlayerStats>(entity).unwrap();
                    s.clan_id = Some(new_id);
                    s.clan_rank = crate::db::ClanRank::Leader as i32;
                    true
                })
                .unwrap_or(false);
            if !applied {
                tracing::error!(
                    player_id = %pid,
                    clan_id = new_id,
                    "Clan created in DB but online player state could not be updated"
                );
                return;
            }
            send_u_packet(tx, "P$", &money(p_money, new_creds).1);
            let cs = clan_show(new_id);
            send_u_packet(tx, cs.0, &cs.1);
            send_clan_ok(tx, "Клан", "Клан успешно создан!");
            handle_clan_info_view(state, tx, pid, new_id).await;
        }
        Err(e) => {
            refund_clan_credits(state, pid);
            tracing::error!(error = ?e, "Failed to create clan");
            send_clan_ok(
                tx,
                "Ошибка",
                "Не удалось создать клан (возможно, имя или тег заняты)",
            );
        }
    }
}

fn refund_clan_credits(state: &Arc<GameState>, pid: PlayerId) {
    state.modify_player(pid, |ecs, entity| {
        if let Some(mut s) = ecs.get_mut::<crate::game::PlayerStats>(entity) {
            s.creds += 1000;
        }
        if let Some(mut flags) = ecs.get_mut::<crate::game::PlayerFlags>(entity) {
            flags.dirty = true;
        }
        Some(())
    });
}

pub async fn handle_clan_leave(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId) {
    let clan_id = match player_clan_id(state, pid) {
        Some(id) => id,
        None => return,
    };

    if is_clan_owner(state, clan_id, pid).await {
        let Some(members) = load_clan_members_or_error(
            state,
            tx,
            pid,
            clan_id,
            "Failed to load clan members before delete",
        )
        .await
        else {
            return;
        };
        if members.len() > 1 {
            send_clan_ok(
                tx,
                "Ошибка",
                "Сначала передайте лидерство или исключите участников",
            );
            return;
        }
        if members
            .iter()
            .any(|(member_pid, _, _)| !online_player_state_ready(state, PlayerId(*member_pid)))
        {
            send_clan_state_error(tx);
            return;
        }
        match state.db.delete_clan(clan_id).await {
            Ok(()) => {
                for target_pid in state.active_player_ids() {
                    state.modify_player(target_pid, |ecs, entity| {
                        let mut s = ecs.get_mut::<crate::game::PlayerStats>(entity)?;
                        if s.clan_id == Some(clan_id) {
                            s.clan_id = None;
                            s.clan_rank = 0;
                            let mut flags = ecs.get_mut::<crate::game::PlayerFlags>(entity)?;
                            flags.dirty = true;
                            let ch = clan_hide();
                            state.send_to_player(target_pid, make_u_packet_bytes(ch.0, &ch.1));
                        }
                        Some(())
                    });
                }
                // Broadcast owner's HB with clan=0 (1:1 C# LeaveClan → SendMyMove).
                let _ = state.query_player_opt(pid, |ecs, entity| {
                    let pos = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
                    let pstats = ecs.get::<crate::game::player::PlayerStats>(entity)?;
                    let tail: u8 = ecs
                        .get::<crate::game::programmator::ProgrammatorState>(entity)
                        .map_or(0, |p| u8::from(p.running));
                    let bot = hb_bot(
                        net_u16_nonneg(pid),
                        net_u16_nonneg(pos.x),
                        net_u16_nonneg(pos.y),
                        net_u8_clamped(pos.dir, 3),
                        net_u8_clamped(pstats.skin, 255),
                        0,
                        tail,
                    );
                    let cx = pos.x.div_euclid(32) as u32;
                    let cy = pos.y.div_euclid(32) as u32;
                    let hb_data = encode_hb_bundle(&hb_bundle(&[bot]).1);
                    state.broadcast_to_nearby(cx, cy, &hb_data, Some(pid));
                    Some(())
                });
                send_clan_ok(tx, "Клан", "Клан расформирован");
                handle_clan_menu(state, tx, pid).await;
            }
            Err(e) => {
                tracing::error!(clan_id, error = ?e, "Failed to delete clan");
                send_clan_ok(tx, "Ошибка", "Не удалось покинуть клан");
            }
        }
    } else {
        if !ensure_online_player_state_ready(state, tx, pid) {
            return;
        }
        match state.db.leave_clan(pid.into()).await {
            Ok(()) => {
                state.modify_player(pid, |ecs, entity| {
                    let mut s = ecs.get_mut::<crate::game::PlayerStats>(entity)?;
                    s.clan_id = None;
                    s.clan_rank = 0;
                    let mut flags = ecs.get_mut::<crate::game::PlayerFlags>(entity)?;
                    flags.dirty = true;
                    Some(())
                });
                let ch = clan_hide();
                send_u_packet(tx, ch.0, &ch.1);
                // Broadcast HB with clan=0 so nearby players see the icon disappear (1:1 C# SendMyMove).
                let _ = state.query_player_opt(pid, |ecs, entity| {
                    let pos = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
                    let pstats = ecs.get::<crate::game::player::PlayerStats>(entity)?;
                    let tail: u8 = ecs
                        .get::<crate::game::programmator::ProgrammatorState>(entity)
                        .map_or(0, |p| u8::from(p.running));
                    let bot = hb_bot(
                        net_u16_nonneg(pid),
                        net_u16_nonneg(pos.x),
                        net_u16_nonneg(pos.y),
                        net_u8_clamped(pos.dir, 3),
                        net_u8_clamped(pstats.skin, 255),
                        0,
                        tail,
                    );
                    let cx = pos.x.div_euclid(32) as u32;
                    let cy = pos.y.div_euclid(32) as u32;
                    let hb_data = encode_hb_bundle(&hb_bundle(&[bot]).1);
                    state.broadcast_to_nearby(cx, cy, &hb_data, Some(pid));
                    Some(())
                });
                send_clan_ok(tx, "Клан", "Вы покинули клан");
                handle_clan_menu(state, tx, pid).await;
            }
            Err(e) => {
                tracing::error!(player_id = %pid, error = ?e, "Failed to leave clan");
                send_clan_ok(tx, "Ошибка", "Не удалось покинуть клан");
            }
        }
    }
}

pub async fn handle_clan_join_request(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    clan_id: i32,
) {
    if player_clan_id(state, pid).is_some() {
        send_clan_ok(tx, "Ошибка", "Вы уже в клане");
        return;
    }

    // Check for existing pending request (1:1 C# Clan.AddReq: reqs.FirstOrDefault(i => i.player.id == id)).
    let existing = match state.db.get_clan_requests(clan_id).await {
        Ok(existing) => existing,
        Err(e) => {
            tracing::error!(player_id = %pid, clan_id, error = ?e, "Failed to load clan requests");
            send_clan_ok(tx, "Ошибка", "Ошибка БД");
            return;
        }
    };
    if existing.iter().any(|(req_pid, _)| *req_pid == pid) {
        send_clan_ok(tx, "Клан", "Заявка уже подана");
        return;
    }

    match state.db.add_clan_request(clan_id, pid.into()).await {
        Ok(()) => {
            send_clan_ok(tx, "Клан", "Заявка отправлена");
        }
        Err(e) => {
            tracing::error!(clan_id, player_id = %pid, error = ?e, "Failed to add clan join request");
            send_clan_ok(tx, "Ошибка", "Не удалось отправить заявку");
        }
    }
}

pub async fn handle_clan_members_view(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId) {
    let clan_id = match player_clan_id(state, pid) {
        Some(id) => id,
        None => return,
    };
    let Some(members) =
        load_clan_members_or_error(state, tx, pid, clan_id, "Failed to load clan members").await
    else {
        return;
    };
    let player_rank = clan_rank_for(&members, pid);

    let mut win = Horb::new("Участники");
    let mut text = String::from("Участники клана:\n");

    for (m_pid, m_name, m_rank_raw) in &members {
        let m_rank = crate::db::ClanRank::from_db(*m_rank_raw);
        text.push_str(&format!("- {} ({:?})\n", m_name, m_rank));

        if *m_pid != pid {
            if player_rank == crate::db::ClanRank::Leader && m_rank == crate::db::ClanRank::Member {
                win = win.button(Button::new(
                    format!("Повысить {}", m_name),
                    format!("clan_promote:{}", m_pid),
                ));
            }
            if player_rank > m_rank {
                win = win.button(Button::new(
                    format!("Исключить {}", m_name),
                    format!("clan_kick_id:{}", m_pid),
                ));
            }
        }
    }

    win.text(text)
        .button(Button::new("Назад", "clan_back"))
        .close_button()
        .send(state, tx, pid, "clan");
}

pub async fn handle_clan_invite_list(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId) {
    let clan_id = match player_clan_id(state, pid) {
        Some(id) => id,
        None => return,
    };
    let Some(members) =
        load_clan_members_or_error(state, tx, pid, clan_id, "Failed to load clan members").await
    else {
        return;
    };
    let player_rank = clan_rank_for(&members, pid);

    if player_rank < crate::db::ClanRank::Officer {
        send_clan_no_rights(tx);
        return;
    }

    let mut win = Horb::new("Пригласить").text("Выберите игрока для приглашения в клан:");
    let mut count = 0;

    for target_pid in state.active_player_ids() {
        if target_pid == pid {
            continue;
        }

        let target_data = state.query_player_opt(target_pid, |ecs, entity| {
            let s = ecs.get::<crate::game::PlayerStats>(entity)?;
            let m = ecs.get::<crate::game::PlayerMetadata>(entity)?;
            if s.clan_id.is_none() {
                Some(m.name.clone())
            } else {
                None
            }
        });

        if let Some(name) = target_data {
            win = win.button(Button::new(
                format!("Пригласить {}", name),
                format!("clan_invite_send:{}", target_pid),
            ));
            count += 1;
        }
        if count >= 20 {
            break;
        }
    }

    if count == 0 {
        win = win.button(Button::new("Никого нет рядом без клана", "noop"));
    }

    win.button(Button::new("Назад", "clan_back"))
        .close_button()
        .send(state, tx, pid, "clan");
}

pub async fn handle_clan_invite_send(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    target_pid: i32,
) {
    let clan_id = match player_clan_id(state, pid) {
        Some(id) => id,
        None => return,
    };
    let Some(members) =
        load_clan_members_or_error(state, tx, pid, clan_id, "Failed to load clan members").await
    else {
        return;
    };
    let player_rank = clan_rank_for(&members, pid);

    if player_rank < crate::db::ClanRank::Officer {
        send_clan_no_rights(tx);
        return;
    }

    match state.db.add_clan_invite(clan_id, target_pid).await {
        Ok(()) => {
            send_clan_ok(tx, "Клан", "Приглашение отправлено");
            state.send_to_player(
                target_pid.into(),
                make_u_packet_bytes("OK", &ok_message("Клан", "Вас пригласили в клан!").1),
            );
        }
        Err(e) => {
            tracing::error!(clan_id, target_id = target_pid, error = ?e, "Failed to add clan invite");
            send_clan_ok(tx, "Ошибка", "Не удалось отправить приглашение");
        }
    }
}

pub async fn handle_clan_invites_view(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId) {
    let invites = match state.db.get_player_invites(pid.into()).await {
        Ok(invites) => invites,
        Err(e) => {
            tracing::error!(player_id = %pid, error = ?e, "Failed to load clan invites");
            send_clan_ok(tx, "Ошибка", "Ошибка БД");
            return;
        }
    };
    let mut win = Horb::new("Приглашения").text("Вас пригласили в следующие кланы:");

    for (clan_id, clan_name) in &invites {
        win = win
            .button(Button::new(
                format!("Принять {}", clan_name),
                format!("clan_invite_accept:{}", clan_id),
            ))
            .button(Button::new(
                format!("Отклонить {}", clan_name),
                format!("clan_invite_decline:{}", clan_id),
            ));
    }

    win.button(Button::new("Назад", "clan_back"))
        .close_button()
        .send(state, tx, pid, "clan");
}

pub async fn handle_clan_invite_accept(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    clan_id: i32,
) {
    if player_clan_id(state, pid).is_some() {
        send_clan_ok(tx, "Ошибка", "Вы уже в клане");
        return;
    }
    let can_update_player_state = state
        .query_player(pid, |ecs, entity| {
            ecs.get::<crate::game::PlayerStats>(entity).is_some()
                && ecs.get::<crate::game::PlayerFlags>(entity).is_some()
        })
        .unwrap_or(false);
    if !can_update_player_state {
        send_clan_state_error(tx);
        return;
    }

    match state.db.accept_clan_invite(clan_id, pid.into()).await {
        Ok(()) => {
            let applied = state
                .modify_player(pid, |ecs, entity| {
                    if ecs.get::<crate::game::PlayerStats>(entity).is_none()
                        || ecs.get::<crate::game::PlayerFlags>(entity).is_none()
                    {
                        send_clan_state_error(tx);
                        return false;
                    }
                    let mut s = ecs
                        .get_mut::<crate::game::PlayerStats>(entity)
                        .expect("PlayerStats checked before clan invite mutation");
                    s.clan_id = Some(clan_id);
                    s.clan_rank = crate::db::ClanRank::Member as i32;
                    let mut flags = ecs
                        .get_mut::<crate::game::PlayerFlags>(entity)
                        .expect("PlayerFlags checked before clan invite mutation");
                    flags.dirty = true;
                    true
                })
                .unwrap_or(false);
            if !applied {
                tracing::error!(
                    player_id = %pid,
                    clan_id,
                    "Clan invite accepted in DB but online player state could not be updated"
                );
                return;
            }
            // cS = номер иконки = id клана (клиент: ClanSprite.sprites[id-1]).
            let cs = clan_show(clan_id);
            send_u_packet(tx, cs.0, &cs.1);
            send_clan_ok(tx, "Клан", "Вы вступили в клан!");
        }
        Err(e) => {
            tracing::error!(clan_id, player_id = %pid, error = ?e, "Failed to accept clan invite");
            send_clan_ok(tx, "Ошибка", "Не удалось принять приглашение");
        }
    }
}

pub async fn handle_clan_invite_decline(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    clan_id: i32,
) {
    let _ = state.db.decline_clan_invite(clan_id, pid.into()).await;
    handle_clan_invites_view(state, tx, pid).await;
}

pub async fn handle_clan_promote(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    target_pid: i32,
) {
    let clan_id = match player_clan_id(state, pid) {
        Some(id) => id,
        None => return,
    };
    if !is_clan_owner(state, clan_id, pid).await {
        send_clan_no_rights(tx);
        return;
    }

    match state
        .db
        .set_clan_rank(target_pid, clan_id, crate::db::ClanRank::Officer)
        .await
    {
        Ok(()) => {
            state.modify_player(target_pid.into(), |ecs, entity| {
                let mut s = ecs.get_mut::<crate::game::PlayerStats>(entity)?;
                s.clan_rank = crate::db::ClanRank::Officer as i32;
                let mut flags = ecs.get_mut::<crate::game::PlayerFlags>(entity)?;
                flags.dirty = true;
                Some(())
            });
            send_clan_ok(tx, "Клан", "Игрок повышен до Офицера");
            handle_clan_members_view(state, tx, pid).await;
        }
        Err(e) => {
            tracing::error!(clan_id, player_id = target_pid, error = ?e, "Failed to promote player rank");
            send_clan_ok(tx, "Ошибка", "Не удалось повысить игрока");
        }
    }
}

pub async fn handle_clan_requests_view(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId) {
    let clan_id = match player_clan_id(state, pid) {
        Some(id) => id,
        None => return,
    };
    let Some(members) =
        load_clan_members_or_error(state, tx, pid, clan_id, "Failed to load clan members").await
    else {
        return;
    };
    let player_rank = clan_rank_for(&members, pid);

    if player_rank < crate::db::ClanRank::Officer {
        send_clan_no_rights(tx);
        return;
    }
    let requests = match state.db.get_clan_requests(clan_id).await {
        Ok(requests) => requests,
        Err(e) => {
            tracing::error!(player_id = %pid, clan_id, error = ?e, "Failed to load clan requests");
            send_clan_ok(tx, "Ошибка", "Ошибка БД");
            return;
        }
    };
    let mut win = Horb::new("Заявки").text("Заявки в клан:");
    for (req_pid, req_name) in &requests {
        win = win
            .button(Button::new(
                format!("{req_name} - Принять"),
                format!("clan_accept:{req_pid}"),
            ))
            .button(Button::new(
                format!("{req_name} - Отклонить"),
                format!("clan_decline:{req_pid}"),
            ));
    }
    win.button(Button::new("Назад", "clan_back"))
        .close_button()
        .send(state, tx, pid, "clan");
}

pub async fn handle_clan_accept(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    target_pid: i32,
) {
    let clan_id = match player_clan_id(state, pid) {
        Some(id) => id,
        None => return,
    };
    let Some(members) =
        load_clan_members_or_error(state, tx, pid, clan_id, "Failed to load clan members").await
    else {
        return;
    };
    let player_rank = clan_rank_for(&members, pid);

    if player_rank < crate::db::ClanRank::Officer {
        send_clan_no_rights(tx);
        return;
    }
    match state.db.accept_clan_request(clan_id, target_pid).await {
        Ok(()) => {
            state.modify_player(target_pid.into(), |ecs, entity| {
                let mut s = ecs.get_mut::<crate::game::PlayerStats>(entity)?;
                s.clan_id = Some(clan_id);
                s.clan_rank = crate::db::ClanRank::Member as i32;
                let mut flags = ecs.get_mut::<crate::game::PlayerFlags>(entity)?;
                flags.dirty = true;
                let cs = clan_show(clan_id);
                state.send_to_player(target_pid.into(), make_u_packet_bytes(cs.0, &cs.1));
                Some(())
            });
            handle_clan_requests_view(state, tx, pid).await;
        }
        Err(e) => {
            tracing::error!(clan_id, player_id = target_pid, error = ?e, "Failed to accept clan request");
            send_clan_ok(tx, "Ошибка", "Не удалось принять заявку");
        }
    }
}

pub async fn handle_clan_decline(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    target_pid: i32,
) {
    let clan_id = match player_clan_id(state, pid) {
        Some(id) => id,
        None => return,
    };
    let Some(members) =
        load_clan_members_or_error(state, tx, pid, clan_id, "Failed to load clan members").await
    else {
        return;
    };
    let player_rank = clan_rank_for(&members, pid);

    if player_rank < crate::db::ClanRank::Officer {
        send_clan_no_rights(tx);
        return;
    }
    if let Err(e) = state.db.decline_clan_request(clan_id, target_pid).await {
        tracing::error!(clan_id, player_id = target_pid, error = ?e, "Failed to decline clan request");
        send_clan_ok(tx, "Ошибка", "Ошибка БД");
        return;
    }
    handle_clan_requests_view(state, tx, pid).await;
}

pub async fn handle_clan_kick(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, target_pid: i32) {
    let clan_id = match player_clan_id(state, pid) {
        Some(id) => id,
        None => return,
    };
    let Some(members) =
        load_clan_members_or_error(state, tx, pid, clan_id, "Failed to load clan members").await
    else {
        return;
    };
    let player_rank = clan_rank_for(&members, pid);

    let target_entry = members.iter().find(|(id, _, _)| *id == target_pid);
    let target_rank = target_entry
        .map(|(_, _, r)| crate::db::ClanRank::from_db(*r))
        .unwrap_or(crate::db::ClanRank::None);

    if target_entry.is_none() {
        send_clan_ok(tx, "Ошибка", "Игрок не в вашем клане");
        return;
    }

    if player_rank <= target_rank && player_rank != crate::db::ClanRank::Leader {
        send_clan_no_rights(tx);
        return;
    }

    if target_rank == crate::db::ClanRank::Leader {
        send_clan_ok(tx, "Ошибка", "Нельзя исключить лидера");
        return;
    }

    if let Err(e) = state.db.kick_from_clan(target_pid).await {
        tracing::error!(clan_id, player_id = target_pid, error = ?e, "Failed to kick player from clan");
        send_clan_ok(tx, "Ошибка", "Ошибка БД");
        return;
    }
    state.modify_player(target_pid.into(), |ecs, entity| {
        let mut s = ecs.get_mut::<crate::game::PlayerStats>(entity)?;
        s.clan_id = None;
        s.clan_rank = crate::db::ClanRank::None as i32;
        let mut flags = ecs.get_mut::<crate::game::PlayerFlags>(entity)?;
        flags.dirty = true;
        let ch = clan_hide();
        state.send_to_player(target_pid.into(), make_u_packet_bytes(ch.0, &ch.1));
        Some(())
    });
    send_clan_ok(tx, "Клан", "Игрок исключён из клана");
    handle_clan_members_view(state, tx, pid).await;
}

pub async fn handle_clan_kick_by_name(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    target_name: &str,
) {
    let target = match state.db.get_player_by_name(target_name).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            send_clan_ok(tx, "Ошибка", "Игрок не найден");
            return;
        }
        Err(e) => {
            tracing::error!(player_id = %pid, target_name, error = ?e, "Failed to load clan kick target");
            send_clan_ok(tx, "Ошибка", "Ошибка БД");
            return;
        }
    };
    handle_clan_kick(state, tx, pid, target.id).await;
}

fn player_clan_id(state: &Arc<GameState>, pid: PlayerId) -> Option<i32> {
    state.query_player_opt(pid, |ecs, entity| {
        ecs.get::<crate::game::PlayerStats>(entity)
            .and_then(|s| s.clan_id)
    })
}

async fn is_clan_owner(state: &Arc<GameState>, clan_id: i32, pid: PlayerId) -> bool {
    match state.db.get_clan(clan_id).await {
        Ok(Some(clan)) => clan.owner_id == pid,
        _ => false,
    }
}

fn send_clan_no_rights(tx: &Outbox) {
    send_clan_ok(tx, "Ошибка", "Нет прав");
}

fn send_clan_ok(tx: &Outbox, title: &str, text: &str) {
    send_u_packet(tx, "OK", &ok_message(title, text).1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{PlayerFlags, PlayerStats};
    use crate::test_support::{ServerTestHarness, drain_events};

    async fn make_clan_test_state(label: &str) -> ServerTestHarness {
        ServerTestHarness::new(&format!("clans_{label}"), "clan-user").await
    }

    #[tokio::test]
    async fn clan_create_missing_flags_is_explicit_error_without_db_mutation() {
        let test = make_clan_test_state("create_missing_flags").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut stats = ecs.get_mut::<PlayerStats>(entity).unwrap();
            stats.creds = 1_000;
            ecs.entity_mut(entity).remove::<PlayerFlags>();
        }

        handle_clan_create(&test.state, &tx, pid, "NoFlags", "NFL").await;

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));
        assert!(test.state.db.list_clans().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn invite_accept_missing_flags_is_explicit_error_without_db_mutation() {
        let test = make_clan_test_state("invite_missing_flags").await;
        let owner = test.create_player("clan-owner").await;
        test.state
            .db
            .create_clan(1, "Owner Clan", "OWN", owner.id)
            .await
            .unwrap();
        test.state
            .db
            .add_clan_invite(1, test.player.id)
            .await
            .unwrap();

        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity).remove::<PlayerFlags>();
        }

        handle_clan_invite_accept(&test.state, &tx, pid, 1).await;

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));
        assert_eq!(
            test.state
                .db
                .get_player_invites(test.player.id)
                .await
                .unwrap(),
            vec![(1, "Owner Clan".to_string())]
        );
    }

    #[tokio::test]
    async fn leave_missing_flags_is_explicit_error_without_db_mutation() {
        let test = make_clan_test_state("leave_missing_flags").await;
        let owner = test.create_player("leave-owner").await;
        test.state
            .db
            .create_clan(1, "Leave Clan", "LVC", owner.id)
            .await
            .unwrap();
        test.state
            .db
            .add_clan_request(1, test.player.id)
            .await
            .unwrap();
        test.state
            .db
            .accept_clan_request(1, test.player.id)
            .await
            .unwrap();
        let player = test
            .state
            .db
            .get_player_by_id(test.player.id)
            .await
            .unwrap()
            .unwrap();

        let (tx, mut rx) = test.connect_player_with_outbox(&player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity).remove::<PlayerFlags>();
        }

        handle_clan_leave(&test.state, &tx, pid).await;

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));
        let db_player = test
            .state
            .db
            .get_player_by_id(player.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(db_player.clan_id, Some(1));
    }

    #[tokio::test]
    async fn owner_cannot_dissolve_populated_clan() {
        let test = make_clan_test_state("owner_populated_leave").await;
        let member = test.create_player("clan-member").await;
        test.state
            .db
            .create_clan(1, "Populated Clan", "POP", test.player.id)
            .await
            .unwrap();
        test.state.db.add_clan_request(1, member.id).await.unwrap();
        test.state
            .db
            .accept_clan_request(1, member.id)
            .await
            .unwrap();
        let owner = test
            .state
            .db
            .get_player_by_id(test.player.id)
            .await
            .unwrap()
            .unwrap();

        let (tx, mut rx) = test.connect_player_with_outbox(&owner, 1);
        drain_events(&mut rx);

        handle_clan_leave(&test.state, &tx, PlayerId(owner.id)).await;

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Сначала передайте лидерство"));

        assert!(test.state.db.get_clan(1).await.unwrap().is_some());
        let db_owner = test
            .state
            .db
            .get_player_by_id(owner.id)
            .await
            .unwrap()
            .unwrap();
        let db_member = test
            .state
            .db
            .get_player_by_id(member.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(db_owner.clan_id, Some(1));
        assert_eq!(db_member.clan_id, Some(1));
    }

    #[tokio::test]
    async fn clan_kick_offline_player_succeeds_in_db() {
        let test = make_clan_test_state("kick_offline").await;
        let mut owner = test.create_player("clan-owner").await;
        let target = test.create_player("clan-member").await;

        test.state
            .db
            .create_clan(1, "Kick Clan", "KCK", owner.id)
            .await
            .unwrap();
        test.state.db.add_clan_request(1, target.id).await.unwrap();
        test.state
            .db
            .accept_clan_request(1, target.id)
            .await
            .unwrap();

        owner.clan_id = Some(1);
        owner.clan_rank = crate::db::ClanRank::Leader as i32;

        let (tx, mut rx) = test.connect_player_with_outbox(&owner, 1);
        drain_events(&mut rx);

        handle_clan_kick(&test.state, &tx, PlayerId(owner.id), target.id).await;

        let db_target = test
            .state
            .db
            .get_player_by_id(target.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(db_target.clan_id, None);
    }

    #[tokio::test]
    async fn clan_promote_offline_player_succeeds_in_db() {
        let test = make_clan_test_state("promote_offline").await;
        let mut owner = test.create_player("clan-owner").await;
        let target = test.create_player("clan-member").await;

        test.state
            .db
            .create_clan(1, "Promote Clan", "PRM", owner.id)
            .await
            .unwrap();
        test.state.db.add_clan_request(1, target.id).await.unwrap();
        test.state
            .db
            .accept_clan_request(1, target.id)
            .await
            .unwrap();

        owner.clan_id = Some(1);
        owner.clan_rank = crate::db::ClanRank::Leader as i32;

        let (tx, mut rx) = test.connect_player_with_outbox(&owner, 1);
        drain_events(&mut rx);

        handle_clan_promote(&test.state, &tx, PlayerId(owner.id), target.id).await;

        let db_target = test
            .state
            .db
            .get_player_by_id(target.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(db_target.clan_rank, crate::db::ClanRank::Officer as i32);
    }

    #[tokio::test]
    async fn clan_accept_offline_player_succeeds_in_db() {
        let test = make_clan_test_state("accept_offline").await;
        let mut owner = test.create_player("clan-owner").await;
        let target = test.create_player("clan-member").await;

        test.state
            .db
            .create_clan(1, "Accept Clan", "ACC", owner.id)
            .await
            .unwrap();
        test.state.db.add_clan_request(1, target.id).await.unwrap();

        owner.clan_id = Some(1);
        owner.clan_rank = crate::db::ClanRank::Leader as i32;

        let (tx, mut rx) = test.connect_player_with_outbox(&owner, 1);
        drain_events(&mut rx);

        handle_clan_accept(&test.state, &tx, PlayerId(owner.id), target.id).await;

        let db_target = test
            .state
            .db
            .get_player_by_id(target.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(db_target.clan_id, Some(1));
    }

    #[tokio::test]
    async fn clan_create_with_toctou_prevention_does_not_go_negative() {
        let test = make_clan_test_state("create_toctou").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        // Задаем игроку ровно 999 кредитов
        test.state.modify_player(pid, |ecs, entity| {
            let mut s = ecs.get_mut::<PlayerStats>(entity)?;
            s.creds = 999;
            Some(())
        });

        handle_clan_create(&test.state, &tx, pid, "ToctouClan", "TTC").await;

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let msg = std::str::from_utf8(&events[0].1).unwrap();
        assert!(msg.contains("Недостаточно кредитов"));

        let current_creds = test
            .state
            .query_player(pid, |ecs, entity| {
                ecs.get::<PlayerStats>(entity).map(|s| s.creds)
            })
            .unwrap();
        assert_eq!(current_creds, Some(999)); // Баланс не изменился и не ушел в минус
    }
}
