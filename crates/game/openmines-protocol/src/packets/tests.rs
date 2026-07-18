use super::*;
use crate::chat::{CHAT_HISTORY_LIMIT, ChatMessage};
use crate::{Packet, u_packet};

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

/// Регресс: ник/текст с `\` или `"` ломали `mU` («Invalid escape character»)
/// → весь чат не парсился клиентом. Экранирование делает JSON валидным,
/// а после split('±') текст восстанавливается без потерь.
#[test]
fn chat_messages_escapes_backslash_and_quote() {
    let msgs = [ChatMessage {
        id: 1,
        time: 1,
        clan_id: 0,
        user_id: 1,
        nickname: "ev\\il".to_string(),
        text: "path C:\\x and \"quote\"".to_string(),
        color: 0,
    }];
    let (_, payload) = chat_messages("FED", &msgs);
    // Строгая проверка: JSON обязан парситься (как Unity JsonUtility).
    let json: serde_json::Value =
        serde_json::from_slice(&payload).expect("mU с backslash/quote должен быть валидным JSON");
    let h = json["h"].as_array().unwrap();
    let parts: Vec<&str> = h[0].as_str().unwrap().split('±').collect();
    assert_eq!(parts.len(), 7);
    assert_eq!(parts[4], "ev\\il", "ник восстановлен 1:1");
    assert_eq!(
        parts[5], "path C:\\x and \"quote\"",
        "текст восстановлен 1:1"
    );
}

#[test]
fn chat_messages_trims_to_history_limit() {
    let msgs: Vec<ChatMessage> = (1..=CHAT_HISTORY_LIMIT + 1)
        .map(|id| ChatMessage {
            id: i64::try_from(id).unwrap(),
            time: i64::try_from(id).unwrap(),
            clan_id: 0,
            user_id: i32::try_from(id).unwrap(),
            nickname: format!("n{id}"),
            text: format!("t{id}"),
            color: 0,
        })
        .collect();

    let (_name, payload) = chat_messages("FED", &msgs);
    let json: serde_json::Value = serde_json::from_slice(&payload).unwrap();
    let h = json["h"].as_array().unwrap();
    assert_eq!(h.len(), CHAT_HISTORY_LIMIT);

    let first = h[0].as_str().unwrap();
    let last = h[CHAT_HISTORY_LIMIT - 1].as_str().unwrap();
    assert!(first.starts_with("2±"));
    assert!(last.starts_with(&(CHAT_HISTORY_LIMIT + 1).to_string()));
}
