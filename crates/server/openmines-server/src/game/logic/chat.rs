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
//! Чат: локальный, канальный, навигация (Cmen/Choo/Cset/Cpri), broadcast.
//! Навигации НЕТ в `server_reference` — спец по `docs/reference/CLIENT_PROTOCOL_GAPS.md`.
use crate::net::session::outbound::chat_sync::parse_private_tag;
use crate::net::session::prelude::*;
use std::sync::Arc;

fn send_chat_state_error(tx: &dyn PacketSink) {
    send_u_packet(tx, "OK", &ok_message("ЧАТ", "Состояние чата недоступно.").1);
}

pub struct PreparedChannelChat {
    pub db_tag: String,
    #[allow(dead_code)]
    pub wire_tag: String,
    pub text: String,
    pub nickname: String,
    pub user_id: i32,
    pub clan_id: i32,
    pub route: ChannelChatRoute,
    pub time: i64,
    pub color: i32,
}

#[derive(Debug, Clone)]
pub enum ChannelChatRoute {
    Global(String),
    Clan(i32),
    Private(String, [i32; 2]),
}

#[derive(Debug, PartialEq, Eq)]
pub enum ChinResync {
    Initial,
    Incremental { current: String, lastid: i64 },
}

pub fn parse_chin_resync_payload(payload: &str) -> Option<ChinResync> {
    let payload = payload.trim();
    if payload == "_" {
        return Some(ChinResync::Initial);
    }

    let rest = payload.strip_prefix("1:")?;
    let (current, lasts) = rest.split_once(':')?;
    let current = current.trim();
    if current.is_empty() {
        return None;
    }
    if lasts.is_empty() {
        return Some(ChinResync::Incremental {
            current: current.to_string(),
            lastid: -1,
        });
    }

    let parts: Vec<&str> = lasts.split('#').collect();
    if !parts.len().is_multiple_of(2) {
        return None;
    }

    let mut lastid = -1;
    for pair in parts.chunks_exact(2) {
        let tag = pair[0].trim();
        let id = pair[1].trim();
        if tag.is_empty() || id.is_empty() {
            return None;
        }
        let parsed_id = id.parse::<i64>().ok()?;
        if tag == current {
            lastid = parsed_id;
        }
    }

    Some(ChinResync::Incremental {
        current: current.to_string(),
        lastid,
    })
}

pub fn handle_local_chat_non_command(
    state: &Arc<GameState>,
    tx: &dyn PacketSink,
    pid: PlayerId,
    msg: &str,
) -> bool {
    let Some(window_open) = state.query_player_opt(pid, |ecs, entity| {
        let Some(ui) = ecs.get::<crate::game::player::PlayerUI>(entity) else {
            tracing::error!(player_id = %pid, component = "PlayerUI", "Player component missing for local chat");
            return None;
        };
        Some(ui.current_window.is_some())
    }) else {
        send_chat_state_error(tx);
        return true;
    };
    if window_open {
        return true;
    }
    if msg == "console" || (msg.starts_with('>') && msg.len() > 1) {
        return true;
    }
    let msg = msg.trim();
    if msg.is_empty() {
        return true;
    }
    if msg.starts_with('/') {
        return false;
    }
    broadcast_player_chat(state, tx, pid, msg);
    true
}

fn broadcast_player_chat(state: &Arc<GameState>, tx: &dyn PacketSink, pid: PlayerId, msg: &str) {
    let data = state.query_player_opt(pid, |ecs: &bevy_ecs::prelude::World, entity| {
        let Some(pos) = ecs.get::<crate::game::player::PlayerPosition>(entity) else {
            tracing::error!(player_id = %pid, component = "PlayerPosition", "Player component missing for local chat");
            return None;
        };
        Some((pos.x, pos.y))
    });

    let Some((px, py)) = data else {
        send_chat_state_error(tx);
        return;
    };

    let chat_sub = hb_chat(
        net_u16_nonneg(pid),
        net_u16_nonneg(px),
        net_u16_nonneg(py),
        msg,
    );
    // Delivery must stay out of simulation dispatch. The side phase batches
    // nearby HB frames and preserves their ordering barriers.
    state.queue_hb_at(px, py, &[chat_sub], None);
}

pub fn prepare_channel_chat_non_command(
    state: &Arc<GameState>,
    tx: &dyn PacketSink,
    pid: PlayerId,
    text: &str,
) -> Option<PreparedChannelChat> {
    if text.trim().is_empty() || text.trim().starts_with('/') {
        return None;
    }
    let p_data = state.query_player_opt(pid, |ecs: &bevy_ecs::prelude::World, entity| {
        let Some(meta) = ecs.get::<crate::game::player::PlayerMetadata>(entity) else {
            tracing::error!(player_id = %pid, component = "PlayerMetadata", "Player component missing for channel chat");
            return None;
        };
        let Some(pstats) = ecs.get::<crate::game::player::PlayerStats>(entity) else {
            tracing::error!(player_id = %pid, component = "PlayerStats", "Player component missing for channel chat");
            return None;
        };
        let Some(ui) = ecs.get::<crate::game::player::PlayerUI>(entity) else {
            tracing::error!(player_id = %pid, component = "PlayerUI", "Player component missing for channel chat");
            return None;
        };
        let Some(settings) = ecs.get::<crate::game::player::PlayerSettings>(entity) else {
            tracing::error!(player_id = %pid, component = "PlayerSettings", "Player component missing for channel chat");
            return None;
        };
        Some((
            meta.name.clone(),
            meta.id,
            pstats.clan_id,
            ui.current_chat.clone(),
            settings.cc,
        ))
    });

    let Some((nickname, my_id, clan_opt, channel_tag, chat_color)) = p_data else {
        send_chat_state_error(tx);
        return None;
    };

    // ⚠ Граница безопасности (клиент не доверенный; `current_chat`
    // ставится только `send_enter_channel`, но клан мог измениться, а
    // защита приватных обязательна). Проверяем ПО АКТУАЛЬНОМУ состоянию
    // ДО записи/рассылки. См. docs/reference/CLIENT_PROTOCOL_GAPS.md.
    let is_global = state
        .chat_channels
        .read()
        .iter()
        .any(|c| c.tag == channel_tag);
    let is_clan = channel_tag == "CLAN";
    let priv_ids = parse_private_tag(&channel_tag);
    let allowed = is_global
        || (is_clan && clan_opt.is_some())
        || priv_ids.is_some_and(|(a, b)| my_id == a || my_id == b);
    if !allowed {
        tracing::warn!(
            player_id = %pid,
            internal_id = %my_id,
            chat_tag = channel_tag,
            "Chat post denied"
        );
        return None;
    }

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    // .NET-минуты (НЕ unix-секунды) — иначе клиент рисует не то время и
    // история ≠ live. Тот же helper, что и в истории (единый источник).
    let time = dotnet_epoch_minutes(now_secs);

    let db_tag = if is_clan {
        format!("CLAN_{}", clan_opt.unwrap_or(0))
    } else {
        channel_tag.clone()
    };
    let route = if is_global {
        ChannelChatRoute::Global(channel_tag.clone())
    } else if is_clan {
        ChannelChatRoute::Clan(clan_opt.unwrap_or(0))
    } else {
        let pair = priv_ids?;
        ChannelChatRoute::Private(channel_tag.clone(), pair.into())
    };

    Some(PreparedChannelChat {
        db_tag,
        wire_tag: channel_tag,
        text: text.trim().to_owned(),
        nickname,
        user_id: my_id.into(),
        clan_id: clan_opt.unwrap_or(0),
        route,
        time,
        color: chat_color,
    })
}

pub fn deliver_chat_fanout(
    state: &Arc<GameState>,
    route: &ChannelChatRoute,
    msg: &openmines_protocol::chat::ChatMessage,
) {
    match route {
        ChannelChatRoute::Global(wire_tag) => {
            {
                let mut channels = state.chat_channels.write();
                if let Some(ch) = channels.iter_mut().find(|c| c.tag == *wire_tag) {
                    ch.messages.push_back(msg.clone());
                    if ch.messages.len() > crate::game::logic::chat::CHAT_HISTORY_LIMIT {
                        ch.messages.pop_front();
                    }
                }
            }
            let pkt =
                crate::protocol::packets::chat_messages(wire_tag, std::slice::from_ref(msg)).1;
            crate::game::logic::chat::send_mu_to_all(state, &pkt);
        }
        ChannelChatRoute::Clan(clan_id) => {
            let pkt = crate::protocol::packets::chat_messages("CLAN", std::slice::from_ref(msg)).1;
            crate::game::logic::chat::send_mu_to_clan(state, &pkt, *clan_id);
        }
        ChannelChatRoute::Private(wire_tag, users) => {
            let pkt =
                crate::protocol::packets::chat_messages(wire_tag, std::slice::from_ref(msg)).1;
            crate::game::logic::chat::send_mu_to_users(state, &pkt, users);
        }
    }
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

/// TY `Chin` — РЕСИНК чата (НЕ no-op). Клиент `WorldInitScript.cs`:
/// первый вход → `Chin "_"`; реконнект → `Chin "1:cur:TAG#id#TAG#id…"`
/// (`getLasts()` = свои наибольшие id по каналам). Реф `Session.Chin`
/// ПУСТ (реф неполон) — контракт по клиенту. Текущий login шлёт `mO` + bounded
/// `mU`; этот обработчик остаётся источником полного/инкрементального resync
/// по `getLasts()` клиента.
/// `docs/reference/CLIENT_PROTOCOL_GAPS.md` §2.
///
/// - `"_"` (первый вход, History клиента пуста) → полная история текущего
///   канала (`mU`). `mO` уже прислан login'ом.
/// - `"1:cur:lasts"` (реконнект) → выставить `current_chat=cur`, `mO` +
///   `mU` ТОЛЬКО с `id > lastid[cur]` (инкремент; нет → −1 → полная).
///   Доступ к `cur` валидируется (`chat_access`); нет прав → drop.
fn send_mu_bytes(data: &[u8]) -> Vec<u8> {
    make_u_packet_bytes("mU", data)
}

/// Рассылка `mU` ВСЕМ активным (global, 1:1 C# `Chat.AddMessage`).
fn send_mu_to_all(state: &Arc<GameState>, data: &[u8]) {
    let pkt = send_mu_bytes(data);
    for pid in state.active_player_ids() {
        state.send_to_player(pid, pkt.clone());
    }
}

/// Рассылка `mU` только членам клана `clan_id`.
fn send_mu_to_clan(state: &Arc<GameState>, data: &[u8], clan_id: i32) {
    let pkt = send_mu_bytes(data);
    for pid in state.active_player_ids() {
        state.query_player(pid, |ecs: &bevy_ecs::prelude::World, entity| {
            if let Some(s) = ecs.get::<crate::game::player::PlayerStats>(entity)
                && s.clan_id == Some(clan_id)
            {
                state.send_to_player(pid, pkt.clone());
            }
        });
    }
}

/// Рассылка `mU` ТОЛЬКО указанным игрокам (приват — не утекает третьим).
fn send_mu_to_users(state: &Arc<GameState>, data: &[u8], user_ids: &[i32]) {
    let pkt = send_mu_bytes(data);
    for &uid in user_ids {
        state.send_to_player(uid.into(), pkt.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chin_initial_accepts_only_underscore() {
        assert_eq!(parse_chin_resync_payload("_"), Some(ChinResync::Initial));
        assert_eq!(parse_chin_resync_payload(""), None);
    }

    #[test]
    fn chin_incremental_requires_current_and_lasts_separator() {
        assert_eq!(
            parse_chin_resync_payload("1:FED:FED#10#DNO#3"),
            Some(ChinResync::Incremental {
                current: "FED".to_string(),
                lastid: 10,
            })
        );
        assert_eq!(
            parse_chin_resync_payload("1:FED:"),
            Some(ChinResync::Incremental {
                current: "FED".to_string(),
                lastid: -1,
            })
        );
        assert_eq!(parse_chin_resync_payload("1:FED"), None);
        assert_eq!(parse_chin_resync_payload("1::FED#10"), None);
    }

    #[test]
    fn chin_incremental_rejects_malformed_lasts() {
        assert_eq!(parse_chin_resync_payload("1:FED:FED"), None);
        assert_eq!(parse_chin_resync_payload("1:FED:FED#x"), None);
        assert_eq!(parse_chin_resync_payload("1:FED:#1"), None);
        assert_eq!(parse_chin_resync_payload("1:FED:FED#"), None);
    }
}
