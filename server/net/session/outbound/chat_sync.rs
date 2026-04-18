use crate::net::session::prelude::*;
use std::collections::VecDeque;
use std::sync::Arc;

/// mO + mL + mU + mN для канала при логине или смене канала.
pub fn send_chat_init(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    channel_tag: &str,
) {
    let channels = state.chat_channels.read();

    let target = channels.iter().find(|c| c.tag == channel_tag);
    let (name, msgs_snapshot) = if let Some(ch) = target {
        (
            ch.name.clone(),
            ch.messages.iter().cloned().collect::<Vec<_>>(),
        )
    } else if channel_tag == "CLAN" {
        // Special case for dynamic clan channel
        if let Some(p) = state.active_players.get(&pid) {
            if let Some(cid) = p.data.clan_id {
                let clan_name = state
                    .db
                    .get_clan(cid)
                    .ok()
                    .flatten()
                    .map(|c| c.name)
                    .unwrap_or_else(|| "Клан".to_string());
                // For history, we might want to load it from DB too, but let's see.
                // Global list doesn't have CLAN history because it's dynamic.
                // But we can load it from DB here.
                let mut history = VecDeque::new();
                if let Ok(msgs) = state
                    .db
                    .get_recent_chat_messages(&format!("CLAN_{cid}"), 50)
                {
                    for (name, text, ts) in msgs {
                        history.push_back(ChatMessage {
                            time: ts / 60,
                            clan_id: cid,
                            user_id: 0,
                            nickname: name,
                            text,
                            color: 1,
                        });
                    }
                }
                (clan_name, history.into_iter().collect::<Vec<_>>())
            } else {
                return;
            }
        } else {
            return;
        }
    } else {
        return;
    };

    send_u_packet(tx, "mO", &chat_current(channel_tag, &name).1);

    let player_clan_id = state.active_players.get(&pid).and_then(|p| p.data.clan_id);

    let mut entries: Vec<(String, bool, String, String)> = channels
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
        .collect();

    if let Some(cid) = player_clan_id {
        let clan_name = state
            .db
            .get_clan(cid)
            .ok()
            .flatten()
            .map(|c| c.name)
            .unwrap_or_else(|| "Клан".to_string());

        let preview = state
            .db
            .get_recent_chat_messages(&format!("CLAN_{cid}"), 1)
            .ok()
            .and_then(|m| m.first().map(|(n, t, _)| format!("{n}: {t}")))
            .unwrap_or_default();

        entries.push(("CLAN".to_string(), false, clan_name, preview));
    }

    drop(channels);
    send_u_packet(tx, "mL", &chat_list(&entries).1);

    send_u_packet(tx, "mN", &chat_notification(0).1);

    send_u_packet(tx, "mU", &chat_messages(channel_tag, &msgs_snapshot).1);
}
