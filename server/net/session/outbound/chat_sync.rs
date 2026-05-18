use crate::game::player::PlayerStats;
use crate::net::session::prelude::*;
use std::sync::Arc;

/// После логина — ТОЛЬКО `mO` (восстановить `currentChat` UI + триггер
/// `moHandler`: clear `ChatContainer` + re-render существующей History на
/// реконнекте). Историю шлёт `Chin`-resync (по `getLasts()` клиента),
/// НЕ login. Иначе на реконнекте login слал бы полную пере-отправку
/// `mU`, а клиент `muHandler` `AddLine`'ит КАЖДОЕ сообщение пакета при
/// `ch==currentChat` (дедуп — только History-словаря, НЕ визуала) →
/// визуальные дубли (репорт юзера). Реф `Player.SendChat` слал mO+mU,
/// `Chin` пуст — реф неполон, контракт по клиенту.
/// `docs/CLIENT_PROTOCOL_GAPS.md` §2.
pub fn send_chat_login_per_reference(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let tag = state
        .query_player(pid, |ecs, e| {
            ecs.get::<crate::game::player::PlayerUI>(e)
                .map(|u| u.current_chat.clone())
        })
        .flatten()
        .unwrap_or_else(|| "FED".to_string());
    if let Some((name, _history)) = chat_access(state, pid, &tag) {
        send_u_packet(tx, "mO", &chat_current(&tag, &name).1);
    }
}

/// Разбор тега приватного канала `_a_b` → `(a, b)` (id игроков, i32).
/// Формат: ведущий `_`, ровно две числовые части через `_`. Нет в
/// `server_reference` — см. `docs/CLIENT_PROTOCOL_GAPS.md` §6.
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

/// `clan_id`/`user_id`/`color`/`time` берутся ИЗ строки (`player_id`, цвет —
/// снимок на отправку; клан — `LEFT JOIN players`, динамический = 1:1 C#
/// `line.player.cid`). Раньше хардкодилось `user_id=0`/`color=1`/`ts/60`
/// → история ≠ live (мелкий шрифт, иной цвет/время).
/// См. `docs/CLIENT_PROTOCOL_GAPS.md` §1.
fn load_db_history(state: &Arc<GameState>, db_tag: &str) -> Vec<ChatMessage> {
    state
        .db
        .get_recent_chat_messages(db_tag, 50)
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
/// если игрок ВПРАВЕ видеть `tag`, иначе `None` (клиент не доверенный —
/// `Choo`/`Cpri` могут прислать любой tag). Правила: глобальные in-mem
/// (FED/DNO/LOC) — всегда; `CLAN` — только член клана; `_a_b` — только
/// участник. См. `docs/CLIENT_PROTOCOL_GAPS.md` (сводка безопасности).
pub fn chat_access(
    state: &Arc<GameState>,
    pid: PlayerId,
    tag: &str,
) -> Option<(String, Vec<ChatMessage>)> {
    let (my_id, clan_id) = state
        .query_player(pid, |w, e| {
            let m = w.get::<crate::game::player::PlayerMetadata>(e)?;
            let s = w.get::<PlayerStats>(e)?;
            Some((m.id, s.clan_id))
        })
        .flatten()?;

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
            .ok()
            .flatten()
            .map(|c| c.name)
            .unwrap_or_else(|| "Клан".to_string());
        return Some((name, load_db_history(state, &format!("CLAN_{cid}"))));
    }

    if let Some((a, b)) = parse_private_tag(tag) {
        if my_id != a && my_id != b {
            return None;
        }
        let other = if my_id == a { b } else { a };
        let name = state
            .db
            .get_player_by_id(other)
            .ok()
            .flatten()
            .map(|p| p.name)
            .unwrap_or_else(|| format!("#{other}"));
        return Some((name, load_db_history(state, tag)));
    }

    None
}

/// Войти/переключить канал (`Choo`/`Cpri`): валидирует доступ, ставит
/// `current_chat`, шлёт ТОЛЬКО `mO`+`mU` (НЕ `mL` — тот переводит клиент
/// в режим списка, `docs/CLIENT_PROTOCOL_GAPS.md` §2/§4). `mO` и `mU`
/// идут с ОДНИМ tag: клиент `moHandler` ставит `currentChat=tag`, а
/// `muHandler` показывает только `MuPacket.ch == currentChat`.
pub fn send_enter_channel(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    tag: &str,
) {
    let Some((name, history)) = chat_access(state, pid, tag) else {
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

/// Список каналов (`Cmen`): `mL` (глобальные + `CLAN` если в клане +
/// приватные игрока из БД) + `mN`. НЕ слать `mO`/`mU` — конфликт с
/// входом в канал. `docs/CLIENT_PROTOCOL_GAPS.md` §3.
pub fn send_channel_list(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let (my_id, clan_id) = match state
        .query_player(pid, |w, e| {
            let m = w.get::<crate::game::player::PlayerMetadata>(e)?;
            let s = w.get::<PlayerStats>(e)?;
            Some((m.id, s.clan_id))
        })
        .flatten()
    {
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
            .ok()
            .flatten()
            .map(|c| c.name)
            .unwrap_or_else(|| "Клан".to_string());
        let preview = state
            .db
            .get_recent_chat_messages(&format!("CLAN_{cid}"), 1)
            .ok()
            .and_then(|m| m.first().map(|(_, n, t, ..)| format!("{n}: {t}")))
            .unwrap_or_default();
        entries.push(("CLAN".to_string(), false, clan_name, preview));
    }

    if let Ok(tags) = state.db.private_chat_tags(my_id) {
        for t in tags {
            let Some((a, b)) = parse_private_tag(&t) else {
                continue;
            };
            let other = if my_id == a { b } else { a };
            let oname = state
                .db
                .get_player_by_id(other)
                .ok()
                .flatten()
                .map(|p| p.name)
                .unwrap_or_else(|| format!("#{other}"));
            let preview = state
                .db
                .get_recent_chat_messages(&t, 1)
                .ok()
                .and_then(|m| m.first().map(|(_, n, t, ..)| format!("{n}: {t}")))
                .unwrap_or_default();
            entries.push((t.clone(), false, oname, preview));
        }
    }

    send_u_packet(tx, "mL", &chat_list(&entries).1);
    send_u_packet(tx, "mN", &chat_notification(0).1);
}
