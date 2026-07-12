use bytes::BytesMut;
use openmines_protocol::{Packet, b_packet, u_packet};

pub fn u_frame(event: [u8; 2], payload: &str) -> Vec<u8> {
    let event = std::str::from_utf8(&event).expect("loadtest event must be ASCII");
    encode_packet(&u_packet(event, payload.as_bytes()))
}

/// TY payload: `[4B event][u32 time][u32 x][u32 y][sub-payload]`.
pub fn ty_frame(inner_event: [u8; 4], time: u32, x: u32, y: u32, sub_payload: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(16 + sub_payload.len());
    payload.extend_from_slice(&inner_event);
    payload.extend_from_slice(&time.to_le_bytes());
    payload.extend_from_slice(&x.to_le_bytes());
    payload.extend_from_slice(&y.to_le_bytes());
    payload.extend_from_slice(sub_payload);
    encode_packet(&b_packet("TY", &payload))
}

fn encode_packet(packet: &Packet) -> Vec<u8> {
    let mut buffer = BytesMut::with_capacity(packet.wire_len());
    packet
        .encode(&mut buffer)
        .expect("loadtest packet must fit wire frame");
    buffer.to_vec()
}
