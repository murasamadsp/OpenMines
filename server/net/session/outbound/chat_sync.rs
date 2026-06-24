use crate::game::player::PlayerStats;
use crate::net::session::prelude::*;
use std::sync::Arc;

/// Разбор тега приватного канала `_a_b` → `(a, b)` (id игроков, i32).
pub fn parse_private_tag(tag: &str) -> Option<(i32, i32)> {
    let rest = tag.strip_prefix('_')?;
    let mut it = rest.split('_');
    let a: i32 = it.next()?.parse().ok()?;
    let b: i32 = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((a, b))
}

async fn load_db_history(state: &Arc<GameState>, db_tag: &str) -> Vec<ChatMessage> {
    state
        .db
        .get_recent_chat_messages(db_tag, CHAT_HISTORY_LIMIT)
        .await
        .map(|msgs| {
            msgs.into_iter()
                .map(
                    |(id, name, text, ts, player_id, color, clan_id)| ChatMessage {
                        id,
                        time: dotnet_epoch_minutes(ts),
                        clan_id,
                        user_id: player_id,
                        nickname: name,
                        text,
                        color,
                    },
                )
                .collect()
        })
        .unwrap_or_default()
}

/// Граница безопасности + резолв канала. `Some((display_name, history))`
pub async fn chat_access(
    state: &Arc<GameState>,
    pid: PlayerId,
    tag: &str,
) -> Option<(String, Vec<ChatMessage>)> {
    let (my_id, clan_id) = state.query_player_opt(pid, |w, e| {
        let m = w.get::<crate::game::player::PlayerMetadata>(e)?;
        let s = w.get::<PlayerStats>(e)?;
        Some((m.id, s.clan_id))
    })?;

    {
        let channels = state.chat_channels.read();
        if let Some(c) = channels.iter().find(|c| c.tag == tag) {
            return Some((c.name.clone(), c.messages.iter().cloned().collect()));
        }
    }

    if tag == "CLAN" {
        let cid = clan_id?;
        let name = state
            .db
            .get_clan(cid)
            .await
            .ok()
            .flatten()
            .map(|c| c.name)
            .unwrap_or_else(|| "Клан".to_string());
        return Some((name, load_db_history(state, &format!("CLAN_{cid}")).await));
    }

    if let Some((a, b)) = parse_private_tag(tag) {
        if my_id != a && my_id != b {
            return None;
        }
        let other = if my_id == a { b } else { a };
        let name = state
            .db
            .get_player_by_id(other)
            .await
            .ok()
            .flatten()
            .map(|p| p.name)
            .unwrap_or_else(|| format!("#{other}"));
        return Some((name, load_db_history(state, tag).await));
    }

    None
}

/// Войти/переключить канал (`Choo`/`Cpri`): валидирует доступ, ставит
/// `current_chat`, шлёт ТОЛЬКО `mO`+`mU`
pub async fn send_enter_channel(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    tag: &str,
) {
    let Some((name, history)) = chat_access(state, pid, tag).await else {
        tracing::warn!("[chat] enter denied pid={pid} tag={tag}");
        return;
    };
    state.modify_player(pid, |w, e| {
        if let Some(mut ui) = w.get_mut::<crate::game::player::PlayerUI>(e) {
            ui.current_chat = tag.to_string();
        }
        Some(())
    });
    send_u_packet(tx, "mO", &chat_current(tag, &name).1);
    send_u_packet(tx, "mU", &chat_messages(tag, &history).1);
}

/// Список каналов (`Cmen`): `mL` + `mN`
pub async fn send_channel_list(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let (my_id, clan_id) = match state.query_player_opt(pid, |w, e| {
        let m = w.get::<crate::game::player::PlayerMetadata>(e)?;
        let s = w.get::<PlayerStats>(e)?;
        Some((m.id, s.clan_id))
    }) {
        Some(v) => v,
        None => return,
    };

    let mut entries: Vec<(String, bool, String, String)> = {
        let channels = state.chat_channels.read();
        channels
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

    if let Some(cid) = clan_id {
        let clan_name = state
            .db
            .get_clan(cid)
            .await
            .ok()
            .flatten()
            .map(|c| c.name)
            .unwrap_or_else(|| "Клан".to_string());
        let preview = state
            .db
            .get_recent_chat_messages(&format!("CLAN_{cid}"), 1)
            .await
            .ok()
            .and_then(|m| m.first().map(|(_, n, t, ..)| format!("{n}: {t}")))
            .unwrap_or_default();
        entries.push(("CLAN".to_string(), false, clan_name, preview));
    }

    if let Ok(tags) = state.db.private_chat_tags(my_id).await {
        for t in tags {
            let Some((a, b)) = parse_private_tag(&t) else {
                continue;
            };
            let other = if my_id == a { b } else { a };
            let oname = state
                .db
                .get_player_by_id(other)
                .await
                .ok()
                .flatten()
                .map(|p| p.name)
                .unwrap_or_else(|| format!("#{other}"));
            let preview = state
                .db
                .get_recent_chat_messages(&t, 1)
                .await
                .ok()
                .and_then(|m| m.first().map(|(_, n, t, ..)| format!("{n}: {t}")))
                .unwrap_or_default();
            entries.push((t.clone(), false, oname, preview));
        }
    }

    send_u_packet(tx, "mL", &chat_list(&entries).1);
    send_u_packet(tx, "mN", &chat_notification(0).1);
}
