use std::borrow::Cow;

/// Decode a TY wrapper: \[4B `event_name`\] \[u32 LE time\] \[u32 LE x\] \[u32 LE y\] \[`sub_payload`...\]
#[derive(Debug, Clone)]
pub struct TyPacket {
    pub event_name: [u8; 4],
    pub time: u32,
    pub x: u32,
    pub y: u32,
    pub sub_payload: bytes::Bytes,
}

impl TyPacket {
    #[must_use]
    pub fn decode(data: &bytes::Bytes) -> Option<Self> {
        if data.len() < 16 {
            return None;
        }
        let event_name = [data[0], data[1], data[2], data[3]];
        let time = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let x = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        let y = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
        let sub_payload = data.slice(16..);
        Some(Self {
            event_name,
            time,
            x,
            y,
            sub_payload,
        })
    }

    #[must_use]
    pub fn event_str(&self) -> &str {
        std::str::from_utf8(&self.event_name).unwrap_or("????")
    }

    #[must_use]
    pub const fn client_timestamp(&self) -> u32 {
        self.time
    }
}

/// Decode an AU packet (client→server): "`uniq_type_token`" or just "sessionid"
pub struct AuClientPacket<'a> {
    pub uniq: &'a str,
    pub auth_type: AuAuthType<'a>,
}

pub enum AuAuthType<'a> {
    NoAuth,
    Regular { user_id: i32, token: &'a str },
}

impl<'a> AuClientPacket<'a> {
    #[must_use]
    pub const fn client_uniq(&self) -> &str {
        self.uniq
    }

    #[must_use]
    pub fn decode(data: &'a [u8]) -> Option<Self> {
        let s = std::str::from_utf8(data).ok()?.trim();
        if s.is_empty() {
            return None;
        }
        let parts: Vec<&str> = s.split('_').collect();
        match parts.as_slice() {
            [_uniq] => {
                // Single-segment payload (no underscores) has no valid auth type.
                None
            }
            [uniq, kind, _rest @ ..] => {
                if uniq.is_empty() || kind.is_empty() {
                    return None;
                }
                let auth_type = match *kind {
                    "NO" | "NO_AUTH" | "NOAUTH" => AuAuthType::NoAuth,
                    _ => {
                        let user_id = (*kind).parse().ok()?;
                        let token_start = uniq.len() + 1 + kind.len() + 1;
                        let token = if token_start <= s.len() {
                            &s[token_start..]
                        } else {
                            ""
                        };
                        AuAuthType::Regular { user_id, token }
                    }
                };
                Some(Self { uniq, auth_type })
            }
            _ => None,
        }
    }
}

/// Decode Pong (client→server, inside TY or standalone): "resp:time"
pub struct PongClient {
    pub response: i32,
    pub current_time: i32,
}

impl PongClient {
    #[must_use]
    pub fn decode(data: &[u8]) -> Option<Self> {
        let s = std::str::from_utf8(data).ok()?;
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 2 {
            return None;
        }
        Some(Self {
            response: parts[0].parse().ok()?,
            current_time: parts[1].parse().ok()?,
        })
    }
}

/// Decode Xmov (inside TY `sub_payload)`: direction as text integer
#[must_use]
pub fn decode_xmov(data: &[u8]) -> Option<i32> {
    parse_i32_text(data)
}

/// Decode Xbld (inside TY `sub_payload)`: "{direction}{blockType}"
/// C# decodes as: dir = int.Parse(data[..^1]), blockType = data[^1..]
pub struct XbldClient<'a> {
    pub direction: i32,
    pub block_type: &'a str,
}

impl<'a> XbldClient<'a> {
    #[must_use]
    pub fn decode(data: &'a [u8]) -> Option<Self> {
        let s = std::str::from_utf8(data).ok()?;
        if s.is_empty() {
            return None;
        }
        let dir_str = &s[..s.len() - 1];
        let block_type = &s[s.len() - 1..];
        let direction = dir_str.parse().ok()?;
        Some(Self {
            direction,
            block_type,
        })
    }
}

/// Decode Xdig (inside TY `sub_payload)`: direction as text integer
#[must_use]
pub fn decode_xdig(data: &[u8]) -> Option<i32> {
    parse_i32_text(data)
}

fn parse_i32_text(data: &[u8]) -> Option<i32> {
    std::str::from_utf8(data).ok()?.trim().parse().ok()
}

/// Decode GUI_ button press (inside TY `sub_payload`).
///
/// C# reference `GUI_Packet.Decode` reads JSON `{"b":"button_name"}`.
/// The current Unity HORB path can also send the button action as raw UTF-8
/// text, so both shapes are part of the observed client wire contract.
#[must_use]
pub fn decode_gui_button(data: &[u8]) -> Option<Cow<'_, str>> {
    let s = std::str::from_utf8(data).ok()?.trim();
    if s.is_empty() {
        return None;
    }

    if s.starts_with('{') {
        let v = serde_json::from_str::<serde_json::Value>(s).ok()?;
        return v
            .get("b")
            .and_then(|b| b.as_str())
            .map(|b| Cow::Owned(b.to_string()));
    }

    Some(Cow::Borrowed(s))
}

/// Decode local chat (inside TY `sub_payload`): the whole payload is UTF-8 text.
pub struct LoclClient<'a> {
    // Kept as byte length for call sites that expect the decoded packet shape.
    pub length: i32,
    pub message: &'a str,
}

impl<'a> LoclClient<'a> {
    #[must_use]
    pub fn decode(data: &'a [u8]) -> Option<Self> {
        let s = std::str::from_utf8(data).ok()?;
        if s.is_empty() {
            return None;
        }

        // Entire payload is the message. Unity sends raw UTF-8 without a
        // `length:` prefix; treating the first colon as metadata truncates chat.
        Some(Self {
            length: i32::try_from(s.len()).unwrap_or(i32::MAX),
            message: s,
        })
    }
}
