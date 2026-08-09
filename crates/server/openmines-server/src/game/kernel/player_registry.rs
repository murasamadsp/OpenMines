use crate::game::actors::player::{ActivePlayer, PlayerId};
use crate::game::kernel::botspot::BotSpotView;
use crate::game::kernel::schedule::BotsRenderPlayer;
use crate::game::logic::contracts::SessionId;
use crate::game::world::coords::ChunkPos;
use bevy_ecs::prelude::Entity;
use dashmap::DashMap;

pub struct PlayerRegistry {
    pub active_players: DashMap<PlayerId, ActivePlayer>,
    pub player_entities: DashMap<PlayerId, Entity>,
    pub chunk_players: DashMap<ChunkPos, Vec<PlayerId>>,
    pub bots_render_players: DashMap<PlayerId, BotsRenderPlayer>,
    pub bots_render_botspots: DashMap<ChunkPos, Vec<BotSpotView>>,
}

impl PlayerRegistry {
    pub fn new() -> Self {
        Self {
            active_players: DashMap::new(),
            player_entities: DashMap::new(),
            chunk_players: DashMap::new(),
            bots_render_players: DashMap::new(),
            bots_render_botspots: DashMap::new(),
        }
    }
    pub fn get_player_entity(&self, pid: PlayerId) -> Option<Entity> {
        self.player_entities.get(&pid).map(|entry| *entry)
    }

    pub fn is_player_active(&self, pid: PlayerId) -> bool {
        self.active_players.contains_key(&pid)
    }

    pub fn active_player_ids(&self) -> Vec<PlayerId> {
        self.active_players
            .iter()
            .map(|entry| *entry.key())
            .collect()
    }

    pub fn active_session_for_player(&self, pid: PlayerId) -> Option<SessionId> {
        self.active_players
            .get(&pid)
            .map(|active| active.session_id)
    }
}

impl Default for PlayerRegistry {
    fn default() -> Self {
        Self::new()
    }
}
