//! Чат: локальный, канальный, навигация (Cmen/Choo/Cset/Cpri), broadcast.
//! Навигации НЕТ в `server_reference` — спец по `docs/CLIENT_PROTOCOL_GAPS.md`.
use crate::net::session::outbound::chat_sync::{
    chat_access, parse_private_tag, send_channel_list, send_enter_channel,
};
use crate::net::session::prelude::*;
use crate::net::session::social::commands::handle_chat_command;

pub async fn handle_local_chat(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    msg: &str,
) {
    handle_chat_text(state, tx, pid, msg).await;
}

// TODO: will be used when chat message dispatch is fully wired to session
#[allow(dead_code)]
pub async fn handle_chat_message(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    payload: &[u8],
) {
    let msg = String::from_utf8_lossy(payload).to_string();
    handle_chat_text(state, tx, pid, &msg).await;
}

async fn handle_chat_text(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    msg: &str,
) {
    let msg = msg.trim();
    if msg.is_empty() {
        return;
    }
    if handle_chat_command_if_present(state, tx, pid, msg).await {
        return;
    }
    broadcast_player_chat(state, pid, msg);
}

async fn handle_chat_command_if_present(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    msg: &str,
) -> bool {
    let msg = msg.trim();
    if !msg.starts_with('/') {
        return false;
    }
    handle_chat_command(state, tx, pid, msg).await;
    true
}

fn broadcast_player_chat(state: &Arc<GameState>, pid: PlayerId, msg: &str) {
    let data = state.query_player_opt(pid, |ecs: &bevy_ecs::prelude::World, entity| {
        let pos = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
        let meta = ecs.get::<crate::game::player::PlayerMetadata>(entity)?;
        Some((pos.x, pos.y, meta.name.clone()))
    });

    if let Some((px, py, name)) = data {
        let text = format!("{name}: {msg}");
        let chat_sub = hb_chat(
            net_u16_nonneg(pid),
            net_u16_nonneg(px),
            net_u16_nonneg(py),
            &text,
        );
        state.broadcast_hb_at(px, py, &[chat_sub], None);
    }
}

pub async fn handle_channel_chat(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    payload: &[u8],
) {
    let text = extract_channel_message_text(payload);
    if text.is_empty() {
        return;
    }
    if handle_chat_command_if_present(state, tx, pid, &text).await {
        return;
    }

    let p_data = state.query_player_opt(pid, |ecs: &bevy_ecs::prelude::World, entity| {
        let meta = ecs.get::<crate::game::player::PlayerMetadata>(entity)?;
        let pstats = ecs.get::<crate::game::player::PlayerStats>(entity)?;
        let ui = ecs.get::<crate::game::player::PlayerUI>(entity)?;
        Some((
            meta.name.clone(),
            meta.id,
            pstats.clan_id,
            ui.current_chat.clone(),
        ))
    });

    let Some((nickname, my_id, clan_opt, channel_tag)) = p_data else {
        return;
    };

    // ⚠ Граница безопасности (клиент не доверенный; `current_chat`
    // ставится только `send_enter_channel`, но клан мог измениться, а
    // защита приватных обязательна). Проверяем ПО АКТУАЛЬНОМУ состоянию
    // ДО записи/рассылки. См. docs/CLIENT_PROTOCOL_GAPS.md (безопасность).
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
            player_id = pid,
            internal_id = my_id,
            chat_tag = channel_tag,
            "Chat post denied"
        );
        return;
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
    // Вставляем в БД ПЕРЕД сборкой пакета: rowid становится `GCMessage.id`
    // клиента (дедуп `LastIDs`). 1:1 с C# `Chat.AddMessage`. `add_chat_message`
    // фиксирует и возвращает `color` (снимок `chat_color` автора) — live,
    // in-mem и история несут ОДИН цвет (реф-баг 1/10: один message → разный
    // цвет live и при перезагрузке). CLIENT_PROTOCOL_GAPS.md §1.
    let (msg_id, color) = match state
        .db
        .add_chat_message(&db_tag, &nickname, &text, my_id)
        .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(chat_tag = db_tag, player_id = pid, error = ?e, "Failed to add chat message in database");
            (0, 10)
        }
    };
    // Один экземпляр: live-рассылка == in-mem-копия == будущая история
    // (тот же id/time/color/user_id/clan_id).
    let msg = ChatMessage {
        id: msg_id,
        time,
        clan_id: clan_opt.unwrap_or(0),
        user_id: my_id,
        nickname,
        text,
        color,
    };

    if is_global {
        {
            let mut channels = state.chat_channels.write();
            if let Some(ch) = channels.iter_mut().find(|c| c.tag == channel_tag) {
                ch.messages.push_back(msg.clone());
                if ch.messages.len() > CHAT_HISTORY_LIMIT {
                    ch.messages.pop_front();
                }
            }
        }
        // Wire-`ch` = РЕАЛЬНЫЙ channel_tag (FED→"FED", DNO→"DNO"). C#
        // `Chat.cs:44` хардкодит "FED" для ЛЮБОГО global — РЕФЕРЕНС-БАГ:
        // DNO-зритель (currentChat="DNO") не видел DNO, оно текло в FED
        // (репорт юзера). Клиент важнее. CLIENT_PROTOCOL_GAPS.md §1.
        let pkt = chat_messages(&channel_tag, &[msg]).1;
        send_mu_to_all(state, &pkt);
    } else if is_clan {
        let pkt = chat_messages("CLAN", &[msg]).1;
        send_mu_to_clan(state, &pkt, clan_opt.unwrap_or(0));
    } else if let Some(pair) = priv_ids {
        // Приват: wire-тег = сам `_a_b` (клиент обоих участников держит
        // `currentChat == _a_b`). Рассылка ТОЛЬКО {a,b} — НЕ всем
        // (утечка ЛС). docs/CLIENT_PROTOCOL_GAPS.md §6.
        let pkt = chat_messages(&channel_tag, &[msg]).1;
        let users: [i32; 2] = pair.into();
        send_mu_to_users(state, &pkt, &users);
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
/// ПУСТ (реф неполон) — контракт по клиенту. login шлёт только `mO`;
/// история — здесь, чтобы на реконнекте НЕ слать всё заново (клиент
/// `muHandler` `AddLine`'ит каждое сообщение пакета при `ch==currentChat`,
/// дедуп только словаря History, не визуала → дубли).
/// `docs/CLIENT_PROTOCOL_GAPS.md` §2.
///
/// - `"_"` (первый вход, History клиента пуста) → полная история текущего
///   канала (`mU`). `mO` уже прислан login'ом.
/// - `"1:cur:lasts"` (реконнект) → выставить `current_chat=cur`, `mO` +
///   `mU` ТОЛЬКО с `id > lastid[cur]` (инкремент; нет → −1 → полная).
///   Доступ к `cur` валидируется (`chat_access`); нет прав → drop.
pub async fn handle_chat_resync(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    payload: &[u8],
) {
    let s = String::from_utf8_lossy(payload).trim().to_string();
    let cur_default = state
        .query_player_opt(pid, |w, e| {
            w.get::<crate::game::player::PlayerUI>(e)
                .map(|u| u.current_chat.clone())
        })
        .unwrap_or_else(|| "FED".to_string());

    if s.is_empty() || s == "_" {
        if let Some((_, hist)) = chat_access(state, pid, &cur_default).await {
            send_u_packet(tx, "mU", &chat_messages(&cur_default, &hist).1);
        }
        return;
    }

    let Some(rest) = s.strip_prefix("1:") else {
        return;
    };
    let (cur, lasts_str) = rest.split_once(':').unwrap_or((rest, ""));
    let cur = cur.trim();
    if cur.is_empty() {
        return;
    }
    // lasts: `TAG#id#TAG#id…`, порядок произволен (Dictionary клиента).
    let mut lastid: i64 = -1;
    let parts: Vec<&str> = lasts_str.split('#').collect();
    let mut i = 0;
    while i + 1 < parts.len() {
        if parts[i] == cur {
            if let Ok(v) = parts[i + 1].parse::<i64>() {
                lastid = v;
            }
        }
        i += 2;
    }

    let Some((name, hist)) = chat_access(state, pid, cur).await else {
        tracing::warn!(player_id = pid, chat_tag = cur, "Chin resync denied");
        return;
    };
    state.modify_player(pid, |w, e| {
        if let Some(mut ui) = w.get_mut::<crate::game::player::PlayerUI>(e) {
            ui.current_chat = cur.to_string();
        }
        Some(())
    });
    let fresh: Vec<ChatMessage> = hist.into_iter().filter(|m| m.id > lastid).collect();
    send_u_packet(tx, "mO", &chat_current(cur, &name).1);
    send_u_packet(tx, "mU", &chat_messages(cur, &fresh).1);
}

/// TY `Cmen` (`"_"`) — открыть список каналов. Клиент `ChatManager.cs:67`
/// `OnMenu`. Ждёт `mL`+`mN`. `docs/CLIENT_PROTOCOL_GAPS.md` §3.
pub async fn handle_chat_menu(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    _payload: &[u8],
) {
    send_channel_list(state, tx, pid).await;
}

/// TY `Choo <tag>` — войти/переключить канал. Клиент `ChatManager.cs:176`
/// (клик по каналу в `mL`). `send_enter_channel` валидирует доступ.
/// `docs/CLIENT_PROTOCOL_GAPS.md` §4.
pub async fn handle_chat_choose(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    payload: &[u8],
) {
    let tag = String::from_utf8_lossy(payload).trim().to_string();
    if tag.is_empty() {
        return;
    }
    send_enter_channel(state, tx, pid, &tag).await;
}

/// TY `Cset` (`"_"`) — циклически сменить цвет поля ввода чата. Клиент
/// `ChatManager.cs:60` `OnSettings`, ответ-обработчик `mcHandler`
/// (`short.Parse`). `docs/CLIENT_PROTOCOL_GAPS.md` §5.
pub async fn handle_chat_settings(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    _payload: &[u8],
) {
    match state.db.cycle_chat_color(pid).await {
        Ok(code) => send_u_packet(tx, "mC", &chat_color(code).1),
        Err(e) => tracing::warn!(player_id = pid, error = ?e, "Failed to cycle chat color"),
    }
}

/// TY `Cpri <userId>` — открыть ЛС с игроком. Клиент `ChatManager.cs:307`
/// (клик по строке сообщения → `message.gid`). Тег `_min_max` стабилен
/// для пары. Валидация: цель существует, не сам с собой.
/// `docs/CLIENT_PROTOCOL_GAPS.md` §6.
pub async fn handle_chat_private(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    payload: &[u8],
) {
    let Ok(uid) = String::from_utf8_lossy(payload).trim().parse::<i32>() else {
        return;
    };
    if uid == pid || uid <= 0 {
        return;
    }
    if !matches!(state.db.get_player_by_id(uid).await, Ok(Some(_))) {
        tracing::warn!(
            target_uid = uid,
            player_id = pid,
            "Private chat request to unknown user ID"
        );
        return;
    }
    let (lo, hi) = if pid < uid { (pid, uid) } else { (uid, pid) };
    let tag = format!("_{lo}_{hi}");
    send_enter_channel(state, tx, pid, &tag).await;
}

fn send_mu_bytes(data: &[u8]) -> Vec<u8> {
    make_u_packet_bytes("mU", data)
}

/// Рассылка `mU` ВСЕМ активным (global, 1:1 C# `Chat.AddMessage`).
fn send_mu_to_all(state: &Arc<GameState>, data: &[u8]) {
    let pkt = send_mu_bytes(data);
    for entry in &state.active_players {
        state.query_player(*entry.key(), |ecs: &bevy_ecs::prelude::World, entity| {
            if let Some(c) = ecs.get::<crate::game::player::PlayerConnection>(entity) {
                let _ = c.tx.send(pkt.clone());
            }
        });
    }
}

/// Рассылка `mU` только членам клана `clan_id`.
fn send_mu_to_clan(state: &Arc<GameState>, data: &[u8], clan_id: i32) {
    let pkt = send_mu_bytes(data);
    for entry in &state.active_players {
        state.query_player(*entry.key(), |ecs: &bevy_ecs::prelude::World, entity| {
            if let (Some(s), Some(c)) = (
                ecs.get::<crate::game::player::PlayerStats>(entity),
                ecs.get::<crate::game::player::PlayerConnection>(entity),
            ) {
                if s.clan_id == Some(clan_id) {
                    let _ = c.tx.send(pkt.clone());
                }
            }
        });
    }
}

/// Рассылка `mU` ТОЛЬКО указанным игрокам (приват — не утекает третьим).
fn send_mu_to_users(state: &Arc<GameState>, data: &[u8], user_ids: &[i32]) {
    let pkt = send_mu_bytes(data);
    for &uid in user_ids {
        state.query_player(uid, |ecs: &bevy_ecs::prelude::World, entity| {
            if let Some(c) = ecs.get::<crate::game::player::PlayerConnection>(entity) {
                let _ = c.tx.send(pkt.clone());
            }
        });
    }
}
