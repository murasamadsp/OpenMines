use crate::db::PlayerRow;
use std::time::Instant;
use tokio::sync::mpsc;

pub type PlayerId = i32;

#[allow(dead_code)]
/// Live player state kept in memory during gameplay
pub struct ActivePlayer {
    pub data: PlayerRow,
    pub tx: mpsc::UnboundedSender<Vec<u8>>,
    pub last_chunk: Option<(u32, u32)>,
    pub visible_chunks: Vec<(u32, u32)>,
    pub auto_dig: bool,
    pub inv_selected: i32,
    pub current_window: Option<String>, // tracks which GUI window is open
    pub dirty: bool,
    pub current_chat: String, // tag of the current chat channel
    pub ecs_entity: bevy_ecs::entity::Entity,
    pub last_move_ts: Instant,
    pub last_dig_ts: Instant,
    pub protection_until: Option<Instant>,
    pub last_shot_ts: Option<Instant>,
}

#[allow(dead_code)]
impl ActivePlayer {
    pub fn chunk_x(&self) -> u32 {
        self.data.x.max(0).cast_unsigned() / 32
    }

    pub fn chunk_y(&self) -> u32 {
        self.data.y.max(0).cast_unsigned() / 32
    }
}
