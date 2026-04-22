//! Чат: локальный, канальный, переключение, broadcast.
use crate::net::session::outbound::chat_sync::send_chat_init;
use crate::net::session::prelude::*;
use crate::net::session::social::commands::handle_chat_command;

pub fn handle_local_chat(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    msg: &str,
) {
    handle_chat_text(state, tx, pid, msg);
}

// TODO: will be used when chat message dispatch is fully wired to session
#[allow(dead_code)]
pub fn handle_chat_message(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    payload: &[u8],
) {
    let msg = String::from_utf8_lossy(payload).to_string();
    handle_chat_text(state, tx, pid, &msg);
}

fn handle_chat_text(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    msg: &str,
) {
    let msg = msg.trim();
    if msg.is_empty() {
        return;
    }
    if handle_chat_command_if_present(state, tx, pid, msg) {
        return;
    }
    broadcast_player_chat(state, pid, msg);
}

fn handle_chat_command_if_present(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    msg: &str,
) -> bool {
    let msg = msg.trim();
    if !msg.starts_with('/') {
        return false;
    }
    handle_chat_command(state, tx, pid, msg);
    true
}

fn broadcast_player_chat(state: &Arc<GameState>, pid: PlayerId, msg: &str) {
    let data = state
        .query_player(pid, |ecs: &bevy_ecs::prelude::World, entity| {
            let pos = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
            let meta = ecs.get::<crate::game::player::PlayerMetadata>(entity)?;
            Some((pos.x, pos.y, meta.name.clone()))
        })
        .flatten();

    if let Some((px, py, name)) = data {
        let text = format!("{name}: {msg}");
        let (cx, cy) = World::chunk_pos(px, py);
        let chat_sub = hb_chat(
            net_u16_nonneg(pid),
            net_u16_nonneg(px),
            net_u16_nonneg(py),
            &text,
        );
        state.broadcast_to_nearby(cx, cy, &encode_hb_bundle(&hb_bundle(&[chat_sub]).1), None);
    }
}

pub fn handle_channel_chat(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    payload: &[u8],
) {
    let text = extract_channel_message_text(payload);
    if text.is_empty() {
        return;
    }
    if handle_chat_command_if_present(state, tx, pid, &text) {
        return;
    }

    let p_data = state
        .query_player(pid, |ecs: &bevy_ecs::prelude::World, entity| {
            let meta = ecs.get::<crate::game::player::PlayerMetadata>(entity)?;
            let stats = ecs.get::<crate::game::player::PlayerStats>(entity)?;
            let ui = ecs.get::<crate::game::player::PlayerUI>(entity)?;
            Some((
                meta.name.clone(),
                meta.id,
                stats.clan_id.unwrap_or(0),
                ui.current_chat.clone(),
            ))
        })
        .flatten();

    let Some((nickname, user_id, clan_id, channel_tag)) = p_data else {
        return;
    };
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let msg = ChatMessage {
        time,
        clan_id,
        user_id,
        nickname: nickname.clone(),
        text: text.clone(),
        color: 1,
    };

    let db_tag = if channel_tag == "CLAN" {
        format!("CLAN_{clan_id}")
    } else {
        channel_tag.clone()
    };
    let _ = state.db.add_chat_message(&db_tag, &nickname, &text);

    let (is_global, packet_data) = {
        let mut channels = state.chat_channels.write();
        let target_ch = channels.iter_mut().find(|c| c.tag == channel_tag);
        if let Some(ch) = target_ch {
            ch.messages.push_back(msg.clone());
            if ch.messages.len() > 50 {
                ch.messages.pop_front();
            }
            (ch.global, chat_messages(&channel_tag, &[msg]).1)
        } else if channel_tag == "CLAN" && clan_id != 0 {
            (false, chat_messages("CLAN", &[msg]).1)
        } else {
            return;
        }
    };
    send_channel_packet_to_players(
        state,
        &packet_data,
        if is_global { None } else { Some(clan_id) },
    );
}

pub fn extract_channel_message_text(payload: &[u8]) -> String {
    let raw = String::from_utf8_lossy(payload).trim().to_string();
    let Some((prefix, body)) = raw.split_once('#') else {
        return raw;
    };
    if prefix.contains(':') {
        body.to_string()
    } else {
        raw
    }
}

// TODO: will be used when chat channel switching is fully wired to session dispatch
#[allow(dead_code)]
pub fn handle_chat_switch(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    payload: &[u8],
) {
    let tag = String::from_utf8_lossy(payload).trim().to_string();
    if tag.is_empty() {
        return;
    }
    if !state.chat_channels.read().iter().any(|c| c.tag == tag) {
        return;
    }
    state.modify_player(pid, |ecs: &mut bevy_ecs::prelude::World, entity| {
        if let Some(mut ui) = ecs.get_mut::<crate::game::player::PlayerUI>(entity) {
            ui.current_chat = tag.clone();
        }
        Some(())
    });
    send_chat_init(state, tx, pid, &tag);
}

/// TY `Chin` — запрос состояния чата (`"_"` или `1:TAG:…` по референсу).
pub fn handle_chat_init_ty(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    payload: &[u8],
) {
    let s = String::from_utf8_lossy(payload).trim().to_string();
    let current_tag = state
        .query_player(pid, |ecs, entity| {
            ecs.get::<crate::game::player::PlayerUI>(entity)
                .map(|ui| ui.current_chat.clone())
        })
        .flatten()
        .unwrap_or_else(|| "FED".to_string());

    if s.is_empty() || s == "_" {
        send_chat_init(state, tx, pid, &current_tag);
        return;
    }
    if let Some(rest) = s.strip_prefix("1:") {
        let tag = rest
            .split(':')
            .next()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .unwrap_or("")
            .to_string();
        if tag.is_empty() || !state.chat_channels.read().iter().any(|c| c.tag == tag) {
            send_chat_init(state, tx, pid, &current_tag);
            return;
        }
        state.modify_player(pid, |ecs, entity| {
            if let Some(mut ui) = ecs.get_mut::<crate::game::player::PlayerUI>(entity) {
                ui.current_chat = tag.clone();
            }
            Some(())
        });
        send_chat_init(state, tx, pid, &tag);
        return;
    }
    send_chat_init(state, tx, pid, &current_tag);
}

fn send_channel_packet_to_players(state: &Arc<GameState>, data: &[u8], clan: Option<i32>) {
    let pkt = make_u_packet_bytes("mU", data);
    for entry in &state.active_players {
        state.query_player(*entry.key(), |ecs: &bevy_ecs::prelude::World, entity| {
            if let (Some(s), Some(c)) = (
                ecs.get::<crate::game::player::PlayerStats>(entity),
                ecs.get::<crate::game::player::PlayerConnection>(entity),
            ) {
                if clan.is_none_or(|id| s.clan_id == Some(id)) {
                    let _ = c.tx.send(pkt.clone());
                }
            }
        });
    }
}
