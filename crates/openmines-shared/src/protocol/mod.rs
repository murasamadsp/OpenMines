pub mod chat;
pub mod packets;

#[cfg(test)]
mod wire_tests;

use anyhow::{Result, bail};
use bytes::{BufMut, BytesMut};

/// Outer packet: [i32 LE length] [1 byte `data_type`] [2 bytes `event_name`] [payload...]
/// `data_type`: "U" = user, "B" = hub/world, "J" = ?

#[derive(Debug, Clone)]
pub struct Packet {
    pub data_type: u8,
    pub event_name: [u8; 2],
    pub payload: bytes::Bytes,
}

impl Packet {
    pub const fn new(data_type: u8, event_name: [u8; 2], payload: bytes::Bytes) -> Self {
        Self {
            data_type,
            event_name,
            payload,
        }
    }

    /// Total wire length including the 4-byte length prefix
    pub const fn wire_len(&self) -> usize {
        4 + 1 + 2 + self.payload.len()
    }

    pub fn encode(&self, buf: &mut BytesMut) -> Result<()> {
        let total = i32::try_from(self.wire_len())
            .map_err(|_| anyhow::anyhow!("packet wire length overflow"))?;
        buf.put_i32_le(total);
        buf.put_u8(self.data_type);
        buf.put_slice(&self.event_name);
        buf.put_slice(&self.payload);
        Ok(())
    }

    /// Try to decode one packet from the buffer. Returns None if not enough data.
    pub fn try_decode(buf: &mut BytesMut) -> Result<Option<Self>> {
        const MAX_PACKET_LEN: usize = 65536;
        if buf.len() < 4 {
            return Ok(None);
        }
        let length_raw = i32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let length = usize::try_from(length_raw)
            .map_err(|_| anyhow::anyhow!("invalid packet length: {length_raw}"))?;
        if length < 7 {
            bail!("packet too small: {length}");
        }
        if length > MAX_PACKET_LEN {
            bail!("packet too large: {length}");
        }
        if buf.len() < length {
            return Ok(None);
        }
        let data = buf.split_to(length).freeze();
        let data_type = data[4];
        let event_name = [data[5], data[6]];
        let payload = data.slice(7..);
        Ok(Some(Self {
            data_type,
            event_name,
            payload,
        }))
    }

    pub fn event_str(&self) -> &str {
        std::str::from_utf8(&self.event_name).unwrap_or("??")
    }
}

/// Helper to build a "U" packet
pub fn u_packet(event: &str, payload: &[u8]) -> Packet {
    let mut ev = [0u8; 2];
    let bytes = event.as_bytes();
    let len = bytes.len().min(2);
    ev[..len].copy_from_slice(&bytes[..len]);
    Packet::new(b'U', ev, bytes::Bytes::copy_from_slice(payload))
}

/// Helper to build a "B" packet
pub fn b_packet(event: &str, payload: &[u8]) -> Packet {
    let mut ev = [0u8; 2];
    let bytes = event.as_bytes();
    let len = bytes.len().min(2);
    ev[..len].copy_from_slice(&bytes[..len]);
    Packet::new(b'B', ev, bytes::Bytes::copy_from_slice(payload))
}
