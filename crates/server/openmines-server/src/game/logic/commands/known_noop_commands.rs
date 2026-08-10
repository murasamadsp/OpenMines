//! Known legacy TY no-op handling and its typed presentation wrapper.

use crate::game::logic::commands::parsing::{
    decode_finv_index, decode_miss_enabled, decode_rndm_hash, is_unit_payload,
};
use crate::game::{CommandEffects, GameState};
use std::sync::Arc;

fn handle_known_noop_ty(
    tx: &dyn crate::net::session::wire::PacketSink,
    player_id: crate::game::PlayerId,
    event: &str,
    payload: &[u8],
) {
    match event {
        "Xhur" => {
            if is_unit_payload(payload) {
                tracing::debug!(pid = %player_id, "known no-op TY event: self-hurt");
            } else {
                tracing::warn!(pid = %player_id, payload = ?payload, "invalid Xhur payload");
            }
        }
        "FINV" => {
            if let Some(index) = decode_finv_index(payload) {
                tracing::debug!(pid = %player_id, index, "known no-op TY event: inventory filter hotkey");
            } else {
                tracing::warn!(pid = %player_id, payload = ?payload, "invalid FINV payload");
            }
        }
        "Help" => crate::game::logic::commands_social::send_ok(
            tx,
            "Справка",
            "Справка пока не подключена на сервере.",
        ),
        "Miso" => {
            let (event, payload) = crate::protocol::packets::mission_panel("", 0, 0, 0, "");
            crate::net::session::wire::send_u_packet(tx, event, &payload);
        }
        "THID" => {
            let marker = String::from_utf8_lossy(payload);
            tracing::debug!(pid = %player_id, marker = %marker, "tutorial marker hidden");
        }
        "Miss" => {
            if let Some(enabled) = decode_miss_enabled(payload) {
                tracing::debug!(pid = %player_id, enabled, "known no-op TY event: mission init");
            } else {
                tracing::warn!(pid = %player_id, payload = ?payload, "invalid Miss payload");
            }
        }
        "Rndm" => {
            if let Some(hash) = decode_rndm_hash(payload) {
                tracing::debug!(pid = %player_id, hash_len = hash.len(), "known no-op TY event: device hash");
            } else {
                tracing::warn!(pid = %player_id, payload = ?payload, "invalid Rndm payload");
            }
        }
        "TAUR" => {
            if is_unit_payload(payload) {
                tracing::debug!(pid = %player_id, "known no-op TY event: auto-respawn toggle");
            } else {
                tracing::warn!(pid = %player_id, payload = ?payload, "invalid TAUR payload");
            }
        }
        _ => tracing::warn!(pid = %player_id, event, "unknown no-op TY command"),
    }
}

pub(super) fn apply_known_noop_ty(
    state: &Arc<GameState>,
    session_id: crate::game::SessionId,
    player_id: crate::game::PlayerId,
    event: &str,
    payload: &[u8],
) -> CommandEffects {
    if state.sessions.session_for_player(player_id) != Some(session_id) {
        return CommandEffects::default();
    }
    let batch = crate::net::session::wire::PacketBatch::default();
    handle_known_noop_ty(&batch, player_id, event, payload);
    let packets = batch.into_packets();
    if packets.is_empty() {
        return CommandEffects::default();
    }
    CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets,
        }],
        ..CommandEffects::default()
    }
}
