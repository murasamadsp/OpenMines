//! Small, pure decoders for command payloads.
//!
//! Keeping these wire-shape helpers separate makes the command dispatcher easier
//! to scan without changing any accepted legacy payloads.

pub(super) fn decode_program_save(payload: &[u8]) -> Option<(i32, String)> {
    crate::game::programmator::ProgrammatorState::decode_prog_packet(payload)
}

pub(super) fn parse_pack_remove_button(button: &str) -> Option<(i32, i32)> {
    let rest = button.strip_prefix("pack_op:remove:")?;
    let mut parts = rest.split(':');
    let x = parts.next()?.parse::<i32>().ok()?;
    let y = parts.next()?.parse::<i32>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((x, y))
}

pub(super) fn decode_finv_index(payload: &[u8]) -> Option<u8> {
    match payload {
        [b'0'..=b'9'] => Some(payload[0] - b'0'),
        _ => None,
    }
}

pub(super) fn is_unit_payload(payload: &[u8]) -> bool {
    payload == b"_"
}

pub(super) fn decode_miss_enabled(payload: &[u8]) -> Option<bool> {
    match payload {
        b"0" => Some(false),
        b"1" => Some(true),
        _ => None,
    }
}

pub(super) fn decode_rndm_hash(payload: &[u8]) -> Option<&str> {
    const PREFIX: &[u8] = b"hash=";
    let hash = payload.strip_prefix(PREFIX)?;
    std::str::from_utf8(hash).ok()
}

pub(super) fn parse_program_rename_button(button: &str) -> Option<(i32, String)> {
    let rest = button.strip_prefix("rename:")?;
    let (id, name) = rest.split_once(':')?;
    Some((id.parse().ok()?, name.to_owned()))
}
