use std::collections::VecDeque;

#[derive(Clone)]
pub struct ChatMessage {
    pub time: i64, // unix timestamp / 60000 (minutes)
    pub clan_id: i32,
    pub user_id: i32,
    pub nickname: String,
    pub text: String,
    pub color: i32, // 1=normal, 10=system
}

pub struct ChatChannel {
    pub tag: String,
    pub name: String,
    pub global: bool,
    #[allow(dead_code)]
    pub clan_id: Option<i32>,
    pub messages: VecDeque<ChatMessage>,
}
