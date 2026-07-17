use bevy_ecs::prelude::Resource;

use super::super::{PlayerId, SessionId, WorldPos};

#[derive(Debug, Clone)]
pub enum BroadcastEffect {
    Direct {
        session_id: SessionId,
        data: Vec<u8>,
    },
    CellUpdate(WorldPos),
    BlockUpdate(WorldPos),
    Nearby {
        cx: u32,
        cy: u32,
        data: Vec<u8>,
        exclude: Option<PlayerId>,
    },
}

#[derive(Resource, Default)]
pub struct BroadcastQueue(pub Vec<BroadcastEffect>);
