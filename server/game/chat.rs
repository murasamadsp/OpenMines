use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
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
            messages: VecDeque::with_capacity(50),
        }
    }
}
