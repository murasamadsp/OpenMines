//! Wire-контракт протокола: байт-точные тесты билдеров и декодеров пакетов.
//!
//! Источник правды — документированный формат фрейма
//! `[len i32 LE][type u8][2B event][payload]` и layout HB-подпакетов
//! (`docs/PROTOCOL.md`). Имена событий case-sensitive. Эти тесты стерегут
//! регресс фрейминга/порядка байт/регистра; они НЕ заменяют проверку на
//! живом клиенте (клиент неизменяем и является эталоном).

use super::packets::*;
use super::{Packet, b_packet, u_packet};
use crate::game::chat::ChatMessage;
use bytes::BytesMut;

// ─── Строковые билдеры: event-имя + точный payload ──────────────────────────

#[test]
fn status_builds_st_with_raw_text() {
    let (event, payload) = status("hello");
    assert_eq!(event, "ST");
    assert_eq!(payload, b"hello");
}

#[test]
fn au_session_builds_au_with_sid() {
    let (event, payload) = au_session("abcde");
    assert_eq!(event, "AU");
    assert_eq!(payload, b"abcde");
}

#[test]
fn ping_joins_resp_time_text_with_colons() {
    let (event, payload) = ping(0, 100, "ok");
    assert_eq!(event, "PI");
    assert_eq!(payload, b"0:100:ok");
}

#[test]
fn ping_handles_negative_response() {
    let (_event, payload) = ping(-1, 42, "x");
    assert_eq!(payload, b"-1:42:x");
}

#[test]
fn auth_hash_joins_userid_and_hash_with_underscore() {
    let (event, payload) = auth_hash(42, "deadbeef");
    assert_eq!(event, "AH");
    assert_eq!(payload, b"42_deadbeef");
}

#[test]
fn tp_joins_coords_with_colon() {
    let (event, payload) = tp(3, 4);
    assert_eq!(event, "@T");
    assert_eq!(payload, b"3:4");
}

#[test]
fn tp_handles_negative_coords() {
    let (_event, payload) = tp(-5, -7);
    assert_eq!(payload, b"-5:-7");
}

#[test]
fn gu_close_is_underscore() {
    let (event, payload) = gu_close();
    assert_eq!(event, "Gu");
    assert_eq!(payload, b"_");
}

#[test]
fn ok_message_joins_title_and_body_with_hash() {
    let (event, payload) = ok_message("Title", "Body");
    assert_eq!(event, "OK");
    assert_eq!(payload, b"Title#Body");
}

#[test]
fn speed_joins_three_ints_with_colons() {
    let (event, payload) = speed(200, 100, 1000);
    assert_eq!(event, "sp");
    assert_eq!(payload, b"200:100:1000");
}

#[test]
fn online_joins_count_and_max() {
    let (event, payload) = online(7, 50);
    assert_eq!(event, "ON");
    assert_eq!(payload, b"7:50");
}

#[test]
fn level_is_plain_number() {
    let (event, payload) = level(13);
    assert_eq!(event, "LV");
    assert_eq!(payload, b"13");
}

#[test]
fn geo_is_region_name_not_coords() {
    let (event, payload) = geo("Каньон");
    assert_eq!(event, "GE");
    assert_eq!(payload, "Каньон".as_bytes());
}

#[test]
fn health_joins_hp_and_max() {
    let (event, payload) = health(50, 120);
    assert_eq!(event, "@L");
    assert_eq!(payload, b"50:120");
}

#[test]
fn basket_joins_six_crystals_then_capacity() {
    let (event, payload) = basket(&[1, 2, 3, 4, 5, 6], 100);
    assert_eq!(event, "@B");
    assert_eq!(payload, b"1:2:3:4:5:6:100");
}

#[test]
fn config_packet_is_raw_string() {
    let (event, payload) = config_packet("oldprogramformat+");
    assert_eq!(event, "#F");
    assert_eq!(payload, b"oldprogramformat+");
}

#[test]
fn clan_show_is_clan_id() {
    let (event, payload) = clan_show(77);
    assert_eq!(event, "cS");
    assert_eq!(payload, b"77");
}

#[test]
fn clan_hide_is_empty() {
    let (event, payload) = clan_hide();
    assert_eq!(event, "cH");
    assert!(payload.is_empty());
}

#[test]
fn chat_current_joins_tag_and_name() {
    let (event, payload) = chat_current("FED", "Федерация");
    assert_eq!(event, "mO");
    assert_eq!(payload, "FED:Федерация".as_bytes());
}

#[test]
fn chat_notification_is_count() {
    let (event, payload) = chat_notification(9);
    assert_eq!(event, "mN");
    assert_eq!(payload, b"9");
}

#[test]
fn chat_color_is_code() {
    let (event, payload) = chat_color(15);
    assert_eq!(event, "mC");
    assert_eq!(payload, b"15");
}

// ─── Булевы билдеры: "1"/"0" ────────────────────────────────────────────────

#[test]
fn auto_digg_true_is_one() {
    assert_eq!(auto_digg(true), ("BD", b"1".to_vec()));
}

#[test]
fn auto_digg_false_is_zero() {
    assert_eq!(auto_digg(false), ("BD", b"0".to_vec()));
}

#[test]
fn programmator_status_true_is_one() {
    assert_eq!(programmator_status(true), ("@P", b"1".to_vec()));
}

#[test]
fn programmator_status_false_is_zero() {
    assert_eq!(programmator_status(false), ("@P", b"0".to_vec()));
}

// ─── JSON-билдеры ───────────────────────────────────────────────────────────

#[test]
fn money_uses_fixed_money_then_creds_order() {
    let (event, payload) = money(100, 50);
    assert_eq!(event, "P$");
    assert_eq!(payload, br#"{"money":100,"creds":50}"#);
}

#[test]
fn money_handles_large_values() {
    let (_event, payload) = money(1_000_000, 2_500);
    assert_eq!(payload, br#"{"money":1000000,"creds":2500}"#);
}

#[test]
fn world_info_emits_cf_lowercase_event() {
    // Клиент регистрирует OnU("cf", ...) — case-sensitive, реф ошибочно слал "CF".
    let (event, _payload) = world_info("Mine", 100, 200, 1, "1.0", "", "");
    assert_eq!(event, "cf");
}

#[test]
fn world_info_payload_is_valid_json_with_fields() {
    let (_event, payload) = world_info("Mine", 100, 200, 5, "1.2", "url", "desc");
    let v: serde_json::Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(v["width"], 100);
    assert_eq!(v["height"], 200);
    assert_eq!(v["name"], "Mine");
    assert_eq!(v["v"], 5);
    assert_eq!(v["version"], "1.2");
}

#[test]
fn bot_info_payload_is_valid_json_with_fields() {
    let (event, payload) = bot_info("Гоша", 10, 20, 7);
    assert_eq!(event, "BI");
    let v: serde_json::Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(v["x"], 10);
    assert_eq!(v["y"], 20);
    assert_eq!(v["id"], 7);
    assert_eq!(v["name"], "Гоша");
}

// ─── Композитные строковые билдеры ──────────────────────────────────────────

#[test]
fn settings_default_wire_has_leading_hash_and_all_pairs() {
    let (event, payload) = settings_default_wire();
    assert_eq!(event, "#S");
    assert_eq!(
        payload,
        "#cc#10#snd#0#mus#0#isca#0#tsca#0#mous#1#pot#0#frc#1#ctrl#1#mof#1".as_bytes()
    );
}

#[test]
fn skills_packet_joins_pairs_and_appends_trailing_hash() {
    let skills = vec![("Health".to_string(), 50), ("Movement".to_string(), 25)];
    let (event, payload) = skills_packet(&skills);
    assert_eq!(event, "@S");
    assert_eq!(payload, b"Health:50#Movement:25#");
}

#[test]
fn skills_packet_empty_is_just_trailing_hash() {
    let (_event, payload) = skills_packet(&[]);
    assert_eq!(payload, b"#");
}

#[test]
fn inventory_full_formats_full_selected_grid() {
    let (event, payload) = inventory_full(&[(1, 5), (2, 3)], 1);
    assert_eq!(event, "IN");
    assert_eq!(payload, b"full:1:1#5#2#3");
}

#[test]
fn inventory_full_empty_grid_has_trailing_colon() {
    let (_event, payload) = inventory_full(&[], 0);
    assert_eq!(payload, b"full:0:");
}

#[test]
fn inventory_close_is_close_colon() {
    let (event, payload) = inventory_close();
    assert_eq!(event, "IN");
    assert_eq!(payload, b"close:");
}

#[test]
fn inventory_choose_has_eight_colon_fields_with_empty_grid() {
    // Client `InventoryHandler` requires the "choose" payload to split into 8
    // fields (choose:hint:dist:dx:dy:w:h:grid) before it arms the item via
    // `ChooseInventoryItem` (sets inventoryItem != -1 → enables Enter→INUS).
    // w=0/h=0 makes the client call HideGrid() and ignore the placement grid.
    let (event, payload) = inventory_choose();
    assert_eq!(event, "IN");
    assert_eq!(payload, b"choose::0:0:0:0:0:");
    let s = std::str::from_utf8(&payload).unwrap();
    assert_eq!(s.split(':').count(), 8);
    assert_eq!(s.split(':').next(), Some("choose"));
}

#[test]
fn chat_list_single_entry_uses_pm_separators() {
    let entries = vec![(
        "FED".to_string(),
        true,
        "Федерация".to_string(),
        "привет".to_string(),
    )];
    let (event, payload) = chat_list(&entries);
    assert_eq!(event, "mL");
    assert_eq!(payload, "FED±1±Федерация±привет".as_bytes());
}

#[test]
fn chat_list_notif_false_is_zero() {
    let entries = vec![("DNO".to_string(), false, "Дно".to_string(), "x".to_string())];
    let (_event, payload) = chat_list(&entries);
    assert_eq!(payload, "DNO±0±Дно±x".as_bytes());
}

#[test]
fn chat_list_joins_entries_with_hash() {
    let entries = vec![
        ("A".to_string(), false, "a".to_string(), "1".to_string()),
        ("B".to_string(), true, "b".to_string(), "2".to_string()),
    ];
    let (_event, payload) = chat_list(&entries);
    assert_eq!(payload, "A±0±a±1#B±1±b±2".as_bytes());
}

#[test]
fn chat_messages_has_seven_pm_fields_without_leading_separator() {
    let msgs = [ChatMessage {
        id: 1,
        time: 2,
        clan_id: 3,
        user_id: 4,
        nickname: "n".to_string(),
        text: "t".to_string(),
        color: 5,
    }];
    let (event, payload) = chat_messages("FED", &msgs);
    assert_eq!(event, "mU");
    let v: serde_json::Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(v["ch"], "FED");
    let entry = v["h"][0].as_str().unwrap();
    assert_eq!(entry, "1±5±3±2±n±t±4");
    assert!(
        !entry.starts_with('±'),
        "ведущий ± ломает int.Parse в Unity"
    );
}

// ─── HB-подпакеты: точный байт-layout ───────────────────────────────────────

#[test]
fn hb_map_layout_is_tag_w_h_x_y_cells() {
    let bytes = hb_map(10, 20, 2, 3, &[1, 2, 3, 4, 5, 6]);
    assert_eq!(
        bytes,
        vec![b'M', 2, 3, 0x0A, 0x00, 0x14, 0x00, 1, 2, 3, 4, 5, 6]
    );
}

#[test]
fn hb_cell_is_one_by_one_map() {
    assert_eq!(hb_cell(5, 7, 9), vec![b'M', 1, 1, 5, 0, 7, 0, 9]);
}

#[test]
fn hb_bot_layout_is_dir_skin_tail_id_x_y_clan() {
    let bytes = hb_bot(1, 2, 3, 4, 5, 6, 7);
    assert_eq!(
        bytes,
        vec![b'X', 4, 5, 7, 1, 0, 2, 0, 3, 0, 6, 0],
        "порядок: dir, skin, tail, id(LE), x(LE), y(LE), clan(LE)"
    );
}

#[test]
fn hb_bot_encodes_u16_fields_little_endian() {
    let bytes = hb_bot(300, 0, 0, 0, 0, 0, 0);
    assert_eq!(bytes[4], 0x2C, "low byte of id=300");
    assert_eq!(bytes[5], 0x01, "high byte of id=300");
}

#[test]
fn hb_fx_layout_is_tag_type_x_y() {
    assert_eq!(hb_fx(8, 9, 2), vec![b'F', 2, 8, 0, 9, 0]);
}

#[test]
fn hb_bot_del_layout_is_tag_id_le() {
    assert_eq!(hb_bot_del(300), vec![b'L', 0x2C, 0x01]);
}

#[test]
fn hb_bot_leave_block_layout_is_tag_id_blockpos_i32() {
    assert_eq!(hb_bot_leave_block(1, 2), vec![b'S', 1, 0, 2, 0, 0, 0]);
}

#[test]
fn hb_directed_fx_layout_is_fx_dir_color_x_y_botid() {
    let bytes = hb_directed_fx(1, 2, 3, 4, 5, 6);
    assert_eq!(bytes, vec![b'D', 4, 5, 6, 2, 0, 3, 0, 1, 0]);
}

#[test]
fn hb_chat_layout_is_botid_x_y_strlen_text() {
    let bytes = hb_chat(1, 2, 3, "hi");
    assert_eq!(bytes, vec![b'C', 1, 0, 2, 0, 3, 0, 2, 0, b'h', b'i']);
}

#[test]
fn hb_chat_strlen_counts_utf8_bytes_not_chars() {
    // "я" = 2 байта UTF-8 → strlen=2.
    let bytes = hb_chat(0, 0, 0, "я");
    assert_eq!(bytes[7], 2, "strlen low byte");
    assert_eq!(bytes[8], 0, "strlen high byte");
    assert_eq!(&bytes[9..], "я".as_bytes());
}

#[test]
fn hb_bots_list_layout_is_count_then_ids() {
    assert_eq!(hb_bots_list(&[1, 2]), vec![b'B', 2, 0, 1, 0, 2, 0]);
}

#[test]
fn hb_bots_list_empty_has_zero_count() {
    assert_eq!(hb_bots_list(&[]), vec![b'B', 0, 0]);
}

#[test]
fn hb_gun_layout_is_amount_color_x_y_ids() {
    assert_eq!(hb_gun(2, 3, 4, &[1]), vec![b'Z', 1, 4, 2, 0, 3, 0, 1, 0]);
}

#[test]
fn hb_packs_layout_is_blockpos_count_then_entries() {
    let bytes = hb_packs(2, &[(80, 3, 4, 5, 6)]);
    // [tag 'O'][i32 LE block_pos][u16 LE count][code][x LE][y LE][0][clan][off]
    assert_eq!(bytes, vec![b'O', 2, 0, 0, 0, 1, 0, 80, 3, 0, 4, 0, 0, 5, 6]);
}

#[test]
fn hb_packs_empty_has_zero_count() {
    assert_eq!(hb_packs(0, &[]), vec![b'O', 0, 0, 0, 0, 0, 0]);
}

#[test]
fn hb_bundle_concatenates_subpackets_in_order() {
    let (event, payload) = hb_bundle(&[vec![1, 2], vec![3], vec![4, 5]]);
    assert_eq!(event, "HB");
    assert_eq!(payload, vec![1, 2, 3, 4, 5]);
}

#[test]
fn hb_bundle_empty_is_empty_payload() {
    let (_event, payload) = hb_bundle(&[]);
    assert!(payload.is_empty());
}

// ─── Фрейминг: [len i32 LE][type][2B event][payload] ────────────────────────

#[test]
fn u_packet_frames_with_length_prefix() {
    let mut buf = BytesMut::new();
    u_packet("AU", b"abcde").encode(&mut buf).unwrap();
    assert_eq!(
        buf.as_ref(),
        &[
            0x0C, 0x00, 0x00, 0x00, b'U', b'A', b'U', b'a', b'b', b'c', b'd', b'e'
        ]
    );
}

#[test]
fn b_packet_uses_b_data_type() {
    let mut buf = BytesMut::new();
    b_packet("HB", &[1, 2, 3]).encode(&mut buf).unwrap();
    assert_eq!(
        buf.as_ref(),
        &[0x0A, 0x00, 0x00, 0x00, b'B', b'H', b'B', 1, 2, 3]
    );
}

#[test]
fn empty_payload_frame_is_seven_bytes() {
    let mut buf = BytesMut::new();
    u_packet("Gu", b"").encode(&mut buf).unwrap();
    assert_eq!(buf.len(), 7);
    assert_eq!(buf[0], 7);
}

#[test]
fn encode_decode_roundtrip_preserves_all_fields() {
    let mut buf = BytesMut::new();
    b_packet("HB", &[9, 8, 7, 6]).encode(&mut buf).unwrap();
    let decoded = Packet::try_decode(&mut buf).unwrap().expect("один фрейм");
    assert_eq!(decoded.data_type, b'B');
    assert_eq!(&decoded.event_name, b"HB");
    assert_eq!(decoded.payload, vec![9, 8, 7, 6]);
    assert!(buf.is_empty(), "буфер должен быть осушён");
}

#[test]
fn try_decode_returns_none_when_buffer_shorter_than_declared() {
    let mut buf = BytesMut::from(&[0x0C, 0x00, 0x00, 0x00, b'U'][..]);
    assert!(matches!(Packet::try_decode(&mut buf), Ok(None)));
}

#[test]
fn try_decode_returns_none_when_fewer_than_four_length_bytes() {
    // Меньше 4 байт — даже длину не прочитать: ждём ещё данных, не Err.
    let mut buf = BytesMut::from(&[0x0C, 0x00][..]);
    assert!(matches!(Packet::try_decode(&mut buf), Ok(None)));
    assert_eq!(buf.as_ref(), &[0x0C, 0x00], "буфер не должен расходоваться");
}

#[test]
fn try_decode_rejects_length_below_minimum() {
    let mut buf = BytesMut::from(&[0x06, 0x00, 0x00, 0x00, b'U', b'A'][..]);
    assert!(Packet::try_decode(&mut buf).is_err());
}

#[test]
fn try_decode_rejects_oversized_length() {
    let mut buf = BytesMut::from(&[0xFF, 0xFF, 0xFF, 0x7F, b'U'][..]);
    assert!(Packet::try_decode(&mut buf).is_err());
}

#[test]
fn try_decode_rejects_negative_length() {
    let mut buf = BytesMut::from(&[0xFF, 0xFF, 0xFF, 0xFF, b'U'][..]);
    assert!(Packet::try_decode(&mut buf).is_err());
}

#[test]
fn try_decode_leaves_trailing_bytes_for_next_frame() {
    let mut buf = BytesMut::new();
    u_packet("Gu", b"_").encode(&mut buf).unwrap();
    buf.extend_from_slice(&[0xAA, 0xBB]);
    let _ = Packet::try_decode(&mut buf).unwrap().expect("первый фрейм");
    assert_eq!(buf.as_ref(), &[0xAA, 0xBB]);
}

// ─── Декодеры client→server ─────────────────────────────────────────────────

#[test]
fn ty_packet_decodes_header_and_subpayload() {
    let mut data = b"Xmov".to_vec();
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&2u32.to_le_bytes());
    data.extend_from_slice(&3u32.to_le_bytes());
    data.push(b'5');
    let p = TyPacket::decode(&data).unwrap();
    assert_eq!(p.event_str(), "Xmov");
    assert_eq!(p.time, 1);
    assert_eq!(p.x, 2);
    assert_eq!(p.y, 3);
    assert_eq!(p.sub_payload, vec![b'5']);
}

#[test]
fn ty_packet_rejects_short_header() {
    assert!(TyPacket::decode(&[0u8; 15]).is_none());
}

#[test]
fn ty_packet_accepts_empty_subpayload() {
    let p = TyPacket::decode(&[0u8; 16]).unwrap();
    assert!(p.sub_payload.is_empty());
}

#[test]
fn au_client_no_auth_variant() {
    let p = AuClientPacket::decode(b"abc_NO").unwrap();
    assert_eq!(p.client_uniq(), "abc");
    assert!(matches!(p.auth_type, AuAuthType::NoAuth));
}

#[test]
fn au_client_regular_variant_parses_userid_and_token() {
    let p = AuClientPacket::decode(b"uniq_42_tok").unwrap();
    assert_eq!(p.client_uniq(), "uniq");
    match p.auth_type {
        AuAuthType::Regular { user_id, token } => {
            assert_eq!(user_id, 42);
            assert_eq!(token, "tok");
        }
        AuAuthType::NoAuth => panic!("ожидался Regular"),
    }
}

#[test]
fn au_client_token_may_contain_underscores() {
    let p = AuClientPacket::decode(b"uniq_42_a_b_c").unwrap();
    match p.auth_type {
        AuAuthType::Regular { token, .. } => assert_eq!(token, "a_b_c"),
        AuAuthType::NoAuth => panic!("ожидался Regular"),
    }
}

#[test]
fn au_client_rejects_single_segment() {
    assert!(AuClientPacket::decode(b"justuniq").is_none());
}

#[test]
fn au_client_rejects_empty_uniq() {
    assert!(AuClientPacket::decode(b"_42_tok").is_none());
}

#[test]
fn au_client_rejects_empty_input() {
    assert!(AuClientPacket::decode(b"").is_none());
}

#[test]
fn au_client_rejects_non_numeric_userid() {
    assert!(AuClientPacket::decode(b"uniq_xx_tok").is_none());
}

#[test]
fn pong_client_parses_response_and_time() {
    let p = PongClient::decode(b"5:100").unwrap();
    assert_eq!(p.current_time, 100);
}

#[test]
fn pong_client_rejects_wrong_field_count() {
    assert!(PongClient::decode(b"5").is_none());
    assert!(PongClient::decode(b"5:6:7").is_none());
}

#[test]
fn pong_client_rejects_non_numeric() {
    assert!(PongClient::decode(b"a:b").is_none());
}

#[test]
fn decode_xmov_parses_text_int() {
    assert_eq!(decode_xmov(b"3"), Some(3));
}

#[test]
fn decode_xmov_trims_whitespace_and_handles_negative() {
    assert_eq!(decode_xmov(b"  -1 "), Some(-1));
}

#[test]
fn decode_xmov_rejects_non_numeric() {
    assert_eq!(decode_xmov(b"left"), None);
}

#[test]
fn decode_xdig_parses_text_int() {
    assert_eq!(decode_xdig(b"2"), Some(2));
}

#[test]
fn xbld_splits_trailing_char_as_block_type() {
    let b = XbldClient::decode(b"3G").unwrap();
    assert_eq!(b.direction, 3);
    assert_eq!(b.block_type, "G");
}

#[test]
fn xbld_handles_multidigit_direction() {
    let b = XbldClient::decode(b"12R").unwrap();
    assert_eq!(b.direction, 12);
    assert_eq!(b.block_type, "R");
}

#[test]
fn xbld_rejects_single_char_without_direction() {
    assert!(XbldClient::decode(b"G").is_none());
}

#[test]
fn xbld_rejects_empty() {
    assert!(XbldClient::decode(b"").is_none());
}

#[test]
fn gui_button_parses_json_b_field() {
    assert_eq!(
        decode_gui_button(br#"{"b":"shop"}"#),
        Some("shop".to_string())
    );
}

#[test]
fn gui_button_falls_back_to_raw_string() {
    assert_eq!(decode_gui_button(b"exit"), Some("exit".to_string()));
}

#[test]
fn gui_button_rejects_blank() {
    assert_eq!(decode_gui_button(b"   "), None);
}

#[test]
fn decode_whoi_parses_comma_separated_ids() {
    assert_eq!(decode_whoi(b"1,2,3"), vec![1, 2, 3]);
}

#[test]
fn decode_whoi_skips_unparsable_ids() {
    assert_eq!(decode_whoi(b"1,x,3"), vec![1, 3]);
}

#[test]
fn decode_whoi_empty_is_empty_vec() {
    assert!(decode_whoi(b"").is_empty());
}

#[test]
fn locl_decode_plain_text_keeps_message() {
    let l = LoclClient::decode(b"hello world").unwrap();
    assert_eq!(l.message, "hello world");
}
