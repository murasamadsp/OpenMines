//! Чат: локальный, канальный, навигация (Cmen/Choo/Cset/Cpri), broadcast.
//! Навигации НЕТ в `server_reference` — спец по `docs/CLIENT_PROTOCOL_GAPS.md`.
use crate::net::session::outbound::chat_sync::{
    chat_access, parse_private_tag, send_channel_list, send_enter_channel,
};
use crate::net::session::prelude::*;
use crate::net::session::social::commands::handle_chat_command;

fn send_chat_state_error(tx: &mpsc::UnboundedSender<Vec<u8>>) {
    send_u_packet(tx, "OK", &ok_message("ЧАТ", "Состояние чата недоступно.").1);
}

#[derive(Debug, PartialEq, Eq)]
enum ChinResync<'a> {
    Initial,
    Incremental { current: &'a str, lastid: i64 },
}

fn parse_chin_resync_payload(payload: &str) -> Option<ChinResync<'_>> {
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
            current,
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

    Some(ChinResync::Incremental { current, lastid })
}

pub async fn handle_local_chat(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    msg: &str,
) {
    let Some(window_open) = state.query_player_opt(pid, |ecs, entity| {
        let Some(ui) = ecs.get::<crate::game::player::PlayerUI>(entity) else {
            tracing::error!(player_id = %pid, component = "PlayerUI", "Player component missing for local chat");
            return None;
        };
        Some(ui.current_window.is_some())
    }) else {
        send_chat_state_error(tx);
        return;
    };
    if window_open {
        return;
    }
    if msg == "console" || (msg.starts_with('>') && msg.len() > 1) {
        return;
    }
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
    broadcast_player_chat(state, tx, pid, msg);
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

fn broadcast_player_chat(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    msg: &str,
) {
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
    state.broadcast_hb_at(px, py, &[chat_sub], None);
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
        Some((
            meta.name.clone(),
            meta.id,
            pstats.clan_id,
            ui.current_chat.clone(),
        ))
    });

    let Some((nickname, my_id, clan_opt, channel_tag)) = p_data else {
        send_chat_state_error(tx);
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
            player_id = %pid,
            internal_id = %my_id,
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
        .add_chat_message(&db_tag, &nickname, &text, my_id.into())
        .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(chat_tag = db_tag, player_id = %pid, error = ?e, "Failed to add chat message in database");
            send_u_packet(tx, "OK", &ok_message("Ошибка", "Ошибка БД").1);
            return;
        }
    };
    // Один экземпляр: live-рассылка == in-mem-копия == будущая история
    // (тот же id/time/color/user_id/clan_id).
    let msg = ChatMessage {
        id: msg_id,
        time,
        clan_id: clan_opt.unwrap_or(0),
        user_id: my_id.into(),
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
    let Some(cur_default) = state.query_player_opt(pid, |w, e| {
        let Some(ui) = w.get::<crate::game::player::PlayerUI>(e) else {
            tracing::error!(player_id = %pid, component = "PlayerUI", "Player component missing for chat resync");
            return None;
        };
        Some(ui.current_chat.clone())
    }) else {
        send_chat_state_error(tx);
        return;
    };

    let Some(request) = parse_chin_resync_payload(&s) else {
        tracing::warn!(player_id = %pid, payload = %s, "Malformed Chin payload");
        return;
    };

    let ChinResync::Incremental {
        current: cur,
        lastid,
    } = request
    else {
        match chat_access(state, pid, &cur_default).await {
            Ok(Some((_, hist))) => send_u_packet(tx, "mU", &chat_messages(&cur_default, &hist).1),
            Ok(None) => {}
            Err(e) => {
                tracing::error!(player_id = %pid, chat_tag = cur_default, error = ?e, "Chat resync failed");
                send_u_packet(
                    tx,
                    "OK",
                    &ok_message("ЧАТ", "Не удалось прочитать данные чата.").1,
                );
            }
        }
        return;
    };

    let (name, hist) = match chat_access(state, pid, cur).await {
        Ok(Some(access)) => access,
        Ok(None) => {
            tracing::warn!(player_id = %pid, chat_tag = cur, "Chin resync denied");
            return;
        }
        Err(e) => {
            tracing::error!(player_id = %pid, chat_tag = cur, error = ?e, "Chin resync failed");
            send_u_packet(
                tx,
                "OK",
                &ok_message("ЧАТ", "Не удалось прочитать данные чата.").1,
            );
            return;
        }
    };
    let updated = state
        .modify_player(pid, |w, e| {
            let Some(mut ui) = w.get_mut::<crate::game::player::PlayerUI>(e) else {
                tracing::error!(player_id = %pid, component = "PlayerUI", "Player component missing while applying chat resync");
                return None;
            };
            ui.current_chat = cur.to_string();
            Some(())
        })
        .is_some();
    if !updated {
        send_chat_state_error(tx);
        return;
    }
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
    match state.db.cycle_chat_color(pid.as_i32()).await {
        Ok(code) => send_u_packet(tx, "mC", &chat_color(code).1),
        Err(e) => {
            tracing::error!(player_id = %pid, error = ?e, "Failed to cycle chat color");
            send_u_packet(tx, "OK", &ok_message("Ошибка", "Ошибка БД").1);
        }
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
    match state.db.get_player_by_id(uid).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            tracing::warn!(
                target_uid = uid,
                player_id = %pid,
                "Private chat request to unknown user ID"
            );
            return;
        }
        Err(e) => {
            tracing::error!(target_uid = uid, player_id = %pid, error = ?e, "Failed to load private chat target");
            send_u_packet(tx, "OK", &ok_message("Ошибка", "Ошибка БД").1);
            return;
        }
    }
    let uid: PlayerId = uid.into();
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
    for pid in state.active_player_ids() {
        state.query_player(pid, |ecs: &bevy_ecs::prelude::World, entity| {
            if let Some(c) = ecs.get::<crate::game::player::PlayerConnection>(entity) {
                let _ = c.tx.send(pkt.clone());
            }
        });
    }
}

/// Рассылка `mU` только членам клана `clan_id`.
fn send_mu_to_clan(state: &Arc<GameState>, data: &[u8], clan_id: i32) {
    let pkt = send_mu_bytes(data);
    for pid in state.active_player_ids() {
        state.query_player(pid, |ecs: &bevy_ecs::prelude::World, entity| {
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
        state.query_player(uid.into(), |ecs: &bevy_ecs::prelude::World, entity| {
            if let Some(c) = ecs.get::<crate::game::player::PlayerConnection>(entity) {
                let _ = c.tx.send(pkt.clone());
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::mpsc::UnboundedReceiver;

    struct TestState {
        state: Arc<GameState>,
        player: crate::db::PlayerRow,
        world_name: String,
        db_path: std::path::PathBuf,
    }

    impl TestState {
        fn cleanup(&self) {
            let dir = std::env::temp_dir();
            let _ = std::fs::remove_file(&self.db_path);
            let _ = std::fs::remove_file(self.db_path.with_extension("db-wal"));
            let _ = std::fs::remove_file(self.db_path.with_extension("db-shm"));
            let _ = std::fs::remove_file(dir.join(format!("{}_v2.map", self.world_name)));
            let _ = std::fs::remove_file(dir.join(format!("{}_durability.map", self.world_name)));
        }
    }

    async fn make_test_state(label: &str) -> TestState {
        let dir = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_path = dir.join(format!("{label}_{}_{}.db", std::process::id(), nonce));
        let _ = std::fs::remove_file(&db_path);
        let database = crate::db::Database::open(&db_path).await.unwrap();
        let player = database.create_player("chat-user", "p", "h").await.unwrap();

        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();
        let world_name = format!("{label}_world_{}_{}", std::process::id(), nonce);
        let world = crate::world::World::new(&world_name, 2, 2, cell_defs, &dir).unwrap();
        let config = crate::config::Config {
            world_name: world_name.clone(),
            port: 8090,
            world_chunks_w: 2,
            world_chunks_h: 2,
            data_dir: dir.to_string_lossy().to_string(),
            logging: crate::config::LoggingConfig::runtime_baseline(),
            cron: crate::config::CronConfig::runtime_baseline(),
            gameplay: crate::config::GameplayConfig::runtime_baseline(),
        };
        let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config)
            .await
            .unwrap();

        TestState {
            state,
            player,
            world_name,
            db_path,
        }
    }

    fn drain_events(rx: &mut UnboundedReceiver<Vec<u8>>) -> Vec<(String, Vec<u8>)> {
        let mut events = Vec::new();
        while let Ok(frame) = rx.try_recv() {
            let mut buf = BytesMut::from(&frame[..]);
            let packet = crate::protocol::Packet::try_decode(&mut buf)
                .expect("valid packet")
                .expect("decoded packet");
            events.push((packet.event_str().to_owned(), packet.payload.to_vec()));
        }
        events
    }

    fn single_hb_chat_text(payload: &[u8]) -> &str {
        assert_eq!(payload[0], b'C');
        let len = u16::from_le_bytes([payload[7], payload[8]]) as usize;
        std::str::from_utf8(&payload[9..9 + len]).unwrap()
    }

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
                current: "FED",
                lastid: 10,
            })
        );
        assert_eq!(
            parse_chin_resync_payload("1:FED:"),
            Some(ChinResync::Incremental {
                current: "FED",
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

    #[tokio::test]
    async fn local_chat_missing_position_is_explicit_error_not_silent_drop() {
        let test = make_test_state("local_chat_missing_position").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerPosition>();
        }

        handle_local_chat(&test.state, &tx, pid, "hello").await;

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние чата недоступно."));

        test.cleanup();
    }

    #[tokio::test]
    async fn local_chat_hb_bubble_uses_raw_message_not_name_prefix() {
        let test = make_test_state("local_chat_raw_bubble").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        handle_local_chat(&test.state, &tx, PlayerId(test.player.id), "5:hi").await;

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "HB");
        assert_eq!(single_hb_chat_text(&events[0].1), "5:hi");

        test.cleanup();
    }

    #[tokio::test]
    async fn local_chat_with_open_window_sends_no_hb_bubble() {
        let test = make_test_state("local_chat_open_window").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.get_mut::<crate::game::player::PlayerUI>(entity)
                .unwrap()
                .current_window = Some("pack:1:1".to_string());
        }

        handle_local_chat(&test.state, &tx, pid, "hello").await;

        let events = drain_events(&mut rx);
        assert!(events.is_empty());

        test.cleanup();
    }

    #[tokio::test]
    async fn local_chat_console_payloads_send_no_hb_bubble() {
        let test = make_test_state("local_chat_console_payloads").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        handle_local_chat(&test.state, &tx, pid, "console").await;
        handle_local_chat(&test.state, &tx, pid, ">status").await;

        let events = drain_events(&mut rx);
        assert!(events.is_empty());

        test.cleanup();
    }

    #[tokio::test]
    async fn channel_chat_missing_stats_is_explicit_error_not_access_denied_noop() {
        let test = make_test_state("channel_chat_missing_stats").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerStats>();
        }

        handle_channel_chat(&test.state, &tx, pid, b"hello").await;

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние чата недоступно."));

        test.cleanup();
    }

    #[tokio::test]
    async fn chin_initial_missing_ui_is_explicit_error_not_fed_fallback() {
        let test = make_test_state("chin_missing_ui").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerUI>();
        }

        handle_chat_resync(&test.state, &tx, pid, b"_").await;

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние чата недоступно."));

        test.cleanup();
    }
}
