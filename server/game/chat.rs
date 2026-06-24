use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Maximum chat history entries sent to the Unity client in one `mU` packet.
pub const CHAT_HISTORY_LIMIT: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// DB rowid (`chat_messages.id`). Клиент читает его как `GCMessage.id`
    /// (`array[0]`) и использует для дедупликации истории (`LastIDs`).
    /// Должен быть первым полем wire-формата `mU`.
    pub id: i64,
    pub time: i64,
    pub clan_id: i32,
    pub user_id: i32,
    pub nickname: String,
    pub text: String,
    pub color: i32,
}

pub struct ChatChannel {
    pub tag: String,
    pub name: String,
    pub global: bool,
    pub messages: VecDeque<ChatMessage>,
}

impl ChatChannel {
    pub fn new(tag: &str, name: &str, global: bool) -> Self {
        Self {
            tag: tag.to_string(),
            name: name.to_string(),
            global,
            messages: VecDeque::with_capacity(CHAT_HISTORY_LIMIT),
        }
    }
}

/// .NET-эпоха (0001-01-01) в тиках по Unix-эпохе (1970-01-01): 1 тик = 100 нс.
const DOTNET_UNIX_EPOCH_TICKS: i64 = 621_355_968_000_000_000;

/// `GCMessage.time` для клиента: минуты с .NET-эпохи.
///
/// Единственный источник правды для `time` и в live, и в истории (был
/// рассинхрон: live слал `as_secs`, история `ts/60`). 1:1 с C#
/// `server_reference` `GLine.time = (int)(DateTime.Now.Ticks/10000L/60000L)`
/// — двухшаговое целочисленное усечение (тики→мс→минуты) воспроизведено
/// точно. Клиент `ChatLineInfo.cs:17` инвертирует: `time*60000L*10000L`
/// тиков → `new DateTime(...)`. См. `docs/CLIENT_PROTOCOL_GAPS.md` §1.
#[must_use]
pub const fn dotnet_epoch_minutes(unix_secs: i64) -> i64 {
    let ticks = DOTNET_UNIX_EPOCH_TICKS + unix_secs * 10_000_000;
    (ticks / 10_000) / 60_000
}

#[cfg(test)]
mod tests {
    use super::dotnet_epoch_minutes;

    #[test]
    fn unix_epoch_maps_to_dotnet_minutes() {
        // 621_355_968_000_000_000 тиков / 10_000 / 60_000 = 1_035_593_280.
        assert_eq!(dotnet_epoch_minutes(0), 1_035_593_280);
    }

    #[test]
    fn one_minute_advances_exactly_one() {
        let base = dotnet_epoch_minutes(0);
        assert_eq!(dotnet_epoch_minutes(60), base + 1);
        assert_eq!(dotnet_epoch_minutes(59), base); // усечение, как в C#
    }

    #[test]
    fn modern_timestamp_fits_client_int32() {
        // 2026-ish: значение должно влезать в i32 (клиент `GCMessage.time`).
        let mins = dotnet_epoch_minutes(1_777_000_000);
        assert!(mins > 0 && mins < i64::from(i32::MAX));
    }
}
