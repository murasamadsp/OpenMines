//! Меню и действия клана.
#![allow(dead_code, clippy::uninlined_format_args)]
use crate::net::session::prelude::*;

// ─── Clans ─────────────────────────────────────────────────────────────

pub fn handle_clan_menu(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let clan_id = player_clan_id(state, pid);
    if let Some(cid) = clan_id {
        handle_clan_info_view(state, tx, pid, cid);
    } else {
        let invites = state.db.get_player_invites(pid).unwrap_or_default();
        let mut buttons: Vec<serde_json::Value> = Vec::new();

        if !invites.is_empty() {
            buttons.push(serde_json::json!(format!(
                "Приглашения ({})",
                invites.len()
            )));
            buttons.push(serde_json::json!("clan_invites_view"));
        }

        buttons.push(serde_json::Value::String("Создать клан (1000 кр.)".into()));
        buttons.push(serde_json::Value::String("clan_create".into()));
        let clans = state.db.list_clans().unwrap_or_default();
        for clan in &clans {
            let members = state.db.get_clan_members(clan.id).unwrap_or_default();
            let label = format!("{} [{}] ({} чел.)", clan.name, clan.abr, members.len());
            buttons.push(serde_json::Value::String(label));
            buttons.push(serde_json::Value::String(format!("clan_view:{}", clan.id)));
        }
        append_close_buttons(&mut buttons);
        let gui = serde_json::json!({
            "title": "КЛАНЫ",
            "text": "Выберите клан или создайте свой",
            "buttons": buttons,
            "back": false
        });
        send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
    }
}

pub fn handle_clan_info_view(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    clan_id: i32,
) {
    let clan = match state.db.get_clan(clan_id) {
        Ok(Some(c)) => c,
        _ => {
            send_clan_ok(tx, "Ошибка", "Клан не найден");
            return;
        }
    };
    let members = state.db.get_clan_members(clan_id).unwrap_or_default();
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
    let mut buttons: Vec<serde_json::Value> = Vec::new();

    buttons.push(serde_json::json!("Участники"));
    buttons.push(serde_json::json!("clan_members"));

    if player_rank >= crate::db::ClanRank::Officer {
        let requests = state.db.get_clan_requests(clan_id).unwrap_or_default();
        let req_label = format!("Заявки ({})", requests.len());
        buttons.push(serde_json::json!(req_label));
        buttons.push(serde_json::json!("clan_requests"));

        buttons.push(serde_json::json!("Пригласить игрока"));
        buttons.push(serde_json::json!("clan_invite_list"));
    }

    buttons.push(serde_json::Value::String("Покинуть клан".into()));
    buttons.push(serde_json::Value::String("clan_leave".into()));
    append_close_buttons(&mut buttons);
    let gui = serde_json::json!({
        "title": clan.name,
        "text": text,
        "buttons": buttons,
        "back": false
    });
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
}

pub fn handle_clan_preview(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    clan_id: i32,
) {
    let clan = match state.db.get_clan(clan_id) {
        Ok(Some(c)) => c,
        _ => {
            send_clan_ok(tx, "Ошибка", "Клан не найден");
            return;
        }
    };
    let members = state.db.get_clan_members(clan_id).unwrap_or_default();
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
    let mut buttons: Vec<serde_json::Value> = Vec::new();

    if player_clan_id(state, pid).is_none() {
        buttons.push(serde_json::json!("Подать заявку"));
        buttons.push(serde_json::json!(format!("clan_request:{}", clan_id)));
    }

    buttons.push(serde_json::json!("Назад"));
    buttons.push(serde_json::json!("clan_back"));
    append_close_buttons(&mut buttons);

    let gui = serde_json::json!({
        "title": clan.name,
        "text": text,
        "buttons": buttons,
        "back": false
    });
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
}

pub fn handle_clan_create(
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

    let stats = state.query_player(pid, |ecs, entity| {
        let s = ecs.get::<crate::game::PlayerStats>(entity)?;
        Some((s.creds, s.money))
    }).flatten();

    let Some((p_creds, p_money)) = stats else { return; };

    if p_creds < 1000 {
        send_clan_ok(tx, "Ошибка", "Недостаточно кредитов (нужно 1000)");
        return;
    }

    let used_ids = state.db.get_used_clan_ids().unwrap_or_default();
    let new_id = used_ids.iter().max().map_or(1, |&id| id + 1);

    match state.db.create_clan(new_id, name, tag, pid) {
        Ok(()) => {
            state.modify_player(pid, |ecs, entity| {
                let mut s = ecs.get_mut::<crate::game::PlayerStats>(entity)?;
                s.creds -= 1000;
                s.clan_id = Some(new_id);
                s.clan_rank = crate::db::ClanRank::Leader as i32;
                Some(())
            });
            send_u_packet(tx, "P$", &money(p_money, p_creds - 1000).1);
            let cs = clan_show(new_id);
            send_u_packet(tx, cs.0, &cs.1);
            send_clan_ok(tx, "Клан", "Клан успешно создан!");
            handle_clan_info_view(state, tx, pid, new_id);
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

pub fn handle_clan_leave(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let clan_id = match player_clan_id(state, pid) {
        Some(id) => id,
        None => return,
    };

    if is_clan_owner(state, clan_id, pid) {
        match state.db.delete_clan(clan_id) {
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
                send_clan_ok(tx, "Клан", "Клан расформирован");
                handle_clan_menu(state, tx, pid);
            }
            Err(e) => {
                tracing::error!("delete_clan error: {e}");
                send_clan_ok(tx, "Ошибка", "Не удалось покинуть клан");
            }
        }
    } else {
        match state.db.leave_clan(pid) {
            Ok(()) => {
                state.modify_player(pid, |ecs, entity| {
                    let mut s = ecs.get_mut::<crate::game::PlayerStats>(entity)?;
                    s.clan_id = None;
                    s.clan_rank = 0;
                    Some(())
                });
                let ch = clan_hide();
                send_u_packet(tx, ch.0, &ch.1);
                send_clan_ok(tx, "Клан", "Вы покинули клан");
                handle_clan_menu(state, tx, pid);
            }
            Err(e) => {
                tracing::error!("leave_clan error: {e}");
                send_clan_ok(tx, "Ошибка", "Не удалось покинуть клан");
            }
        }
    }
}

pub fn handle_clan_join_request(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    clan_id: i32,
) {
    if player_clan_id(state, pid).is_some() {
        send_clan_ok(tx, "Ошибка", "Вы уже в клане");
        return;
    }

    match state.db.add_clan_request(clan_id, pid) {
        Ok(()) => {
            send_clan_ok(tx, "Клан", "Заявка отправлена");
        }
        Err(e) => {
            tracing::error!("add_clan_request error: {e}");
            send_clan_ok(tx, "Ошибка", "Не удалось отправить заявку");
        }
    }
}

pub fn handle_clan_members_view(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let clan_id = match player_clan_id(state, pid) {
        Some(id) => id,
        None => return,
    };
    let members = state.db.get_clan_members(clan_id).unwrap_or_default();
    let player_rank = members
        .iter()
        .find(|(id, _, _)| *id == pid)
        .map(|(_, _, r)| crate::db::ClanRank::from_db(*r))
        .unwrap_or(crate::db::ClanRank::None);

    let mut buttons: Vec<serde_json::Value> = Vec::new();
    let mut text = String::from("Участники клана:\n");

    for (m_pid, m_name, m_rank_raw) in &members {
        let m_rank = crate::db::ClanRank::from_db(*m_rank_raw);
        text.push_str(&format!("- {} ({:?})\n", m_name, m_rank));

        if *m_pid != pid {
            if player_rank == crate::db::ClanRank::Leader && m_rank == crate::db::ClanRank::Member {
                buttons.push(serde_json::json!(format!("Повысить {}", m_name)));
                buttons.push(serde_json::json!(format!("clan_promote:{}", m_pid)));
            }
            if player_rank > m_rank {
                buttons.push(serde_json::json!(format!("Исключить {}", m_name)));
                buttons.push(serde_json::json!(format!("clan_kick_id:{}", m_pid)));
            }
        }
    }

    buttons.push(serde_json::json!("Назад"));
    buttons.push(serde_json::json!("clan_back"));
    append_close_buttons(&mut buttons);

    let gui = serde_json::json!({
        "title": "Участники",
        "text": text,
        "buttons": buttons,
        "back": false
    });
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
}

pub fn handle_clan_invite_list(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let clan_id = match player_clan_id(state, pid) {
        Some(id) => id,
        None => return,
    };
    let members = state.db.get_clan_members(clan_id).unwrap_or_default();
    let player_rank = members
        .iter()
        .find(|(id, _, _)| *id == pid)
        .map(|(_, _, r)| crate::db::ClanRank::from_db(*r))
        .unwrap_or(crate::db::ClanRank::None);

    if player_rank < crate::db::ClanRank::Officer {
        send_clan_no_rights(tx);
        return;
    }

    let mut buttons: Vec<serde_json::Value> = Vec::new();
    let mut count = 0;

    for entry in &state.active_players {
        let target_pid = *entry.key();
        if target_pid == pid {
            continue;
        }
        
        let target_data = state.query_player(target_pid, |ecs, entity| {
            let s = ecs.get::<crate::game::PlayerStats>(entity)?;
            let m = ecs.get::<crate::game::PlayerMetadata>(entity)?;
            if s.clan_id.is_none() {
                Some(m.name.clone())
            } else { None }
        }).flatten();

        if let Some(name) = target_data {
            buttons.push(serde_json::json!(format!("Пригласить {}", name)));
            buttons.push(serde_json::json!(format!("clan_invite_send:{}", target_pid)));
            count += 1;
        }
        if count >= 20 { break; }
    }

    if count == 0 {
        buttons.push(serde_json::json!("Никого нет рядом без клана"));
        buttons.push(serde_json::json!("noop"));
    }

    buttons.push(serde_json::json!("Назад"));
    buttons.push(serde_json::json!("clan_back"));
    append_close_buttons(&mut buttons);

    let gui = serde_json::json!({
        "title": "Пригласить",
        "text": "Выберите игрока для приглашения в клан:",
        "buttons": buttons,
        "back": false
    });
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
}

pub fn handle_clan_invite_send(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    target_pid: i32,
) {
    let clan_id = match player_clan_id(state, pid) {
        Some(id) => id,
        None => return,
    };
    let members = state.db.get_clan_members(clan_id).unwrap_or_default();
    let player_rank = members
        .iter()
        .find(|(id, _, _)| *id == pid)
        .map(|(_, _, r)| crate::db::ClanRank::from_db(*r))
        .unwrap_or(crate::db::ClanRank::None);

    if player_rank < crate::db::ClanRank::Officer {
        send_clan_no_rights(tx);
        return;
    }

    match state.db.add_clan_invite(clan_id, target_pid) {
        Ok(()) => {
            send_clan_ok(tx, "Клан", "Приглашение отправлено");
            state.query_player(target_pid, |ecs, entity| {
                if let Some(conn) = ecs.get::<crate::game::PlayerConnection>(entity) {
                    let _ = conn.tx.send(make_u_packet_bytes("OK", &ok_message("Клан", "Вас пригласили в клан!").1));
                }
            });
        }
        Err(e) => {
            tracing::error!("add_clan_invite error: {e}");
            send_clan_ok(tx, "Ошибка", "Не удалось отправить приглашение");
        }
    }
}

pub fn handle_clan_invites_view(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let invites = state.db.get_player_invites(pid).unwrap_or_default();
    let mut buttons: Vec<serde_json::Value> = Vec::new();

    for (clan_id, clan_name) in &invites {
        buttons.push(serde_json::json!(format!("Принять {}", clan_name)));
        buttons.push(serde_json::json!(format!("clan_invite_accept:{}", clan_id)));
        buttons.push(serde_json::json!(format!("Отклонить {}", clan_name)));
        buttons.push(serde_json::json!(format!(
            "clan_invite_decline:{}",
            clan_id
        )));
    }

    buttons.push(serde_json::json!("Назад"));
    buttons.push(serde_json::json!("clan_back"));
    append_close_buttons(&mut buttons);

    let gui = serde_json::json!({
        "title": "Приглашения",
        "text": "Вас пригласили в следующие кланы:",
        "buttons": buttons,
        "back": false
    });
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
}

pub fn handle_clan_invite_accept(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    clan_id: i32,
) {
    if player_clan_id(state, pid).is_some() {
        send_clan_ok(tx, "Ошибка", "Вы уже в клане");
        return;
    }

    match state.db.accept_clan_invite(clan_id, pid) {
        Ok(()) => {
            state.modify_player(pid, |ecs, entity| {
                let mut s = ecs.get_mut::<crate::game::PlayerStats>(entity)?;
                s.clan_id = Some(clan_id);
                s.clan_rank = crate::db::ClanRank::Member as i32;
                Some(())
            });
            let cs = clan_show(clan_id);
            send_u_packet(tx, cs.0, &cs.1);
            send_clan_ok(tx, "Клан", "Вы вступили в клан!");
        }
        Err(e) => {
            tracing::error!("accept_clan_invite error: {e}");
            send_clan_ok(tx, "Ошибка", "Не удалось принять приглашение");
        }
    }
}

pub fn handle_clan_invite_decline(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    clan_id: i32,
) {
    let _ = state.db.decline_clan_invite(clan_id, pid);
    handle_clan_invites_view(state, tx, pid);
}

pub fn handle_clan_promote(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    target_pid: i32,
) {
    let clan_id = match player_clan_id(state, pid) {
        Some(id) => id,
        None => return,
    };
    if !is_clan_owner(state, clan_id, pid) {
        send_clan_no_rights(tx);
        return;
    }

    match state
        .db
        .set_clan_rank(target_pid, crate::db::ClanRank::Officer)
    {
        Ok(()) => {
            state.modify_player(target_pid, |ecs, entity| {
                let mut s = ecs.get_mut::<crate::game::PlayerStats>(entity)?;
                s.clan_rank = crate::db::ClanRank::Officer as i32;
                Some(())
            });
            send_clan_ok(tx, "Клан", "Игрок повышен до Офицера");
            handle_clan_members_view(state, tx, pid);
        }
        Err(e) => {
            tracing::error!("set_clan_rank error: {e}");
            send_clan_ok(tx, "Ошибка", "Не удалось повысить игрока");
        }
    }
}

pub fn handle_clan_requests_view(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let clan_id = match player_clan_id(state, pid) {
        Some(id) => id,
        None => return,
    };
    let members = state.db.get_clan_members(clan_id).unwrap_or_default();
    let player_rank = members
        .iter()
        .find(|(id, _, _)| *id == pid)
        .map(|(_, _, r)| crate::db::ClanRank::from_db(*r))
        .unwrap_or(crate::db::ClanRank::None);

    if player_rank < crate::db::ClanRank::Officer {
        send_clan_no_rights(tx);
        return;
    }
    let requests = state.db.get_clan_requests(clan_id).unwrap_or_default();
    let mut buttons: Vec<serde_json::Value> = Vec::new();
    for (req_pid, req_name) in &requests {
        buttons.push(serde_json::Value::String(format!("{req_name} - Принять")));
        buttons.push(serde_json::Value::String(format!("clan_accept:{req_pid}")));
        buttons.push(serde_json::Value::String(format!("{req_name} - Отклонить")));
        buttons.push(serde_json::Value::String(format!("clan_decline:{req_pid}")));
    }
    buttons.push(serde_json::Value::String("Назад".into()));
    buttons.push(serde_json::Value::String("clan_back".into()));
    append_close_buttons(&mut buttons);
    let gui = serde_json::json!({
        "title": "Заявки",
        "text": "Заявки в клан:",
        "buttons": buttons,
        "back": false
    });
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
}

pub fn handle_clan_accept(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    target_pid: i32,
) {
    let clan_id = match player_clan_id(state, pid) {
        Some(id) => id,
        None => return,
    };
    let members = state.db.get_clan_members(clan_id).unwrap_or_default();
    let player_rank = members
        .iter()
        .find(|(id, _, _)| *id == pid)
        .map(|(_, _, r)| crate::db::ClanRank::from_db(*r))
        .unwrap_or(crate::db::ClanRank::None);

    if player_rank < crate::db::ClanRank::Officer {
        send_clan_no_rights(tx);
        return;
    }
    match state.db.accept_clan_request(clan_id, target_pid) {
        Ok(()) => {
            state.modify_player(target_pid, |ecs, entity| {
                let mut s = ecs.get_mut::<crate::game::PlayerStats>(entity)?;
                s.clan_id = Some(clan_id);
                s.clan_rank = crate::db::ClanRank::Member as i32;
                if let Some(conn) = ecs.get::<crate::game::PlayerConnection>(entity) {
                    let cs = clan_show(clan_id);
                    let _ = conn.tx.send(make_u_packet_bytes(cs.0, &cs.1));
                }
                Some(())
            });
            handle_clan_requests_view(state, tx, pid);
        }
        Err(e) => {
            tracing::error!("accept_clan_request error: {e}");
            send_clan_ok(tx, "Ошибка", "Не удалось принять заявку");
        }
    }
}

pub fn handle_clan_decline(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    target_pid: i32,
) {
    let clan_id = match player_clan_id(state, pid) {
        Some(id) => id,
        None => return,
    };
    let members = state.db.get_clan_members(clan_id).unwrap_or_default();
    let player_rank = members
        .iter()
        .find(|(id, _, _)| *id == pid)
        .map(|(_, _, r)| crate::db::ClanRank::from_db(*r))
        .unwrap_or(crate::db::ClanRank::None);

    if player_rank < crate::db::ClanRank::Officer {
        send_clan_no_rights(tx);
        return;
    }
    let _ = state.db.decline_clan_request(clan_id, target_pid);
    handle_clan_requests_view(state, tx, pid);
}

pub fn handle_clan_kick(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    target_pid: i32,
) {
    let clan_id = match player_clan_id(state, pid) {
        Some(id) => id,
        None => return,
    };
    let members = state.db.get_clan_members(clan_id).unwrap_or_default();
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

    let _ = state.db.kick_from_clan(target_pid);
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
    handle_clan_members_view(state, tx, pid);
}

pub fn handle_clan_kick_by_name(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    target_name: &str,
) {
    let target = match state.db.get_player_by_name(target_name) {
        Ok(Some(p)) => p,
        _ => {
            send_clan_ok(tx, "Ошибка", "Игрок не найден");
            return;
        }
    };
    handle_clan_kick(state, tx, pid, target.id);
}

fn player_clan_id(state: &Arc<GameState>, pid: PlayerId) -> Option<i32> {
    state.query_player(pid, |ecs, entity| {
        ecs.get::<crate::game::PlayerStats>(entity).and_then(|s| s.clan_id)
    }).flatten()
}

fn is_clan_owner(state: &Arc<GameState>, clan_id: i32, pid: PlayerId) -> bool {
    match state.db.get_clan(clan_id) {
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

fn append_close_buttons(buttons: &mut Vec<serde_json::Value>) {
    buttons.extend(
        CLOSE_WINDOW_BUTTON_LABELS
            .iter()
            .map(|label| serde_json::Value::String(label.to_string())),
    );
}
