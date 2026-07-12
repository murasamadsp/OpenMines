use crate::chat::{CHAT_HISTORY_LIMIT, ChatMessage};

/// Encode a `ClanHide` packet (`ClanHidePacket.packetName` = `"cH"`).
#[must_use]
pub const fn clan_hide() -> (&'static str, Vec<u8>) {
    ("cH", Vec::new())
}

/// Encode a `ClanShow` packet (`ClanShowPacket.packetName` = `"cS"`).
///
/// Client uses the value as sprite index: `ClanSprite.sprites[icon - 1]`.
#[must_use]
pub fn clan_show(icon: i32) -> (&'static str, Vec<u8>) {
    ("cS", format!("{icon}").into_bytes())
}

/// `mO`: set current chat channel as `TAG:ChannelName`.
#[must_use]
pub fn chat_current(tag: &str, name: &str) -> (&'static str, Vec<u8>) {
    ("mO", format!("{tag}:{name}").into_bytes())
}

/// `mL`: channel entries joined by `#`, fields joined by `±` on the wire.
#[must_use]
pub fn chat_list(entries: &[(String, bool, String, String)]) -> (&'static str, Vec<u8>) {
    let parts: Vec<String> = entries
        .iter()
        .map(|(tag, notification, title, preview)| {
            format!(
                "{}±{}±{}±{}",
                tag,
                if *notification { "1" } else { "0" },
                title,
                preview
            )
        })
        .collect();
    ("mL", parts.join("#").into_bytes())
}

fn json_escape(s: &str) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

/// `mU`: exactly seven `±`-separated fields per message, without a leading separator.
#[must_use]
pub fn chat_messages(tag: &str, messages: &[ChatMessage]) -> (&'static str, Vec<u8>) {
    let start = messages.len().saturating_sub(CHAT_HISTORY_LIMIT);
    let messages: Vec<String> = messages
        .iter()
        .skip(start)
        .map(|message| {
            format!(
                "\"{}±{}±{}±{}±{}±{}±{}\"",
                message.id,
                message.color,
                message.clan_id,
                message.time,
                json_escape(&message.nickname),
                json_escape(&message.text),
                message.user_id
            )
        })
        .collect();
    let json = format!("{{\"ch\":\"{}\",\"h\":[{}]}}", tag, messages.join(","));
    ("mU", json.into_bytes())
}

/// `mN`: unread notification count.
#[must_use]
pub fn chat_notification(count: i32) -> (&'static str, Vec<u8>) {
    ("mN", count.to_string().into_bytes())
}

/// `mC`: chat input color code in the client-supported `0..20` range.
#[must_use]
pub fn chat_color(code: i32) -> (&'static str, Vec<u8>) {
    ("mC", code.to_string().into_bytes())
}
