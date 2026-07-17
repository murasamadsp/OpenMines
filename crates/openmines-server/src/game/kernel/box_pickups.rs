use bevy_ecs::prelude::Resource;
use parking_lot::Mutex;
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use super::super::{PlayerId, SessionId, WorldPos};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BoxPickupSource {
    Standing,
    Dig {
        session_id: Option<SessionId>,
        direction: i32,
        skin: i32,
        clan_id: i32,
        tail: u8,
        exclude_self: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoxPickupIntent {
    pub player_id: PlayerId,
    pub player_pos: WorldPos,
    pub box_pos: WorldPos,
    pub source: BoxPickupSource,
}

#[derive(Default)]
struct BoxPickupQueueState {
    queue: VecDeque<BoxPickupIntent>,
    players: HashSet<PlayerId>,
}

#[derive(Resource, Clone, Default)]
pub struct BoxPickupQueue(Arc<Mutex<BoxPickupQueueState>>);

impl BoxPickupQueue {
    pub fn push(&self, intent: BoxPickupIntent) {
        let mut state = self.0.lock();
        if state.players.insert(intent.player_id) {
            state.queue.push_back(intent);
        }
    }

    pub fn drain(&self) -> Vec<BoxPickupIntent> {
        let mut state = self.0.lock();
        state.players.clear();
        state.queue.drain(..).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.0.lock().queue.is_empty()
    }
}
