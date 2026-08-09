use crate::game::actors::player::PlayerId;
use crate::game::world::coords::{ChunkPos, WorldPos};
use bevy_ecs::prelude::Entity;
use dashmap::DashMap;

pub struct BuildingIndex {
    pub by_origin: DashMap<WorldPos, Entity>,
    pub chunk_buildings: DashMap<ChunkPos, Vec<Entity>>,
    pub botspot_index: DashMap<PlayerId, Entity>,
    pub chunk_botspots: DashMap<ChunkPos, Vec<Entity>>,
}

impl BuildingIndex {
    pub fn new() -> Self {
        Self {
            by_origin: DashMap::new(),
            chunk_buildings: DashMap::new(),
            botspot_index: DashMap::new(),
            chunk_botspots: DashMap::new(),
        }
    }
}

impl Default for BuildingIndex {
    fn default() -> Self {
        Self::new()
    }
}
