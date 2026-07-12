#[path = "packets/client.rs"]
mod client;
#[path = "packets/hub.rs"]
mod hub;
#[path = "packets/social.rs"]
mod social;

pub use client::*;
pub use hub::*;
pub use social::*;

#[cfg(test)]
use bytes::BytesMut;

/// Encode a Status packet (ST): UTF-8 string
#[must_use]
pub fn status(msg: &str) -> (/*event*/ &'static str, Vec<u8>) {
    ("ST", msg.as_bytes().to_vec())
}

/// Encode an AU packet (server→client): session id string
#[must_use]
pub fn au_session(sid: &str) -> (&'static str, Vec<u8>) {
    ("AU", sid.as_bytes().to_vec())
}

/// Encode a Ping packet (PI): "`pong_resp:client_time:text`"
#[must_use]
pub fn ping(pong_resp: i32, client_time: i32, text: &str) -> (&'static str, Vec<u8>) {
    let s = format!("{pong_resp}:{client_time}:{text}");
    ("PI", s.into_bytes())
}

/// Encode a WorldInfo/CF packet: JSON
#[must_use]
pub fn world_info(
    name: &str,
    width: u32,
    height: u32,
    version_code: i32,
    version_name: &str,
    update_url: &str,
    update_desc: &str,
) -> (&'static str, Vec<u8>) {
    let json = format!(
        r#"{{"width":{width},"height":{height},"name":"{name}","v":{version_code},"version":"{version_name}","update_url":"{update_url}","update_desc":"{update_desc}"}}"#
    );
    // Client WorldInitScript registers OnU("cf", ...) — case-sensitive.
    // Референс сервер: `"CF"`, но клиент ожидает lowercase `"cf"`.
    ("cf", json.into_bytes())
}

/// Encode an AH packet: "`user_id_hash`" or "BAD"
#[must_use]
pub fn auth_hash(user_id: i32, hash: &str) -> (&'static str, Vec<u8>) {
    let s = format!("{user_id}_{hash}");
    ("AH", s.into_bytes())
}

/// Encode a TP packet (@T): "x:y"
#[must_use]
pub fn tp(x: i32, y: i32) -> (&'static str, Vec<u8>) {
    let s = format!("{x}:{y}");
    ("@T", s.into_bytes())
}

/// Encode a `BotInfo` packet (`BotInfoPacket.packetName` = `"BI"`).
#[must_use]
pub fn bot_info(name: &str, x: i32, y: i32, id: i32) -> (&'static str, Vec<u8>) {
    let json = serde_json::json!({
        "x": x,
        "y": y,
        "id": id,
        "name": name
    });
    ("BI", json.to_string().into_bytes())
}

/// Encode a Gu (close window) packet: payload is just "_"
#[must_use]
pub fn gu_close() -> (&'static str, Vec<u8>) {
    ("Gu", b"_".to_vec())
}

/// Encode a BB (bibika/beep) packet.
#[must_use]
pub fn bibika() -> (&'static str, Vec<u8>) {
    ("BB", b"_".to_vec())
}

/// Encode an OK packet: "title#message"
#[must_use]
pub fn ok_message(title: &str, message: &str) -> (&'static str, Vec<u8>) {
    let s = format!("{title}#{message}");
    ("OK", s.into_bytes())
}

/// Encode a Speed packet (sp): "`xy_pause:road_pause:depth`" (ints, ms)
#[must_use]
pub fn speed(xy_pause: i32, road_pause: i32, depth: i32) -> (&'static str, Vec<u8>) {
    let s = format!("{xy_pause}:{road_pause}:{depth}");
    ("sp", s.into_bytes())
}

/// Encode an Online packet (ON): "online:max"
#[must_use]
pub fn online(count: i32, max: i32) -> (&'static str, Vec<u8>) {
    let s = format!("{count}:{max}");
    ("ON", s.into_bytes())
}

/// Encode a Level packet (LV): text number
#[must_use]
pub fn level(lvl: i32) -> (&'static str, Vec<u8>) {
    let s = format!("{lvl}");
    ("LV", s.into_bytes())
}

/// Encode a Money packet (P$): JSON {"money":M,"creds":C}.
/// C# `SendMoney()` clamps negative balances to zero before sending.
#[must_use]
pub fn money(money: i64, creds: i64) -> (&'static str, Vec<u8>) {
    let money = money.max(0);
    let creds = creds.max(0);
    let s = format!(r#"{{"money":{money},"creds":{creds}}}"#);
    ("P$", s.into_bytes())
}

/// Encode an `AutoDigg` packet (BD): "0" or "1"
#[must_use]
pub fn auto_digg(enabled: bool) -> (&'static str, Vec<u8>) {
    (
        "BD",
        if enabled {
            b"1".to_vec()
        } else {
            b"0".to_vec()
        },
    )
}

/// Encode an aggression packet (BA): "0" or "1".
#[must_use]
pub fn aggression(enabled: bool) -> (&'static str, Vec<u8>) {
    (
        "BA",
        if enabled {
            b"1".to_vec()
        } else {
            b"0".to_vec()
        },
    )
}

/// Encode a hand-mode packet (BH): "0" or "1".
#[must_use]
pub fn hand_mode(enabled: bool) -> (&'static str, Vec<u8>) {
    (
        "BH",
        if enabled {
            b"1".to_vec()
        } else {
            b"0".to_vec()
        },
    )
}

/// Encode a Geo packet (GE): имя региона (строка), НЕ координаты.
///
/// Ref: `pSenders.cs:28` — `World.GetProp(p.geo.Peek()).name`
/// Клиент отображает payload как текст: `GUIManager.THIS.GeoTF.text = " " + msg + " "`
#[must_use]
pub fn geo(name: &str) -> (&'static str, Vec<u8>) {
    ("GE", name.as_bytes().to_vec())
}

/// Encode a Live/Health packet (@L): "health:max"
#[must_use]
pub fn health(hp: i32, max_hp: i32) -> (&'static str, Vec<u8>) {
    let s = format!("{hp}:{max_hp}");
    ("@L", s.into_bytes())
}

/// Encode a Basket/Crystals packet (@B): "G:R:B:V:W:C:Capacity"
#[must_use]
pub fn basket(crys: &[i64; 6], capacity: i64) -> (&'static str, Vec<u8>) {
    let s = format!(
        "{}:{}:{}:{}:{}:{}:{}",
        crys[0], crys[1], crys[2], crys[3], crys[4], crys[5], capacity
    );
    ("@B", s.into_bytes())
}

/// Encode a Config packet (#F): UTF-8 string
#[must_use]
pub fn config_packet(content: &str) -> (&'static str, Vec<u8>) {
    ("#F", content.as_bytes().to_vec())
}

/// Пакет `#S` как `SettingsPacket` в `server_reference/Server/Network/SettingsPacket.cs`:
/// `"#" + join("#", key + "#" + value ...)` для словаря из `Settings(bool)` в `Settings.cs`.
#[must_use]
pub fn settings_default_wire() -> (&'static str, Vec<u8>) {
    const PAIRS: &[(&str, &str)] = &[
        ("cc", "10"),
        ("snd", "0"),
        ("mus", "0"),
        ("isca", "0"),
        ("tsca", "0"),
        ("mous", "1"),
        ("pot", "0"),
        ("frc", "1"),
        ("ctrl", "1"),
        ("mof", "1"),
    ];
    let inner = PAIRS
        .iter()
        .map(|(k, v)| format!("{k}#{v}"))
        .collect::<Vec<_>>()
        .join("#");
    ("#S", format!("#{inner}").into_bytes())
}

/// Encode a Programmator status packet (@P): "0" or "1"
#[must_use]
pub fn programmator_status(running: bool) -> (&'static str, Vec<u8>) {
    (
        "@P",
        if running {
            b"1".to_vec()
        } else {
            b"0".to_vec()
        },
    )
}

/// Encode an Inventory Full packet (IN): "full:{selected}:{key1#val1#key2#val2...}"
#[must_use]
pub fn inventory_full(items: &[(i32, i32)], selected: i32) -> (&'static str, Vec<u8>) {
    let grid = if items.is_empty() {
        String::new()
    } else {
        items
            .iter()
            .map(|(k, v)| format!("{k}#{v}"))
            .collect::<Vec<_>>()
            .join("#")
    };
    let s = format!("full:{selected}:{grid}");
    ("IN", s.into_bytes())
}

/// Encode an Inventory Show packet (IN): "show:{all}:{selected}:{key1#val1#key2#val2...}"
#[must_use]
pub fn inventory_show(items: &[(i32, i32)], selected: i32, all: i32) -> (&'static str, Vec<u8>) {
    let grid = if items.is_empty() {
        String::new()
    } else {
        items
            .iter()
            .map(|(k, v)| format!("{k}#{v}"))
            .collect::<Vec<_>>()
            .join("#")
    };
    let s = format!("show:{all}:{selected}:{grid}");
    ("IN", s.into_bytes())
}

/// Encode an Inventory Close packet (IN): "close:"
#[must_use]
pub fn inventory_close() -> (&'static str, Vec<u8>) {
    ("IN", b"close:".to_vec())
}

/// Encode an Inventory Choose packet (IN):
/// "choose:{hint}:{distance}:{dx}:{dy}:{w}:{h}:{grid01}"
///
/// C# `Inventory.Choose` (Inventory.cs) always follows `InvToSend()` with this
/// packet (placeholder `bool[0,0]` grid). The client only needs it to set
/// `GUIManager.inventoryItem != -1`, which is the precondition that enables
/// Enter→`INUS` and shows the "item selected" UI state. With `w=0`/`h=0` the
/// client calls `HideGrid()` and ignores hint/distance/dx/dy. Without this
/// packet, selecting an item never arms it and pressing Enter does nothing.
#[must_use]
pub fn inventory_choose() -> (&'static str, Vec<u8>) {
    ("IN", b"choose::0:0:0:0:0:".to_vec())
}

/// Encode a Mission Panel packet (`MM`):
/// `{url}#{img_x}#{img_y}#{progress}#{text}`
#[must_use]
pub fn mission_panel(
    url: &str,
    img_x: i32,
    img_y: i32,
    progress: i32,
    text: &str,
) -> (&'static str, Vec<u8>) {
    let s = format!("{url}#{img_x}#{img_y}#{progress}#{text}");
    ("MM", s.into_bytes())
}

/// Encode a Mission Progress packet (`MP`): "{experience}:{max}".
#[must_use]
pub fn mission_progress(experience: i32, max: i32) -> (&'static str, Vec<u8>) {
    let s = format!("{experience}:{max}");
    ("MP", s.into_bytes())
}

/// Encode a Mission/Tutorial Navigation packet (`MN`):
/// `{text}#{dx}#{dy}#{anchor_type}#{hide_reason}`.
///
/// This packet is client-driven; no C# reference packet type exists.
#[must_use]
pub fn mission_notification(
    text: &str,
    dx: i32,
    dy: i32,
    anchor_type: i32,
    hide_reason: &str,
) -> (&'static str, Vec<u8>) {
    let s = format!("{text}#{dx}#{dy}#{anchor_type}#{hide_reason}");
    ("MN", s.into_bytes())
}

/// `MinesServer.Network.GUI.SkillsPacket`: имя `@S`, тело `Join("#", k:v...) + "#"`.
#[must_use]
pub fn skills_packet(skills: &[(String, i32)]) -> (&'static str, Vec<u8>) {
    let body = skills
        .iter()
        .map(|(code, pct)| format!("{code}:{pct}"))
        .collect::<Vec<_>>()
        .join("#");
    let s = format!("{body}#");
    ("@S", s.into_bytes())
}

/// `#P` — open programmator editor: `{"id":N,"title":"name","source":"code"}`.
#[must_use]
pub fn open_programmator(id: i32, title: &str, source: &str) -> (&'static str, Vec<u8>) {
    let json = serde_json::json!({"id": id, "title": title, "source": source});
    ("#P", json.to_string().into_bytes())
}

/// Decode Whoi (bot ID request): list of u16 IDs
#[must_use]
pub fn decode_whoi(data: &[u8]) -> Option<Vec<i32>> {
    let s = std::str::from_utf8(data).ok()?;
    if s.is_empty() {
        return Some(Vec::new());
    }
    s.split(',')
        .map(|p| {
            let p = p.trim();
            if p.is_empty() { None } else { p.parse().ok() }
        })
        .collect()
}

#[cfg(test)]
#[path = "packets/tests.rs"]
mod tests;
