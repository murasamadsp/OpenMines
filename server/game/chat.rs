use std::collections::VecDeque;

// Re-export shared chat models and helpers
pub use openmines_shared::protocol::chat::{CHAT_HISTORY_LIMIT, ChatMessage, dotnet_epoch_minutes};

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
