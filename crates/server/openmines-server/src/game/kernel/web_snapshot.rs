use crate::game::actors::player::PlayerId;
use crate::game::structures::buildings::PackType;
use parking_lot::RwLock;
use std::sync::Arc;

#[derive(Clone, Debug, Default)]
pub struct WebSnapshot {
    pub players: Vec<WebPlayerInfo>,
    pub buildings: Vec<WebBuildingInfo>,
}

#[derive(Clone, Debug)]
pub struct WebPlayerInfo {
    pub id: PlayerId,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub health: i32,
    pub max_health: i32,
    pub crystals: i64,
    pub money: i64,
    pub creds: i64,
    pub role: i32,
}

#[derive(Clone, Debug)]
pub struct WebBuildingInfo {
    pub x: i32,
    pub y: i32,
    pub pack_type: PackType,
    pub hp: i32,
    pub max_hp: i32,
    pub clan_id: i32,
}

pub struct WebSnapshotOwner {
    snapshot: RwLock<Arc<WebSnapshot>>,
}

impl WebSnapshotOwner {
    pub fn new() -> Self {
        Self {
            snapshot: RwLock::new(Arc::new(WebSnapshot::default())),
        }
    }

    pub fn snapshot(&self) -> Arc<WebSnapshot> {
        self.snapshot.read().clone()
    }

    pub fn update(&self, state: &crate::game::GameState) {
        let mut ecs = state.ecs_write_profiled("web.update_snapshot");
        let mut players = Vec::new();
        for pid in state.active_player_ids() {
            if let Some(entity) = state.get_player_entity(pid)
                && let Some(pos) = ecs.get::<crate::game::player::PlayerPosition>(entity)
                && let Some(p_stats) = ecs.get::<crate::game::player::PlayerStats>(entity)
                && let Some(meta) = ecs.get::<crate::game::player::PlayerMetadata>(entity)
            {
                players.push(WebPlayerInfo {
                    id: pid,
                    name: meta.name.clone(),
                    x: pos.x,
                    y: pos.y,
                    health: p_stats.health,
                    max_health: p_stats.max_health,
                    crystals: p_stats.crystals.iter().sum(),
                    money: p_stats.money,
                    creds: p_stats.creds,
                    role: p_stats.role,
                });
            }
        }
        let mut b_query = ecs.query::<(
            &crate::game::buildings::GridPosition,
            &crate::game::buildings::BuildingMetadata,
            &crate::game::buildings::BuildingStats,
            &crate::game::buildings::BuildingOwnership,
        )>();
        let mut buildings = Vec::new();
        for (grid_pos, metadata, stats, ownership) in b_query.iter(&ecs) {
            buildings.push(WebBuildingInfo {
                x: grid_pos.x,
                y: grid_pos.y,
                pack_type: metadata.pack_type,
                hp: stats.hp,
                max_hp: stats.max_hp,
                clan_id: ownership.clan_id,
            });
        }
        drop(ecs);
        *self.snapshot.write() = Arc::new(WebSnapshot { players, buildings });
    }
}

impl Default for WebSnapshotOwner {
    fn default() -> Self {
        Self::new()
    }
}
