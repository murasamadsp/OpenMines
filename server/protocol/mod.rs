pub mod packets;

use anyhow::{Result, bail};
use bytes::{Buf, BufMut, BytesMut};

/// Outer packet: [i32 LE length] [1 byte `data_type`] [2 bytes `event_name`] [payload...]
/// `data_type`: "U" = user, "B" = hub/world, "J" = ?

#[derive(Debug, Clone)]
pub struct Packet {
    pub data_type: u8,
    pub event_name: [u8; 2],
    pub payload: Vec<u8>,
}

impl Packet {
    pub const fn new(data_type: u8, event_name: [u8; 2], payload: Vec<u8>) -> Self {
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
        let mut data = buf.split_to(length);
        data.advance(4); // skip length
        let data_type = data.get_u8();
        let event_name = [data[0], data[1]];
        data.advance(2);
        let payload = data.to_vec();
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
    ev.copy_from_slice(event.as_bytes());
    Packet::new(b'U', ev, payload.to_vec())
}

/// Helper to build a "B" packet
pub fn b_packet(event: &str, payload: &[u8]) -> Packet {
    let mut ev = [0u8; 2];
    ev.copy_from_slice(event.as_bytes());
    Packet::new(b'B', ev, payload.to_vec())
}
