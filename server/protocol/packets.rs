use bytes::{BufMut, BytesMut};

/// Encode a Status packet (ST): UTF-8 string
pub fn status(msg: &str) -> (/*event*/ &'static str, Vec<u8>) {
    ("ST", msg.as_bytes().to_vec())
}

/// Encode an AU packet (server→client): session id string
pub fn au_session(sid: &str) -> (&'static str, Vec<u8>) {
    ("AU", sid.as_bytes().to_vec())
}

/// Encode a Ping packet (PI): "`pong_resp:client_time:text`"
pub fn ping(pong_resp: i32, client_time: i32, text: &str) -> (&'static str, Vec<u8>) {
    let s = format!("{pong_resp}:{client_time}:{text}");
    ("PI", s.into_bytes())
}

/// Encode a WorldInfo/CF packet: JSON
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
// TODO: will be used when legacy AH auth flow is fully wired
#[allow(dead_code)]
pub fn auth_hash(user_id: i32, hash: &str) -> (&'static str, Vec<u8>) {
    let s = format!("{user_id}_{hash}");
    ("AH", s.into_bytes())
}

/// Encode a TP packet (@T): "x:y"
pub fn tp(x: i32, y: i32) -> (&'static str, Vec<u8>) {
    let s = format!("{x}:{y}");
    ("@T", s.into_bytes())
}

/// Encode a BotInfo packet (`BotInfoPacket.packetName` = `"BI"`).
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
pub fn gu_close() -> (&'static str, Vec<u8>) {
    ("Gu", b"_".to_vec())
}

/// Encode an OK packet: "title#message"
pub fn ok_message(title: &str, message: &str) -> (&'static str, Vec<u8>) {
    let s = format!("{title}#{message}");
    ("OK", s.into_bytes())
}

/// Encode a Speed packet (sp): "`xy_pause:road_pause:depth`" (ints, ms)
pub fn speed(xy_pause: i32, road_pause: i32, depth: i32) -> (&'static str, Vec<u8>) {
    let s = format!("{xy_pause}:{road_pause}:{depth}");
    ("sp", s.into_bytes())
}

/// Encode an Online packet (ON): "online:max"
// TODO: will be used when online count broadcast is fully wired
#[allow(dead_code)]
pub fn online(count: i32, max: i32) -> (&'static str, Vec<u8>) {
    let s = format!("{count}:{max}");
    ("ON", s.into_bytes())
}

/// Encode a Level packet (LV): text number
pub fn level(lvl: i32) -> (&'static str, Vec<u8>) {
    let s = format!("{lvl}");
    ("LV", s.into_bytes())
}

/// Encode a Money packet (P$): JSON {"money":M,"creds":C}
pub fn money(money: i64, creds: i64) -> (&'static str, Vec<u8>) {
    let s = format!(r#"{{"money":{money},"creds":{creds}}}"#);
    ("P$", s.into_bytes())
}

/// Encode an `AutoDigg` packet (BD): "0" or "1"
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

/// Encode a Geo packet (GE): имя региона (строка), НЕ координаты.
/// Ref: `pSenders.cs:28` — `World.GetProp(p.geo.Peek()).name`
/// Клиент отображает payload как текст: `GUIManager.THIS.GeoTF.text = " " + msg + " "`
pub fn geo(name: &str) -> (&'static str, Vec<u8>) {
    ("GE", name.as_bytes().to_vec())
}

/// Encode a Live/Health packet (@L): "health:max"
pub fn health(hp: i32, max_hp: i32) -> (&'static str, Vec<u8>) {
    let s = format!("{hp}:{max_hp}");
    ("@L", s.into_bytes())
}

/// Encode a Basket/Crystals packet (@B): "G:R:B:V:W:C:Capacity"
pub fn basket(crys: &[i64; 6], capacity: i64) -> (&'static str, Vec<u8>) {
    let s = format!(
        "{}:{}:{}:{}:{}:{}:{}",
        crys[0], crys[1], crys[2], crys[3], crys[4], crys[5], capacity
    );
    ("@B", s.into_bytes())
}

/// Encode a Config packet (#F): UTF-8 string
pub fn config_packet(content: &str) -> (&'static str, Vec<u8>) {
    ("#F", content.as_bytes().to_vec())
}

/// Пакет `#S` как `SettingsPacket` в `server_reference/Server/Network/SettingsPacket.cs`:
/// `"#" + join("#", key + "#" + value ...)` для словаря из `Settings(bool)` в `Settings.cs`.
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

/// Encode an Inventory Show packet (IN): "show:{all}:{selected}:{key1#val1#key2#val2...}"
pub fn inventory_show(items: &[(i32, i32)], selected: i32, total: i32) -> (&'static str, Vec<u8>) {
    let grid = if items.is_empty() {
        String::new()
    } else {
        items
            .iter()
            .map(|(k, v)| format!("{k}#{v}"))
            .collect::<Vec<_>>()
            .join("#")
    };
    let s = format!("show:{total}:{selected}:{grid}");
    ("IN", s.into_bytes())
}

/// Encode an Inventory Close packet (IN): "close:"
pub fn inventory_close() -> (&'static str, Vec<u8>) {
    ("IN", b"close:".to_vec())
}

/// `MinesServer.Network.GUI.SkillsPacket`: имя `@S`, тело `Join("#", k:v...) + "#"`.
pub fn skills_packet(skills: &[(String, i32)]) -> (&'static str, Vec<u8>) {
    let body = skills
        .iter()
        .map(|(code, pct)| format!("{code}:{pct}"))
        .collect::<Vec<_>>()
        .join("#");
    let s = format!("{body}#");
    ("@S", s.into_bytes())
}

/// Encode a `ClanHide` packet (`ClanHidePacket.packetName` = `"cH"`).
pub const fn clan_hide() -> (&'static str, Vec<u8>) {
    ("cH", Vec::new())
}

/// Encode a `ClanShow` packet (`ClanShowPacket.packetName` = `"cS"`).
pub fn clan_show(clan_id: i32) -> (&'static str, Vec<u8>) {
    ("cS", format!("{clan_id}").into_bytes())
}

// ─── Chat channel packets ────────────────────────────────────────────────────

/// mO — set current chat channel: "TAG:ChannelName"
pub fn chat_current(tag: &str, name: &str) -> (&'static str, Vec<u8>) {
    ("mO", format!("{tag}:{name}").into_bytes())
}

/// mL — chat channel list
/// Each entry: `TAG±NOTIFICATION±TITLE±LAST_MSG_PREVIEW`, joined by '#'
pub fn chat_list(entries: &[(String, bool, String, String)]) -> (&'static str, Vec<u8>) {
    let parts: Vec<String> = entries
        .iter()
        .map(|(tag, notif, title, preview)| {
            format!(
                "{}±{}±{}±{}",
                tag,
                if *notif { "1" } else { "0" },
                title,
                preview
            )
        })
        .collect();
    ("mL", parts.join("#").into_bytes())
}

/// mU — chat messages for a channel
/// Format: {"ch":"TAG","h":["±COLOR±CLANID±TIME±NICKNAME±TEXT±USERID",...]}
use crate::game::chat::ChatMessage;

pub fn chat_messages(tag: &str, messages: &[ChatMessage]) -> (&'static str, Vec<u8>) {
    let msg_strs: Vec<String> = messages
        .iter()
        .map(|m| {
            format!(
                "\"±{}±{}±{}±{}±{}±{}\"",
                m.color, m.clan_id, m.time, m.nickname, m.text, m.user_id
            )
        })
        .collect();
    let json = format!("{{\"ch\":\"{}\",\"h\":[{}]}}", tag, msg_strs.join(","));
    ("mU", json.into_bytes())
}

/// mN — unread notification count as string
pub fn chat_notification(count: i32) -> (&'static str, Vec<u8>) {
    ("mN", count.to_string().into_bytes())
}

// ─── HB (Hub Bundle) sub-packets ────────────────────────────────────────────

/// Build a complete HB packet payload from sub-packets
pub fn hb_bundle(sub_packets: &[Vec<u8>]) -> (&'static str, Vec<u8>) {
    let total_len: usize = sub_packets.iter().map(std::vec::Vec::len).sum();
    let mut buf = BytesMut::with_capacity(total_len);
    for p in sub_packets {
        buf.put_slice(p);
    }
    ("HB", buf.to_vec())
}

/// HB sub-packet: Map chunk (type `M`)
/// `[1B tag M][1B width][1B height][u16 LE x][u16 LE y][cells...]`
pub fn hb_map(x: u16, y: u16, width: u8, height: u8, cells: &[u8]) -> Vec<u8> {
    let mut buf = BytesMut::with_capacity(1 + 2 + 4 + cells.len());
    buf.put_u8(b'M');
    buf.put_u8(width);
    buf.put_u8(height);
    buf.put_u16_le(x);
    buf.put_u16_le(y);
    buf.put_slice(cells);
    buf.to_vec()
}

/// HB sub-packet: Bot (type "X")
/// [1B tag 'X'] [1B dir] [1B skin] [1B tail] [u16 LE id] [u16 LE x] [u16 LE y] [u16 LE `clan_id`]
pub fn hb_bot(id: u16, x: u16, y: u16, dir: u8, skin: u8, clan_id: u16, tail: u8) -> Vec<u8> {
    let mut buf = BytesMut::with_capacity(12);
    buf.put_u8(b'X');
    buf.put_u8(dir);
    buf.put_u8(skin);
    buf.put_u8(tail);
    buf.put_u16_le(id);
    buf.put_u16_le(x);
    buf.put_u16_le(y);
    buf.put_u16_le(clan_id);
    buf.to_vec()
}

/// HB sub-packet: Pack/building (type "O")
/// [1B tag 'O'] [i32 LE `block_pos`] [u16 LE count] [packs...]
///
/// В `server_reference` у `HBPack` **Encode и Decode расходятся** (клан пишут в [5], читают с [6]).
/// На проводе ориентируемся на **`Decode`** — то, что реально читает приёмник: `[6]` clan, `[7]` off, `[5]` не используется.
pub fn hb_packs(block_pos: i32, packs: &[(u8, u16, u16, u8, u8)]) -> Vec<u8> {
    let Ok(n) = u16::try_from(packs.len()) else {
        return vec![];
    };
    let mut buf = BytesMut::with_capacity(1 + 4 + 2 + packs.len() * 8);
    buf.put_u8(b'O');
    buf.put_i32_le(block_pos);
    buf.put_u16_le(n);
    for &(code, x, y, clan_id, off) in packs {
        buf.put_u8(code);
        buf.put_u16_le(x);
        buf.put_u16_le(y);
        buf.put_u8(0);
        buf.put_u8(clan_id);
        buf.put_u8(off);
    }
    buf.to_vec()
}

/// HB sub-packet: FX effect (type "F")
/// [1B tag 'F'] [1B `fx_type`] [u16 LE x] [u16 LE y]
pub fn hb_fx(x: u16, y: u16, fx_type: u8) -> Vec<u8> {
    let mut buf = BytesMut::with_capacity(6);
    buf.put_u8(b'F');
    buf.put_u8(fx_type);
    buf.put_u16_le(x);
    buf.put_u16_le(y);
    buf.to_vec()
}

/// HB sub-packet: Delete object/bot (type "L")
/// [1B tag 'L'] [u16 LE id]
pub fn hb_bot_del(id: u16) -> Vec<u8> {
    let mut buf = BytesMut::with_capacity(3);
    buf.put_u8(b'L');
    buf.put_u16_le(id);
    buf.to_vec()
}

/// HB sub-packet: Bot leave block (type "S")
/// [1B tag 'S'] [u16 LE id] [i32 LE block_pos]
// TODO: will be used when bot block-leave events are fully wired
#[allow(dead_code)]
pub fn hb_bot_leave_block(id: u16, block_pos: i32) -> Vec<u8> {
    let mut buf = BytesMut::with_capacity(7);
    buf.put_u8(b'S');
    buf.put_u16_le(id);
    buf.put_i32_le(block_pos);
    buf.to_vec()
}

/// HB sub-packet: Directed FX (type "D")
/// [1B tag 'D'] [1B fx] [1B dir] [1B color] [u16 LE x] [u16 LE y] [u16 LE `bot_id`]
pub fn hb_directed_fx(bot_id: u16, x: u16, y: u16, fx: u8, dir: u8, color: u8) -> Vec<u8> {
    let mut buf = BytesMut::with_capacity(10);
    buf.put_u8(b'D');
    buf.put_u8(fx);
    buf.put_u8(dir);
    buf.put_u8(color);
    buf.put_u16_le(x);
    buf.put_u16_le(y);
    buf.put_u16_le(bot_id);
    buf.to_vec()
}

/// HB sub-packet: Chat bubble (type "C")
/// [1B tag 'C'] [u16 LE bid] [u16 LE x] [u16 LE y] [u16 LE strlen] [utf8 text]
pub fn hb_chat(bot_id: u16, x: u16, y: u16, text: &str) -> Vec<u8> {
    let text_bytes = text.as_bytes();
    let Ok(text_len) = u16::try_from(text_bytes.len()) else {
        return vec![];
    };
    let mut buf = BytesMut::with_capacity(1 + 2 + 2 + 2 + 2 + text_bytes.len());
    buf.put_u8(b'C');
    buf.put_u16_le(bot_id);
    buf.put_u16_le(x);
    buf.put_u16_le(y);
    buf.put_u16_le(text_len);
    buf.put_slice(text_bytes);
    buf.to_vec()
}

/// HB sub-packet: Bot list (type "B")
/// Layout: `[1B tag 'B'][u16 LE count][u16 LE bot_id]*count`
// TODO: will be used when bot-list HB sync is fully wired
#[allow(dead_code)]
pub fn hb_bots_list(bot_ids: &[u16]) -> Vec<u8> {
    let Ok(n) = u16::try_from(bot_ids.len()) else {
        return vec![];
    };
    let mut buf = BytesMut::with_capacity(1 + 2 + bot_ids.len() * 2);
    buf.put_u8(b'B');
    buf.put_u16_le(n);
    for &id in bot_ids {
        buf.put_u16_le(id);
    }
    buf.to_vec()
}

/// HB sub-packet: Gun/shot (type "Z")
/// Layout: `[1B tag 'Z'][1B amount][1B color][u16 LE x][u16 LE y][u16 LE bot_id]*amount`
// TODO: will be used when gun-shot HB broadcast is fully wired
#[allow(dead_code)]
pub fn hb_gun(x: u16, y: u16, color: u8, bot_ids: &[u16]) -> Vec<u8> {
    let Ok(n) = u8::try_from(bot_ids.len()) else {
        return vec![];
    };
    let mut buf = BytesMut::with_capacity(1 + 2 + 4 + bot_ids.len() * 2);
    buf.put_u8(b'Z');
    buf.put_u8(n);
    buf.put_u8(color);
    buf.put_u16_le(x);
    buf.put_u16_le(y);
    for &id in bot_ids {
        buf.put_u16_le(id);
    }
    buf.to_vec()
}

/// HB sub-packet: single cell update — wraps `hb_map` with 1x1
pub fn hb_cell(x: u16, y: u16, cell: u8) -> Vec<u8> {
    hb_map(x, y, 1, 1, &[cell])
}

// ─── Decode helpers for client→server packets ───────────────────────────────

/// Decode a TY wrapper: [4B `event_name`] [u32 LE time] [u32 LE x] [u32 LE y] [`sub_payload`...]
pub struct TyPacket {
    pub event_name: [u8; 4],
    pub time: u32,
    pub x: u32,
    pub y: u32,
    pub sub_payload: Vec<u8>,
}

impl TyPacket {
    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < 16 {
            return None;
        }
        let mut event_name = [0u8; 4];
        event_name.copy_from_slice(&data[0..4]);
        let time = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let x = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        let y = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
        let sub_payload = data[16..].to_vec();
        Some(Self {
            event_name,
            time,
            x,
            y,
            sub_payload,
        })
    }

    pub fn event_str(&self) -> &str {
        std::str::from_utf8(&self.event_name).unwrap_or("????")
    }

    // TODO: will be used when client-side timestamp handling is fully wired
    #[must_use]
    #[allow(dead_code)]
    pub const fn client_timestamp(&self) -> u32 {
        self.time
    }
}

/// Decode an AU packet (client→server): "`uniq_type_token`" or just "sessionid"
pub struct AuClientPacket {
    pub uniq: String,
    pub auth_type: AuAuthType,
}

pub enum AuAuthType {
    NoAuth,
    Regular { user_id: i32, token: String },
}

impl AuClientPacket {
    #[must_use]
    pub const fn client_uniq(&self) -> &str {
        self.uniq.as_str()
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        let s = std::str::from_utf8(data).ok()?.trim();
        if s.is_empty() {
            return None;
        }
        let parts: Vec<&str> = s.split('_').collect();
        match parts.as_slice() {
            [_uniq] => {
                // Single-segment payload (no underscores) — no valid auth type; deny.
                None
            }
            [uniq, kind, rest @ ..] => {
                if uniq.is_empty() || kind.is_empty() {
                    return None;
                }
                let uniq = (*uniq).to_string();
                let token: String = rest.join("_");
                let auth_type = match *kind {
                    "NO" | "NO_AUTH" | "NOAUTH" => AuAuthType::NoAuth,
                    _ => {
                        let user_id = (*kind).parse().ok()?;
                        AuAuthType::Regular { user_id, token }
                    }
                };
                Some(Self { uniq, auth_type })
            }
            _ => None,
        }
    }
}

/// Decode Pong (client→server, inside TY or standalone): "resp:time"
pub struct PongClient {
    // TODO: response field will be used when ping/pong roundtrip validation is wired
    #[allow(dead_code)]
    pub response: i32,
    pub current_time: i32,
}

impl PongClient {
    pub fn decode(data: &[u8]) -> Option<Self> {
        let s = std::str::from_utf8(data).ok()?;
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 2 {
            return None;
        }
        Some(Self {
            response: parts[0].parse().ok()?,
            current_time: parts[1].parse().ok()?,
        })
    }
}

/// Decode Xmov (inside TY `sub_payload)`: direction as text integer
pub fn decode_xmov(data: &[u8]) -> Option<i32> {
    parse_i32_text(data)
}

/// Decode Xbld (inside TY `sub_payload)`: "{direction}{blockType}"
/// C# decodes as: dir = int.Parse(data[..^1]), blockType = data[^1..]
pub struct XbldClient {
    pub direction: i32,
    pub block_type: String,
}

impl XbldClient {
    pub fn decode(data: &[u8]) -> Option<Self> {
        let s = std::str::from_utf8(data).ok()?;
        if s.is_empty() {
            return None;
        }
        let dir_str = &s[..s.len() - 1];
        let block_type = s[s.len() - 1..].to_string();
        let direction = dir_str.parse().ok()?;
        Some(Self {
            direction,
            block_type,
        })
    }
}

/// Decode Xdig (inside TY `sub_payload)`: direction as text integer
pub fn decode_xdig(data: &[u8]) -> Option<i32> {
    parse_i32_text(data)
}

fn parse_i32_text(data: &[u8]) -> Option<i32> {
    std::str::from_utf8(data).ok()?.trim().parse().ok()
}

/// Decode GUI_ button press (inside TY `sub_payload`): JSON `{"b":"button_name"}`
/// Референс `GUI_Packet.Decode`: `JSON.Parse(UTF8.GetString(data))["b"]`
pub fn decode_gui_button(data: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(data).ok()?;
    let v: serde_json::Value = serde_json::from_str(s).ok()?;
    v.get("b")?.as_str().map(std::string::ToString::to_string)
}

/// Decode local chat (inside TY `sub_payload`): legacy `length:message` or plain UTF-8 text
pub struct LoclClient {
    // TODO: length field is decoded from legacy wire format, will be used for validation
    #[allow(dead_code)]
    pub length: i32,
    pub message: String,
}

impl LoclClient {
    pub fn decode(data: &[u8]) -> Option<Self> {
        let s = std::str::from_utf8(data).ok()?;
        if s.is_empty() {
            return None;
        }

        if let Some((len_part, message)) = s.split_once(':')
            && let Ok(length) = len_part.parse()
        {
            return Some(Self {
                length,
                message: message.to_string(),
            });
        }

        Some(Self {
            length: i32::try_from(s.len()).unwrap_or(i32::MAX),
            message: s.to_string(),
        })
    }
}

/// Decode Whoi (bot ID request): list of u16 IDs
pub fn decode_whoi(data: &[u8]) -> Vec<i32> {
    let Ok(s) = std::str::from_utf8(data) else {
        return vec![];
    };
    s.split(',').filter_map(|p| p.parse().ok()).collect()
}
