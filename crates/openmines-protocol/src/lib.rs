pub mod chat;
pub mod packets;

#[cfg(test)]
mod wire_tests;

use anyhow::{Result, bail};
use bytes::{BufMut, BytesMut};

/// Outer wire packet.
///
/// Layout: `[i32 LE length][1 byte data_type][2 bytes event_name][payload...]`.
/// `data_type`: `U` = user/string, `B` = hub/world, `J` = JSON.
#[derive(Debug, Clone)]
pub struct Packet {
    pub data_type: u8,
    pub event_name: [u8; 2],
    pub payload: bytes::Bytes,
}

impl Packet {
    #[must_use]
    pub const fn new(data_type: u8, event_name: [u8; 2], payload: bytes::Bytes) -> Self {
        Self {
            data_type,
            event_name,
            payload,
        }
    }

    /// Total wire length including the 4-byte length prefix
    #[must_use]
    pub const fn wire_len(&self) -> usize {
        4 + 1 + 2 + self.payload.len()
    }

    /// Encode this packet into the legacy Unity wire frame.
    ///
    /// # Errors
    ///
    /// Returns an error when the packet length does not fit the signed 32-bit
    /// length prefix used by the client protocol.
    pub fn encode(&self, buf: &mut BytesMut) -> Result<()> {
        let total = i32::try_from(self.wire_len())
            .map_err(|_| anyhow::anyhow!("packet wire length overflow"))?;
        buf.put_i32_le(total);
        buf.put_u8(self.data_type);
        buf.put_slice(&self.event_name);
        buf.put_slice(&self.payload);
        Ok(())
    }

    /// Try to decode one packet from the buffer.
    ///
    /// Returns `Ok(None)` if the buffer does not contain a full frame yet.
    ///
    /// # Errors
    ///
    /// Returns an error when the length prefix is invalid, smaller than the
    /// minimal frame size, or larger than the protocol limit.
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

    #[must_use]
    pub fn event_str(&self) -> &str {
        std::str::from_utf8(&self.event_name).unwrap_or("??")
    }
}

/// Helper to build a "U" packet
#[must_use]
pub fn u_packet(event: &str, payload: &[u8]) -> Packet {
    let mut ev = [0u8; 2];
    let bytes = event.as_bytes();
    let len = bytes.len().min(2);
    ev[..len].copy_from_slice(&bytes[..len]);
    Packet::new(b'U', ev, bytes::Bytes::copy_from_slice(payload))
}

/// Helper to build a "B" packet
#[must_use]
pub fn b_packet(event: &str, payload: &[u8]) -> Packet {
    let mut ev = [0u8; 2];
    let bytes = event.as_bytes();
    let len = bytes.len().min(2);
    ev[..len].copy_from_slice(&bytes[..len]);
    Packet::new(b'B', ev, bytes::Bytes::copy_from_slice(payload))
}
