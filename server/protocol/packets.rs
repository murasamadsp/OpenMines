use binrw::BinWrite;
use bytes::{BufMut, BytesMut};
use std::borrow::Cow;

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
pub fn auth_hash(user_id: i32, hash: &str) -> (&'static str, Vec<u8>) {
    let s = format!("{user_id}_{hash}");
    ("AH", s.into_bytes())
}

/// Encode a TP packet (@T): "x:y"
pub fn tp(x: i32, y: i32) -> (&'static str, Vec<u8>) {
    let s = format!("{x}:{y}");
    ("@T", s.into_bytes())
}

/// Encode a `BotInfo` packet (`BotInfoPacket.packetName` = `"BI"`).
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

/// Encode an Inventory Full packet (IN): "full:{selected}:{key1#val1#key2#val2...}"
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

/// Encode an Inventory Close packet (IN): "close:"
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
pub fn inventory_choose() -> (&'static str, Vec<u8>) {
    ("IN", b"choose::0:0:0:0:0:".to_vec())
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
/// Client uses the value as sprite index: `ClanSprite.sprites[icon - 1]`.
/// C# reference uses icon ID (1..=218) as the clan identity; Rust stores it separately.
pub fn clan_show(icon: i32) -> (&'static str, Vec<u8>) {
    ("cS", format!("{icon}").into_bytes())
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

/// mU — chat messages for a channel.
///
/// Wire-формат каждого сообщения 1:1 с `client/.../ChatManager.cs` `muHandler`:
/// `ID±COLOR±CLANID±TIME±NICKNAME±TEXT±USERID` — ровно 7 полей, разделитель `±`,
/// БЕЗ ведущего `±`. Клиент делает `array = h[i].Split('±')`, требует
/// `array.Length == 7` и `int.Parse(array[0])` (= `GCMessage.id`). Ведущий `±`
/// (как в `server_reference` `GCMessage.Encode`) даёт `array[0] == ""` →
/// `FormatException` в Unity → сообщение не отображается. Клиент — источник
/// правды по wire (он неизменяем); референс здесь неверен.
/// Полный пакет: `{"ch":"TAG","h":["ID±...","..."]}`.
use crate::game::chat::ChatMessage;

pub fn chat_messages(tag: &str, messages: &[ChatMessage]) -> (&'static str, Vec<u8>) {
    let msg_strs: Vec<String> = messages
        .iter()
        .map(|m| {
            format!(
                "\"{}±{}±{}±{}±{}±{}±{}\"",
                m.id, m.color, m.clan_id, m.time, m.nickname, m.text, m.user_id
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

/// mC — код цвета поля ввода чата (short `0..=19`). Клиент
/// `ChatManager.cs muHandler`→`mcHandler`: `short.Parse(msg)` →
/// `Color.HSVToRGB(num/20, ...)`. Реф: `Chat/ChatColorPacket.cs`
/// (валидный диапазон `[0,20)`). Нет в `server_reference` логики — см.
/// `docs/CLIENT_PROTOCOL_GAPS.md` §5.
pub fn chat_color(code: i32) -> (&'static str, Vec<u8>) {
    ("mC", code.to_string().into_bytes())
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

fn write_slice<'a, T, W>(
    slice: &&[T],
    writer: &mut W,
    endian: binrw::Endian,
    _args: (),
) -> binrw::BinResult<()>
where
    T: binrw::BinWrite<Args<'a> = ()>,
    W: std::io::Write + std::io::Seek,
{
    for item in *slice {
        item.write_options(writer, endian, ())?;
    }
    Ok(())
}

#[derive(BinWrite)]
#[bw(little)]
struct HbMap<'a> {
    tag: u8,
    width: u8,
    height: u8,
    x: u16,
    y: u16,
    #[bw(write_with = write_slice)]
    cells: &'a [u8],
}

/// HB sub-packet: Map chunk (type `M`)
/// `[1B tag M][1B width][1B height][u16 LE x][u16 LE y][cells...]`
pub fn hb_map(x: u16, y: u16, width: u8, height: u8, cells: &[u8]) -> Vec<u8> {
    let packet = HbMap {
        tag: b'M',
        width,
        height,
        x,
        y,
        cells,
    };
    let mut writer = std::io::Cursor::new(Vec::new());
    packet.write(&mut writer).unwrap();
    writer.into_inner()
}

#[derive(BinWrite)]
#[bw(little)]
struct HbBot {
    tag: u8,
    dir: u8,
    skin: u8,
    tail: u8,
    id: u16,
    x: u16,
    y: u16,
    clan_id: u16,
}

/// HB sub-packet: Bot (type "X")
/// [1B tag 'X'] [1B dir] [1B skin] [1B tail] [u16 LE id] [u16 LE x] [u16 LE y] [u16 LE `clan_id`]
pub fn hb_bot(id: u16, x: u16, y: u16, dir: u8, skin: u8, clan_id: u16, tail: u8) -> Vec<u8> {
    let packet = HbBot {
        tag: b'X',
        dir,
        skin,
        tail,
        id,
        x,
        y,
        clan_id,
    };
    let mut writer = std::io::Cursor::new(Vec::new());
    packet.write(&mut writer).unwrap();
    writer.into_inner()
}

#[derive(BinWrite)]
#[bw(little)]
struct HbPackItem {
    code: u8,
    x: u16,
    y: u16,
    clan_id: u8,
    zero: u8,
    off: u8,
}

#[derive(BinWrite)]
#[bw(little)]
struct HbPacks {
    tag: u8,
    block_pos: i32,
    count: u16,
    packs: Vec<HbPackItem>,
}

/// HB sub-packet: Pack/building (type "O")
/// [1B tag 'O'] [i32 LE `block_pos`] [u16 LE count] [packs...]
///
/// Раскладка entry эталонируется по **КЛИЕНТУ** (`ServerController.Handlers.cs:1065`):
/// `clan = ToUInt16(buffer, i+5)` — u16 из байтов `[5..=6]`; `off = buffer[i+7]`.
/// C# `HBPack.Encode` (то, что реально шлётся клиенту) кладёт `ClanId` в байт `[5]`,
/// `[6]` остаётся 0 → клиент читает `clan | 0 = clan`. Раньше Rust ошибочно
/// ориентировался на C# `Decode` (читает `[6]`) и писал clan в `[6]` → клиент видел
/// `clan << 8` (clan*256, неверный клан/цвет пака). Теперь `[5]` clan, `[6]` 0 — 1:1 `Encode`.
pub fn hb_packs(block_pos: i32, packs: &[(u8, u16, u16, u8, u8)]) -> Vec<u8> {
    let Ok(n) = u16::try_from(packs.len()) else {
        return vec![];
    };
    let items = packs
        .iter()
        .map(|&(code, x, y, clan_id, off)| HbPackItem {
            code,
            x,
            y,
            clan_id,
            zero: 0,
            off,
        })
        .collect();
    let packet = HbPacks {
        tag: b'O',
        block_pos,
        count: n,
        packs: items,
    };
    let mut writer = std::io::Cursor::new(Vec::new());
    packet.write(&mut writer).unwrap();
    writer.into_inner()
}

#[derive(BinWrite)]
#[bw(little)]
struct HbFx {
    tag: u8,
    fx_type: u8,
    x: u16,
    y: u16,
}

/// HB sub-packet: FX effect (type "F")
/// [1B tag 'F'] [1B `fx_type`] [u16 LE x] [u16 LE y]
pub fn hb_fx(x: u16, y: u16, fx_type: u8) -> Vec<u8> {
    let packet = HbFx {
        tag: b'F',
        fx_type,
        x,
        y,
    };
    let mut writer = std::io::Cursor::new(Vec::new());
    packet.write(&mut writer).unwrap();
    writer.into_inner()
}

#[derive(BinWrite)]
#[bw(little)]
struct HbBotDel {
    tag: u8,
    id: u16,
}

/// HB sub-packet: Delete object/bot (type "L")
/// [1B tag 'L'] [u16 LE id]
pub fn hb_bot_del(id: u16) -> Vec<u8> {
    let packet = HbBotDel { tag: b'L', id };
    let mut writer = std::io::Cursor::new(Vec::new());
    packet.write(&mut writer).unwrap();
    writer.into_inner()
}

#[derive(BinWrite)]
#[bw(little)]
struct HbBotLeaveBlock {
    tag: u8,
    id: u16,
    block_pos: i32,
}

/// HB sub-packet: Bot leave block (type "S")
/// [1B tag 'S'] [u16 LE id] [i32 LE `block_pos`]
// TODO: will be used when bot block-leave events are fully wired
#[allow(dead_code)]
pub fn hb_bot_leave_block(id: u16, block_pos: i32) -> Vec<u8> {
    let packet = HbBotLeaveBlock {
        tag: b'S',
        id,
        block_pos,
    };
    let mut writer = std::io::Cursor::new(Vec::new());
    packet.write(&mut writer).unwrap();
    writer.into_inner()
}

#[derive(BinWrite)]
#[bw(little)]
struct HbDirectedFx {
    tag: u8,
    fx: u8,
    dir: u8,
    color: u8,
    x: u16,
    y: u16,
    bot_id: u16,
}

/// HB sub-packet: Directed FX (type "D")
/// [1B tag 'D'] [1B fx] [1B dir] [1B color] [u16 LE x] [u16 LE y] [u16 LE `bot_id`]
pub fn hb_directed_fx(bot_id: u16, x: u16, y: u16, fx: u8, dir: u8, color: u8) -> Vec<u8> {
    let packet = HbDirectedFx {
        tag: b'D',
        fx,
        dir,
        color,
        x,
        y,
        bot_id,
    };
    let mut writer = std::io::Cursor::new(Vec::new());
    packet.write(&mut writer).unwrap();
    writer.into_inner()
}

#[derive(BinWrite)]
#[bw(little)]
struct HbChat<'a> {
    tag: u8,
    bot_id: u16,
    x: u16,
    y: u16,
    text_len: u16,
    #[bw(write_with = write_slice)]
    text: &'a [u8],
}

/// HB sub-packet: Chat bubble (type "C")
/// [1B tag 'C'] [u16 LE bid] [u16 LE x] [u16 LE y] [u16 LE strlen] [utf8 text]
pub fn hb_chat(bot_id: u16, x: u16, y: u16, text: &str) -> Vec<u8> {
    let text_bytes = text.as_bytes();
    let Ok(text_len) = u16::try_from(text_bytes.len()) else {
        return vec![];
    };
    let packet = HbChat {
        tag: b'C',
        bot_id,
        x,
        y,
        text_len,
        text: text_bytes,
    };
    let mut writer = std::io::Cursor::new(Vec::new());
    packet.write(&mut writer).unwrap();
    writer.into_inner()
}

#[derive(BinWrite)]
#[bw(little)]
struct HbBotsList<'a> {
    tag: u8,
    count: u16,
    #[bw(write_with = write_slice)]
    bot_ids: &'a [u16],
}

/// HB sub-packet: Bot list (type "B")
/// Layout: `[1B tag 'B'][u16 LE count][u16 LE bot_id]*count`
// TODO: will be used when bot-list HB sync is fully wired
#[allow(dead_code)]
pub fn hb_bots_list(bot_ids: &[u16]) -> Vec<u8> {
    let Ok(n) = u16::try_from(bot_ids.len()) else {
        return vec![];
    };
    let packet = HbBotsList {
        tag: b'B',
        count: n,
        bot_ids,
    };
    let mut writer = std::io::Cursor::new(Vec::new());
    packet.write(&mut writer).unwrap();
    writer.into_inner()
}

#[derive(BinWrite)]
#[bw(little)]
struct HbGun<'a> {
    tag: u8,
    amount: u8,
    color: u8,
    x: u16,
    y: u16,
    #[bw(write_with = write_slice)]
    bot_ids: &'a [u16],
}

/// HB sub-packet: Gun/shot (type "Z")
/// Layout: `[1B tag 'Z'][1B amount][1B color][u16 LE x][u16 LE y][u16 LE bot_id]*amount`
// TODO: will be used when gun-shot HB broadcast is fully wired
#[allow(dead_code)]
pub fn hb_gun(x: u16, y: u16, color: u8, bot_ids: &[u16]) -> Vec<u8> {
    let Ok(n) = u8::try_from(bot_ids.len()) else {
        return vec![];
    };
    let packet = HbGun {
        tag: b'Z',
        amount: n,
        color,
        x,
        y,
        bot_ids,
    };
    let mut writer = std::io::Cursor::new(Vec::new());
    packet.write(&mut writer).unwrap();
    writer.into_inner()
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
    pub sub_payload: bytes::Bytes,
}

impl TyPacket {
    pub fn decode(data: &bytes::Bytes) -> Option<Self> {
        if data.len() < 16 {
            return None;
        }
        let event_name = [data[0], data[1], data[2], data[3]];
        let time = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let x = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        let y = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
        let sub_payload = data.slice(16..);
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
pub struct AuClientPacket<'a> {
    pub uniq: &'a str,
    pub auth_type: AuAuthType<'a>,
}

pub enum AuAuthType<'a> {
    NoAuth,
    Regular { user_id: i32, token: &'a str },
}

impl<'a> AuClientPacket<'a> {
    #[must_use]
    pub const fn client_uniq(&self) -> &str {
        self.uniq
    }

    pub fn decode(data: &'a [u8]) -> Option<Self> {
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
            [uniq, kind, _rest @ ..] => {
                if uniq.is_empty() || kind.is_empty() {
                    return None;
                }
                let auth_type = match *kind {
                    "NO" | "NO_AUTH" | "NOAUTH" => AuAuthType::NoAuth,
                    _ => {
                        let user_id = (*kind).parse().ok()?;
                        let token_start = uniq.len() + 1 + kind.len() + 1;
                        let token = if token_start <= s.len() {
                            &s[token_start..]
                        } else {
                            ""
                        };
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
pub struct XbldClient<'a> {
    pub direction: i32,
    pub block_type: &'a str,
}

impl<'a> XbldClient<'a> {
    pub fn decode(data: &'a [u8]) -> Option<Self> {
        let s = std::str::from_utf8(data).ok()?;
        if s.is_empty() {
            return None;
        }
        let dir_str = &s[..s.len() - 1];
        let block_type = &s[s.len() - 1..];
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
pub fn decode_gui_button(data: &[u8]) -> Option<Cow<'_, str>> {
    let s = std::str::from_utf8(data).ok()?;
    if s.starts_with('{')
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(s)
        && let Some(b) = v.get("b").and_then(|b| b.as_str())
    {
        return Some(Cow::Owned(b.to_string()));
    }
    // Fallback: treat entire payload as button name (raw string)
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(Cow::Borrowed(trimmed))
    }
}

/// Decode local chat (inside TY `sub_payload`): legacy `length:message` or plain UTF-8 text
pub struct LoclClient<'a> {
    // TODO: length field is decoded from legacy wire format, will be used for validation
    #[allow(dead_code)]
    pub length: i32,
    pub message: &'a str,
}

impl<'a> LoclClient<'a> {
    pub fn decode(data: &'a [u8]) -> Option<Self> {
        let s = std::str::from_utf8(data).ok()?;
        if s.is_empty() {
            return None;
        }

        if let Some((len_part, message)) = s.split_once(':')
            && let Ok(length) = len_part.parse()
        {
            return Some(Self { length, message });
        }

        Some(Self {
            length: i32::try_from(s.len()).unwrap_or(i32::MAX),
            message: s,
        })
    }
}

/// `#P` — open programmator editor: `{"id":N,"title":"name","source":"code"}`.
pub fn open_programmator(id: i32, title: &str, source: &str) -> (&'static str, Vec<u8>) {
    let json = serde_json::json!({"id": id, "title": title, "source": source});
    ("#P", json.to_string().into_bytes())
}

/// Decode Whoi (bot ID request): list of u16 IDs
pub fn decode_whoi(data: &[u8]) -> Vec<i32> {
    let Ok(s) = std::str::from_utf8(data) else {
        return vec![];
    };
    s.split(',').filter_map(|p| p.parse().ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::chat::ChatMessage;
    use crate::protocol::{Packet, u_packet};

    // Golden wire bytes привязаны к ДОКУМЕНТИРОВАННОМУ контракту фрейма
    // `[len i32 LE][type u8][2B event][payload]`, а НЕ сняты со снапшота текущего кода.
    // Имена событий case-sensitive (`AU` ≠ `au`). Тесты стерегут фрейминг,
    // порядок байт и регистр события; они НЕ заменяют проверку на живом клиенте.
    #[test]
    fn au_session_frame_golden_bytes() {
        let (event, payload) = au_session("abcde");
        assert_eq!(event, "AU");
        let mut buf = BytesMut::new();
        u_packet(event, &payload).encode(&mut buf).unwrap();
        assert_eq!(
            buf.as_ref(),
            &[
                0x0C, 0x00, 0x00,
                0x00, // length = 12 (i32 LE, включая эти 4 байта)
                b'U', // data_type
                b'A', b'U', // event_name
                b'a', b'b', b'c', b'd', b'e', // payload
            ]
        );
    }

    #[test]
    fn ping_frame_golden_bytes() {
        let (event, payload) = ping(0, 100, "hi");
        assert_eq!(event, "PI");
        let mut buf = BytesMut::new();
        u_packet(event, &payload).encode(&mut buf).unwrap();
        assert_eq!(
            buf.as_ref(),
            &[
                0x0F, 0x00, 0x00, 0x00, // length = 15
                b'U', b'P', b'I', // type + event
                b'0', b':', b'1', b'0', b'0', b':', b'h', b'i', // "0:100:hi"
            ]
        );
    }

    #[test]
    fn tp_frame_golden_bytes() {
        let (event, payload) = tp(5, 7);
        assert_eq!(event, "@T");
        let mut buf = BytesMut::new();
        u_packet(event, &payload).encode(&mut buf).unwrap();
        assert_eq!(
            buf.as_ref(),
            &[
                0x0A, 0x00, 0x00, 0x00, // length = 10
                b'U', b'@', b'T', // type + event
                b'5', b':', b'7', // "5:7"
            ]
        );
    }

    #[test]
    fn frame_roundtrip_decode_matches_encode() {
        let (event, payload) = au_session("xyzab");
        let mut buf = BytesMut::new();
        u_packet(event, &payload).encode(&mut buf).unwrap();
        let decoded = Packet::try_decode(&mut buf)
            .unwrap()
            .expect("полный фрейм обязан декодироваться");
        assert_eq!(decoded.data_type, b'U');
        assert_eq!(&decoded.event_name, b"AU");
        assert_eq!(decoded.payload, payload);
    }

    #[test]
    fn try_decode_rejects_bogus_length_without_panic() {
        // Сетевой ввод: кривой length-префикс не паникует — только Err / Ok(None).
        let mut tiny = BytesMut::from(&[0x05, 0x00, 0x00, 0x00, b'U', b'A'][..]); // length=5 (<7)
        assert!(Packet::try_decode(&mut tiny).is_err());

        let mut huge = BytesMut::from(&[0xFF, 0xFF, 0xFF, 0x7F, b'U'][..]); // length=i32::MAX
        assert!(Packet::try_decode(&mut huge).is_err());

        let mut partial = BytesMut::from(&[0x0C, 0x00, 0x00, 0x00, b'U'][..]); // объявлено 12, есть 5
        assert!(matches!(Packet::try_decode(&mut partial), Ok(None)));
    }

    /// Контракт `client/Assets/Scripts/ChatManager.cs` `muHandler` (источник
    /// правды по wire, клиент неизменяем):
    /// ```text
    /// string[] array = MuPacket.h[i].Split('±');
    /// if (array.Length == 7) {
    ///     GCMessage.id    = int.Parse(array[0]);
    ///     GCMessage.color = int.Parse(array[1]);
    ///     GCMessage.cid   = int.Parse(array[2]);
    ///     GCMessage.time  = int.Parse(array[3]);
    ///     GCMessage.nick  =           array[4];
    ///     GCMessage.text  =           array[5];
    ///     GCMessage.gid   = int.Parse(array[6]);
    /// }
    /// ```
    /// Ведущий `±` (как в неверном `server_reference` `GCMessage.Encode`) даёт
    /// `array[0] == ""` → `int.Parse` бросает `FormatException` в Unity →
    /// сообщение не отображается (FED-чат «не работает»). Тест ловит регресс.
    #[test]
    fn chat_messages_matches_client_gcmessage_parse_contract() {
        let msgs = [
            ChatMessage {
                id: 42,
                time: 12_345,
                clan_id: 7,
                user_id: 99,
                nickname: "Игрок".to_string(),
                text: "привет всем".to_string(),
                color: 10,
            },
            ChatMessage {
                id: 43,
                time: 12_346,
                clan_id: 0,
                user_id: 100,
                nickname: "bob".to_string(),
                text: "hi".to_string(),
                color: 1,
            },
        ];

        let (name, payload) = chat_messages("FED", &msgs);
        assert_eq!(name, "mU");

        let json: serde_json::Value =
            serde_json::from_slice(&payload).expect("mU payload must be valid JSON");
        assert_eq!(json["ch"], "FED");
        let h = json["h"].as_array().expect("`h` must be a JSON array");
        assert_eq!(h.len(), msgs.len());

        for (entry, src) in h.iter().zip(msgs.iter()) {
            let s = entry.as_str().expect("each `h` element is a JSON string");
            let parts: Vec<&str> = s.split('±').collect();
            // Клиент: `if (array.Length == 7)` — иначе сообщение игнорируется.
            assert_eq!(
                parts.len(),
                7,
                "expected exactly 7 ±-separated parts, got {}: {s:?}",
                parts.len()
            );
            // Клиент: `int.Parse(array[0])` — НЕ должно бросать (это и есть баг).
            let id: i32 = parts[0]
                .parse()
                .unwrap_or_else(|_| panic!("array[0] must parse as i32, got {:?}", parts[0]));
            assert_eq!(i64::from(id), src.id);
            assert_eq!(parts[1].parse::<i32>().unwrap(), src.color);
            assert_eq!(parts[2].parse::<i32>().unwrap(), src.clan_id);
            assert_eq!(parts[3].parse::<i64>().unwrap(), src.time);
            assert_eq!(parts[4], src.nickname);
            assert_eq!(parts[5], src.text);
            assert_eq!(parts[6].parse::<i32>().unwrap(), src.user_id);
        }
    }
}
