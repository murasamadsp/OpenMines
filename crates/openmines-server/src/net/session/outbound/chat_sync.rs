use crate::game::player::PlayerStats;
use crate::net::session::prelude::*;
use anyhow::{Result, bail};
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

async fn load_db_history(state: &Arc<GameState>, db_tag: &str) -> Result<Vec<ChatMessage>> {
    Ok(state
        .db
        .get_recent_chat_messages(db_tag, CHAT_HISTORY_LIMIT)
        .await?
        .into_iter()
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
        .collect())
}

async fn load_latest_preview(state: &Arc<GameState>, db_tag: &str) -> Result<String> {
    Ok(state
        .db
        .get_recent_chat_messages(db_tag, 1)
        .await?
        .first()
        .map(|(_, n, t, ..)| format!("{n}: {t}"))
        .unwrap_or_default())
}

async fn load_player_name(state: &Arc<GameState>, player_id: i32) -> Result<String> {
    let Some(player) = state.db.get_player_by_id(player_id).await? else {
        bail!("player {player_id} is missing");
    };
    Ok(player.name)
}

async fn load_clan_name(state: &Arc<GameState>, clan_id: i32) -> Result<String> {
    let Some(clan) = state.db.get_clan(clan_id).await? else {
        bail!("clan {clan_id} is missing");
    };
    Ok(clan.name)
}

fn send_chat_storage_error(tx: &Outbox) {
    send_u_packet(
        tx,
        "OK",
        &ok_message("ЧАТ", "Не удалось прочитать данные чата.").1,
    );
}

fn send_chat_state_error(tx: &Outbox) {
    send_u_packet(tx, "OK", &ok_message("ЧАТ", "Состояние чата недоступно.").1);
}

/// Граница безопасности + резолв канала. `Ok(Some((display_name, history)))`
pub async fn chat_access(
    state: &Arc<GameState>,
    pid: PlayerId,
    tag: &str,
) -> Result<Option<(String, Vec<ChatMessage>)>> {
    let Some((my_id, clan_id)) = state.query_player_opt(pid, |w, e| {
        let Some(m) = w.get::<crate::game::player::PlayerMetadata>(e) else {
            tracing::error!(player_id = %pid, component = "PlayerMetadata", "Player component missing for chat access");
            return None;
        };
        let Some(s) = w.get::<PlayerStats>(e) else {
            tracing::error!(player_id = %pid, component = "PlayerStats", "Player component missing for chat access");
            return None;
        };
        Some((m.id, s.clan_id))
    }) else {
        bail!("player chat state is missing");
    };

    {
        let channels = state.chat_channels.read();
        if let Some(c) = channels.iter().find(|c| c.tag == tag) {
            return Ok(Some((c.name.clone(), c.messages.iter().cloned().collect())));
        }
    }

    if tag == "CLAN" {
        let Some(cid) = clan_id else {
            return Ok(None);
        };
        return Ok(Some((
            load_clan_name(state, cid).await?,
            load_db_history(state, &format!("CLAN_{cid}")).await?,
        )));
    }

    if let Some((a, b)) = parse_private_tag(tag) {
        if my_id != a && my_id != b {
            return Ok(None);
        }
        let other = if my_id == a { b } else { a };
        return Ok(Some((
            load_player_name(state, other).await?,
            load_db_history(state, tag).await?,
        )));
    }

    Ok(None)
}

/// Войти/переключить канал (`Choo`/`Cpri`): валидирует доступ, ставит
/// `current_chat`, шлёт ТОЛЬКО `mO`+`mU`
pub async fn send_enter_channel(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, tag: &str) {
    let (name, history) = match chat_access(state, pid, tag).await {
        Ok(Some(access)) => access,
        Ok(None) => {
            tracing::warn!(player_id = %pid, chat_tag = tag, "Chat enter denied");
            return;
        }
        Err(e) => {
            tracing::error!(player_id = %pid, chat_tag = tag, error = ?e, "Chat enter failed");
            send_chat_storage_error(tx);
            return;
        }
    };
    let updated = state
        .modify_player(pid, |w, e| {
            let Some(mut ui) = w.get_mut::<crate::game::player::PlayerUI>(e) else {
                tracing::error!(player_id = %pid, component = "PlayerUI", "Player component missing while entering chat");
                return None;
            };
            ui.current_chat = tag.to_string();
            Some(())
        })
        .is_some();
    if !updated {
        send_chat_state_error(tx);
        return;
    }
    send_u_packet(tx, "mO", &chat_current(tag, &name).1);
    send_u_packet(tx, "mU", &chat_messages(tag, &history).1);
}

/// Список каналов (`Cmen`): `mL` + `mN`
pub async fn send_channel_list(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId) {
    let (my_id, clan_id) = match state.query_player_opt(pid, |w, e| {
        let Some(m) = w.get::<crate::game::player::PlayerMetadata>(e) else {
            tracing::error!(player_id = %pid, component = "PlayerMetadata", "Player component missing for chat menu");
            return None;
        };
        let Some(s) = w.get::<PlayerStats>(e) else {
            tracing::error!(player_id = %pid, component = "PlayerStats", "Player component missing for chat menu");
            return None;
        };
        Some((m.id, s.clan_id))
    }) {
        Some(v) => v,
        None => {
            send_chat_state_error(tx);
            return;
        }
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
        let clan_name = match load_clan_name(state, cid).await {
            Ok(name) => name,
            Err(e) => {
                tracing::error!(player_id = %pid, clan_id = cid, error = ?e, "Failed to load clan chat name");
                send_chat_storage_error(tx);
                return;
            }
        };
        let preview = match load_latest_preview(state, &format!("CLAN_{cid}")).await {
            Ok(preview) => preview,
            Err(e) => {
                tracing::error!(player_id = %pid, clan_id = cid, error = ?e, "Failed to load clan chat preview");
                send_chat_storage_error(tx);
                return;
            }
        };
        entries.push(("CLAN".to_string(), false, clan_name, preview));
    }

    let tags = match state.db.private_chat_tags(my_id.into()).await {
        Ok(tags) => tags,
        Err(e) => {
            tracing::error!(player_id = %pid, error = ?e, "Failed to load private chat tags");
            send_chat_storage_error(tx);
            return;
        }
    };
    for t in tags {
        let Some((a, b)) = parse_private_tag(&t) else {
            tracing::error!(player_id = %pid, chat_tag = t, "Invalid private chat tag in database");
            send_chat_storage_error(tx);
            return;
        };
        let other = if my_id == a { b } else { a };
        let oname = match load_player_name(state, other).await {
            Ok(name) => name,
            Err(e) => {
                tracing::error!(player_id = %pid, other_id = other, error = ?e, "Failed to load private chat name");
                send_chat_storage_error(tx);
                return;
            }
        };
        let preview = match load_latest_preview(state, &t).await {
            Ok(preview) => preview,
            Err(e) => {
                tracing::error!(player_id = %pid, chat_tag = t, error = ?e, "Failed to load private chat preview");
                send_chat_storage_error(tx);
                return;
            }
        };
        entries.push((t.clone(), false, oname, preview));
    }

    send_u_packet(tx, "mL", &chat_list(&entries).1);
    send_u_packet(tx, "mN", &chat_notification(0).1);
}
