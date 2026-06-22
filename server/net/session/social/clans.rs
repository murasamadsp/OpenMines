//! Меню и действия клана.
#![allow(dead_code, clippy::uninlined_format_args)]
use crate::net::session::prelude::*;
use crate::net::session::ui::horb::{Button, Horb};

// ─── Clans ─────────────────────────────────────────────────────────────

pub async fn handle_clan_menu(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let clan_id = player_clan_id(state, pid);
    if let Some(cid) = clan_id {
        handle_clan_info_view(state, tx, pid, cid).await;
    } else {
        let invites = state.db.get_player_invites(pid).await.unwrap_or_default();
        let mut win = Horb::new("КЛАНЫ").text("Выберите клан или создайте свой");

        if !invites.is_empty() {
            win = win.button(Button::new(
                format!("Приглашения ({})", invites.len()),
                "clan_invites_view",
            ));
        }

        win = win.button(Button::new("Создать клан (1000 кр.)", "clan_create"));
        let clans = state.db.list_clans().await.unwrap_or_default();
        for clan in &clans {
            let members = state.db.get_clan_members(clan.id).await.unwrap_or_default();
            let label = format!("{} [{}] ({} чел.)", clan.name, clan.abr, members.len());
            win = win.button(Button::new(label, format!("clan_view:{}", clan.id)));
        }
        win.close_button().send_untracked(tx);
    }
}

pub async fn handle_clan_info_view(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    clan_id: i32,
) {
    let clan = match state.db.get_clan(clan_id).await {
        Ok(Some(c)) => c,
        _ => {
            send_clan_ok(tx, "Ошибка", "Клан не найден");
            return;
        }
    };
    let members = state.db.get_clan_members(clan_id).await.unwrap_or_default();
    let player_rank = members
        .iter()
        .find(|(id, _, _)| *id == pid)
        .map(|(_, _, r)| crate::db::ClanRank::from_db(*r))
        .unwrap_or(crate::db::ClanRank::None);

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
        let requests = state
            .db
            .get_clan_requests(clan_id)
            .await
            .unwrap_or_default();
        win = win
            .button(Button::new(
                format!("Заявки ({})", requests.len()),
                "clan_requests",
            ))
            .button(Button::new("Пригласить игрока", "clan_invite_list"));
    }

    win.button(Button::new("Покинуть клан", "clan_leave"))
        .close_button()
        .send_untracked(tx);
}

pub async fn handle_clan_preview(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    clan_id: i32,
) {
    let clan = match state.db.get_clan(clan_id).await {
        Ok(Some(c)) => c,
        _ => {
            send_clan_ok(tx, "Ошибка", "Клан не найден");
            return;
        }
    };
    let members = state.db.get_clan_members(clan_id).await.unwrap_or_default();
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
        .send_untracked(tx);
}

pub async fn handle_clan_create(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    name: &str,
    tag: &str,
) {
    if player_clan_id(state, pid).is_some() {
        send_clan_ok(tx, "Ошибка", "Вы уже в клане");
        return;
    }

    let pstats = state.query_player_opt(pid, |ecs, entity| {
        let s = ecs.get::<crate::game::PlayerStats>(entity)?;
        Some((s.creds, s.money))
    });

    let Some((p_creds, p_money)) = pstats else {
        return;
    };

    if p_creds < 1000 {
        send_clan_ok(tx, "Ошибка", "Недостаточно кредитов (нужно 1000)");
        return;
    }

    let used_ids = state.db.get_used_clan_ids().await.unwrap_or_default();
    let new_id = used_ids.iter().max().map_or(1, |&id| id + 1);
    let icon = state.db.pick_clan_icon().await.unwrap_or(1);

    match state.db.create_clan(new_id, name, tag, pid, icon).await {
        Ok(()) => {
            state.modify_player(pid, |ecs, entity| {
                let mut s = ecs.get_mut::<crate::game::PlayerStats>(entity)?;
                s.creds -= 1000;
                s.clan_id = Some(new_id);
                s.clan_rank = crate::db::ClanRank::Leader as i32;
                Some(())
            });
            send_u_packet(tx, "P$", &money(p_money, p_creds - 1000).1);
            // cS = номер иконки (клиент: ClanSprite.sprites[icon-1], 1..=218),
            // а НЕ clan_id. Иконка уже выбрана выше (pick_clan_icon).
            let cs = clan_show(icon);
            send_u_packet(tx, cs.0, &cs.1);
            send_clan_ok(tx, "Клан", "Клан успешно создан!");
            handle_clan_info_view(state, tx, pid, new_id).await;
        }
        Err(e) => {
            tracing::error!("create_clan error: {e}");
            send_clan_ok(
                tx,
                "Ошибка",
                "Не удалось создать клан (возможно, имя или тег заняты)",
            );
        }
    }
}

pub async fn handle_clan_leave(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let clan_id = match player_clan_id(state, pid) {
        Some(id) => id,
        None => return,
    };

    if is_clan_owner(state, clan_id, pid).await {
        match state.db.delete_clan(clan_id).await {
            Ok(()) => {
                for entry in &state.active_players {
                    let target_pid = *entry.key();
                    state.modify_player(target_pid, |ecs, entity| {
                        let mut s = ecs.get_mut::<crate::game::PlayerStats>(entity)?;
                        if s.clan_id == Some(clan_id) {
                            s.clan_id = None;
                            s.clan_rank = 0;
                            let conn = ecs.get::<crate::game::PlayerConnection>(entity)?;
                            let ch = clan_hide();
                            let _ = conn.tx.send(make_u_packet_bytes(ch.0, &ch.1));
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
                tracing::error!("delete_clan error: {e}");
                send_clan_ok(tx, "Ошибка", "Не удалось покинуть клан");
            }
        }
    } else {
        match state.db.leave_clan(pid).await {
            Ok(()) => {
                state.modify_player(pid, |ecs, entity| {
                    let mut s = ecs.get_mut::<crate::game::PlayerStats>(entity)?;
                    s.clan_id = None;
                    s.clan_rank = 0;
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
                tracing::error!("leave_clan error: {e}");
                send_clan_ok(tx, "Ошибка", "Не удалось покинуть клан");
            }
        }
    }
}

pub async fn handle_clan_join_request(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    clan_id: i32,
) {
    if player_clan_id(state, pid).is_some() {
        send_clan_ok(tx, "Ошибка", "Вы уже в клане");
        return;
    }

    // Check for existing pending request (1:1 C# Clan.AddReq: reqs.FirstOrDefault(i => i.player.id == id)).
    let existing = state
        .db
        .get_clan_requests(clan_id)
        .await
        .unwrap_or_default();
    if existing.iter().any(|(req_pid, _)| *req_pid == pid) {
        send_clan_ok(tx, "Клан", "Заявка уже подана");
        return;
    }

    match state.db.add_clan_request(clan_id, pid).await {
        Ok(()) => {
            send_clan_ok(tx, "Клан", "Заявка отправлена");
        }
        Err(e) => {
            tracing::error!("add_clan_request error: {e}");
            send_clan_ok(tx, "Ошибка", "Не удалось отправить заявку");
        }
    }
}

pub async fn handle_clan_members_view(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let clan_id = match player_clan_id(state, pid) {
        Some(id) => id,
        None => return,
    };
    let members = state.db.get_clan_members(clan_id).await.unwrap_or_default();
    let player_rank = members
        .iter()
        .find(|(id, _, _)| *id == pid)
        .map(|(_, _, r)| crate::db::ClanRank::from_db(*r))
        .unwrap_or(crate::db::ClanRank::None);

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
        .send_untracked(tx);
}

pub async fn handle_clan_invite_list(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let clan_id = match player_clan_id(state, pid) {
        Some(id) => id,
        None => return,
    };
    let members = state.db.get_clan_members(clan_id).await.unwrap_or_default();
    let player_rank = members
        .iter()
        .find(|(id, _, _)| *id == pid)
        .map(|(_, _, r)| crate::db::ClanRank::from_db(*r))
        .unwrap_or(crate::db::ClanRank::None);

    if player_rank < crate::db::ClanRank::Officer {
        send_clan_no_rights(tx);
        return;
    }

    let mut win = Horb::new("Пригласить").text("Выберите игрока для приглашения в клан:");
    let mut count = 0;

    for entry in &state.active_players {
        let target_pid = *entry.key();
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
        .send_untracked(tx);
}

pub async fn handle_clan_invite_send(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    target_pid: i32,
) {
    let clan_id = match player_clan_id(state, pid) {
        Some(id) => id,
        None => return,
    };
    let members = state.db.get_clan_members(clan_id).await.unwrap_or_default();
    let player_rank = members
        .iter()
        .find(|(id, _, _)| *id == pid)
        .map(|(_, _, r)| crate::db::ClanRank::from_db(*r))
        .unwrap_or(crate::db::ClanRank::None);

    if player_rank < crate::db::ClanRank::Officer {
        send_clan_no_rights(tx);
        return;
    }

    match state.db.add_clan_invite(clan_id, target_pid).await {
        Ok(()) => {
            send_clan_ok(tx, "Клан", "Приглашение отправлено");
            state.query_player(target_pid, |ecs, entity| {
                if let Some(conn) = ecs.get::<crate::game::PlayerConnection>(entity) {
                    let _ = conn.tx.send(make_u_packet_bytes(
                        "OK",
                        &ok_message("Клан", "Вас пригласили в клан!").1,
                    ));
                }
            });
        }
        Err(e) => {
            tracing::error!("add_clan_invite error: {e}");
            send_clan_ok(tx, "Ошибка", "Не удалось отправить приглашение");
        }
    }
}

pub async fn handle_clan_invites_view(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let invites = state.db.get_player_invites(pid).await.unwrap_or_default();
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
        .send_untracked(tx);
}

pub async fn handle_clan_invite_accept(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    clan_id: i32,
) {
    if player_clan_id(state, pid).is_some() {
        send_clan_ok(tx, "Ошибка", "Вы уже в клане");
        return;
    }

    match state.db.accept_clan_invite(clan_id, pid).await {
        Ok(()) => {
            state.modify_player(pid, |ecs, entity| {
                let mut s = ecs.get_mut::<crate::game::PlayerStats>(entity)?;
                s.clan_id = Some(clan_id);
                s.clan_rank = crate::db::ClanRank::Member as i32;
                Some(())
            });
            // cS = номер иконки (клиент: ClanSprite.sprites[icon-1]), не clan_id.
            let icon = state
                .db
                .get_clan(clan_id)
                .await
                .ok()
                .flatten()
                .map_or(1, |c| c.icon);
            let cs = clan_show(icon);
            send_u_packet(tx, cs.0, &cs.1);
            send_clan_ok(tx, "Клан", "Вы вступили в клан!");
        }
        Err(e) => {
            tracing::error!("accept_clan_invite error: {e}");
            send_clan_ok(tx, "Ошибка", "Не удалось принять приглашение");
        }
    }
}

pub async fn handle_clan_invite_decline(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    clan_id: i32,
) {
    let _ = state.db.decline_clan_invite(clan_id, pid).await;
    handle_clan_invites_view(state, tx, pid).await;
}

pub async fn handle_clan_promote(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
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
        .set_clan_rank(target_pid, crate::db::ClanRank::Officer)
        .await
    {
        Ok(()) => {
            state.modify_player(target_pid, |ecs, entity| {
                let mut s = ecs.get_mut::<crate::game::PlayerStats>(entity)?;
                s.clan_rank = crate::db::ClanRank::Officer as i32;
                Some(())
            });
            send_clan_ok(tx, "Клан", "Игрок повышен до Офицера");
            handle_clan_members_view(state, tx, pid).await;
        }
        Err(e) => {
            tracing::error!("set_clan_rank error: {e}");
            send_clan_ok(tx, "Ошибка", "Не удалось повысить игрока");
        }
    }
}

pub async fn handle_clan_requests_view(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let clan_id = match player_clan_id(state, pid) {
        Some(id) => id,
        None => return,
    };
    let members = state.db.get_clan_members(clan_id).await.unwrap_or_default();
    let player_rank = members
        .iter()
        .find(|(id, _, _)| *id == pid)
        .map(|(_, _, r)| crate::db::ClanRank::from_db(*r))
        .unwrap_or(crate::db::ClanRank::None);

    if player_rank < crate::db::ClanRank::Officer {
        send_clan_no_rights(tx);
        return;
    }
    let requests = state
        .db
        .get_clan_requests(clan_id)
        .await
        .unwrap_or_default();
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
        .send_untracked(tx);
}

pub async fn handle_clan_accept(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    target_pid: i32,
) {
    let clan_id = match player_clan_id(state, pid) {
        Some(id) => id,
        None => return,
    };
    let members = state.db.get_clan_members(clan_id).await.unwrap_or_default();
    let player_rank = members
        .iter()
        .find(|(id, _, _)| *id == pid)
        .map(|(_, _, r)| crate::db::ClanRank::from_db(*r))
        .unwrap_or(crate::db::ClanRank::None);

    if player_rank < crate::db::ClanRank::Officer {
        send_clan_no_rights(tx);
        return;
    }
    match state.db.accept_clan_request(clan_id, target_pid).await {
        Ok(()) => {
            let icon = state
                .db
                .get_clan(clan_id)
                .await
                .ok()
                .flatten()
                .map_or(1, |c| c.icon);
            state.modify_player(target_pid, |ecs, entity| {
                let mut s = ecs.get_mut::<crate::game::PlayerStats>(entity)?;
                s.clan_id = Some(clan_id);
                s.clan_rank = crate::db::ClanRank::Member as i32;
                if let Some(conn) = ecs.get::<crate::game::PlayerConnection>(entity) {
                    let cs = clan_show(icon);
                    let _ = conn.tx.send(make_u_packet_bytes(cs.0, &cs.1));
                }
                Some(())
            });
            handle_clan_requests_view(state, tx, pid).await;
        }
        Err(e) => {
            tracing::error!("accept_clan_request error: {e}");
            send_clan_ok(tx, "Ошибка", "Не удалось принять заявку");
        }
    }
}

pub async fn handle_clan_decline(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    target_pid: i32,
) {
    let clan_id = match player_clan_id(state, pid) {
        Some(id) => id,
        None => return,
    };
    let members = state.db.get_clan_members(clan_id).await.unwrap_or_default();
    let player_rank = members
        .iter()
        .find(|(id, _, _)| *id == pid)
        .map(|(_, _, r)| crate::db::ClanRank::from_db(*r))
        .unwrap_or(crate::db::ClanRank::None);

    if player_rank < crate::db::ClanRank::Officer {
        send_clan_no_rights(tx);
        return;
    }
    let _ = state.db.decline_clan_request(clan_id, target_pid).await;
    handle_clan_requests_view(state, tx, pid).await;
}

pub async fn handle_clan_kick(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    target_pid: i32,
) {
    let clan_id = match player_clan_id(state, pid) {
        Some(id) => id,
        None => return,
    };
    let members = state.db.get_clan_members(clan_id).await.unwrap_or_default();
    let player_rank = members
        .iter()
        .find(|(id, _, _)| *id == pid)
        .map(|(_, _, r)| crate::db::ClanRank::from_db(*r))
        .unwrap_or(crate::db::ClanRank::None);

    let target_rank = members
        .iter()
        .find(|(id, _, _)| *id == target_pid)
        .map(|(_, _, r)| crate::db::ClanRank::from_db(*r))
        .unwrap_or(crate::db::ClanRank::None);

    if player_rank <= target_rank && player_rank != crate::db::ClanRank::Leader {
        send_clan_no_rights(tx);
        return;
    }

    if target_rank == crate::db::ClanRank::Leader {
        send_clan_ok(tx, "Ошибка", "Нельзя исключить лидера");
        return;
    }

    let _ = state.db.kick_from_clan(target_pid).await;
    state.modify_player(target_pid, |ecs, entity| {
        let mut s = ecs.get_mut::<crate::game::PlayerStats>(entity)?;
        s.clan_id = None;
        s.clan_rank = crate::db::ClanRank::None as i32;
        if let Some(conn) = ecs.get::<crate::game::PlayerConnection>(entity) {
            let ch = clan_hide();
            let _ = conn.tx.send(make_u_packet_bytes(ch.0, &ch.1));
        }
        Some(())
    });
    send_clan_ok(tx, "Клан", "Игрок исключён из клана");
    handle_clan_members_view(state, tx, pid).await;
}

pub async fn handle_clan_kick_by_name(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    target_name: &str,
) {
    let target = match state.db.get_player_by_name(target_name).await {
        Ok(Some(p)) => p,
        _ => {
            send_clan_ok(tx, "Ошибка", "Игрок не найден");
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

fn send_clan_no_rights(tx: &mpsc::UnboundedSender<Vec<u8>>) {
    send_clan_ok(tx, "Ошибка", "Нет прав");
}

fn send_clan_ok(tx: &mpsc::UnboundedSender<Vec<u8>>, title: &str, text: &str) {
    send_u_packet(tx, "OK", &ok_message(title, text).1);
}
