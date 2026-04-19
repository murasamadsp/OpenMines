//! Кодирование исходящих пакетов и отладочные превью — отдельно от игровой логики сессии.

use crate::metrics;
use crate::protocol::{b_packet, u_packet};
use bytes::BytesMut;
use tokio::sync::mpsc;

pub const OUTGOING_PACKET_PREVIEW: usize = 256;
pub const INCOMING_PACKET_PREVIEW: usize = 160;

pub fn make_u_packet_bytes(event: &str, payload: &[u8]) -> Vec<u8> {
    let p = u_packet(event, payload);
    let mut buf = BytesMut::with_capacity(p.wire_len());
    p.encode(&mut buf).expect("U packet wire length overflow");
    buf.to_vec()
}

pub fn send_u_packet(tx: &mpsc::UnboundedSender<Vec<u8>>, event: &str, payload: &[u8]) {
    trace_outgoing_packet("U", event, payload);
    println!("[Net] >>> [U] event={} len={}", event, payload.len());
    metrics::PACKETS_OUT_TOTAL.with_label_values(&[event]).inc();
    let p = u_packet(event, payload);
    let mut buf = BytesMut::with_capacity(p.wire_len());
    p.encode(&mut buf).expect("U packet wire length overflow");
    if let Err(err) = tx.send(buf.to_vec()) {
        tracing::warn!("Failed to enqueue U {event} packet: {err}");
    }
}

pub fn send_b_packet(tx: &mpsc::UnboundedSender<Vec<u8>>, event: &str, payload: &[u8]) {
    trace_outgoing_packet("B", event, payload);
    println!("[Net] >>> [B] event={} len={}", event, payload.len());
    metrics::PACKETS_OUT_TOTAL.with_label_values(&[event]).inc();
    let p = b_packet(event, payload);
    let mut buf = BytesMut::with_capacity(p.wire_len());
    p.encode(&mut buf).expect("B packet wire length overflow");
    if let Err(err) = tx.send(buf.to_vec()) {
        tracing::warn!("Failed to enqueue B {event} packet: {err}");
    }
}

pub fn encode_hb_bundle(payload: &[u8]) -> Vec<u8> {
    let p = b_packet("HB", payload);
    let mut buf = BytesMut::with_capacity(p.wire_len());
    p.encode(&mut buf).expect("HB packet wire length overflow");
    buf.to_vec()
}

fn trace_outgoing_packet(data_type: &str, event: &str, payload: &[u8]) {
    tracing::debug!(
        ">>> [{}] event={} len={} payload={}",
        data_type,
        event,
        payload.len(),
        preview_payload(payload)
    );
}

fn preview_payload(payload: &[u8]) -> String {
    if payload.is_empty() {
        return "empty".to_string();
    }
    let preview_len = payload.len().min(OUTGOING_PACKET_PREVIEW);
    let text_preview = String::from_utf8_lossy(&payload[..preview_len]);
    if payload.len() > preview_len {
        format!("{text_preview:?} (+{} bytes)", payload.len() - preview_len)
    } else {
        format!("{text_preview:?}")
    }
}

pub fn describe_wire_packet(data: &[u8]) -> String {
    if data.len() < 4 {
        return format!("invalid packet (len={})", data.len());
    }
    let declared_len = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let data_type = if data.len() > 4 { data[4] as char } else { '?' };
    let event = if data.len() >= 7 {
        std::str::from_utf8(&data[5..7]).unwrap_or("??")
    } else {
        "??"
    };
    let payload_len = if data.len() >= 7 { data.len() - 7 } else { 0 };
    format!("len={declared_len} type={data_type} event={event} payload_len={payload_len}")
}
