//! Кодирование исходящих пакетов и отладочные превью — отдельно от игровой логики сессии.

use crate::metrics;
use crate::net::session::outbox::Outbox;
use crate::protocol::{b_packet, u_packet};
use bytes::BytesMut;
use std::cell::RefCell;

pub const OUTGOING_PACKET_PREVIEW: usize = 256;
pub const INCOMING_PACKET_PREVIEW: usize = 160;

pub trait PacketSink {
    fn send_packet(&self, packet: Vec<u8>) -> bool;
}

impl PacketSink for Outbox {
    fn send_packet(&self, packet: Vec<u8>) -> bool {
        self.send(packet).is_ok()
    }
}

#[derive(Debug, Default)]
pub struct PacketBatch {
    packets: RefCell<Vec<Vec<u8>>>,
}

impl PacketBatch {
    pub fn into_packets(self) -> Vec<Vec<u8>> {
        self.packets.into_inner()
    }
}

impl PacketSink for PacketBatch {
    fn send_packet(&self, packet: Vec<u8>) -> bool {
        self.packets.borrow_mut().push(packet);
        true
    }
}

pub fn make_u_packet_bytes(event: &str, payload: &[u8]) -> Vec<u8> {
    let p = u_packet(event, payload);
    let mut buf = BytesMut::with_capacity(p.wire_len());
    p.encode(&mut buf).expect("U packet wire length overflow");
    buf.to_vec()
}

pub fn send_u_packet(tx: &dyn PacketSink, event: &str, payload: &[u8]) {
    trace_outgoing_packet("U", event, payload);
    metrics::PACKETS_OUT_TOTAL.with_label_values(&[event]).inc();
    if !tx.send_packet(make_u_packet_bytes(event, payload)) {
        tracing::debug!(event, "Failed to enqueue U packet");
    }
}

pub fn make_b_packet_bytes(event: &str, payload: &[u8]) -> Vec<u8> {
    let p = b_packet(event, payload);
    let mut buf = BytesMut::with_capacity(p.wire_len());
    p.encode(&mut buf).expect("B packet wire length overflow");
    buf.to_vec()
}

pub fn send_b_packet(tx: &dyn PacketSink, event: &str, payload: &[u8]) {
    trace_outgoing_packet("B", event, payload);
    metrics::PACKETS_OUT_TOTAL.with_label_values(&[event]).inc();
    if !tx.send_packet(make_b_packet_bytes(event, payload)) {
        tracing::debug!(event, "Failed to enqueue B packet");
    }
}

pub fn encode_hb_bundle(payload: &[u8]) -> Vec<u8> {
    make_b_packet_bytes("HB", payload)
}

fn trace_outgoing_packet(data_type: &str, event: &str, payload: &[u8]) {
    tracing::debug!(
        direction = "outgoing",
        packet_type = data_type,
        event,
        len = payload.len(),
        payload = %preview_payload(payload),
        "Trace outgoing packet"
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
