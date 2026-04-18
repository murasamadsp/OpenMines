pub mod buildings;
pub mod chat;
pub mod combat;
pub mod crafting;
pub mod direction;
pub mod player;
pub mod programmator;
pub mod sand;
pub mod skills;

use crate::config::Config;
use crate::db::Database;
use crate::world::{World, WorldProvider};
use bevy_ecs::prelude::{Resource, World as EcsWorld, Entity};
use bevy_ecs::schedule::Schedule;
use dashmap::DashMap;
use parking_lot::RwLock;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;
use std::time::{Duration, Instant};

pub use player::{ActivePlayer, PlayerId, PlayerPosition, PlayerConnection, PlayerStats, PlayerMetadata, PlayerFlags, PlayerUI, PlayerView, PlayerCooldowns, PlayerSettings, PlayerSkills};
pub use buildings::{PackType, PackView, BuildingMetadata, BuildingStats, BuildingStorage, BuildingCrafting, BuildingOwnership, GridPosition, BuildingFlags};

#[derive(Resource)]
pub struct GameStateResource(pub Arc<GameState>);

pub struct GameState {
    pub world: Arc<World>,
    pub db: Arc<Database>,
    pub config: Config,
    pub active_players: DashMap<PlayerId, ActivePlayer>,
    pub chunk_players: DashMap<(u32, u32), Vec<PlayerId>>,
    pub building_index: DashMap<(i32, i32), Entity>,
    pub chat_channels: RwLock<Vec<chat::ChatChannel>>,
    pub ecs: RwLock<EcsWorld>,
    pub schedule: RwLock<Schedule>,
    pub auth_failures: DashMap<std::net::IpAddr, (u32, Instant)>,
}

impl GameState {
    pub const CHUNK_VIEW_RADIUS: i32 = 2;

    pub fn new(world: Arc<World>, database: Arc<Database>, config: Config) -> Arc<Self> {
        let mut schedule = Schedule::default();
        schedule.add_systems(sand::sand_physics_system);
        schedule.add_systems(combat::gun_firing_system);
        schedule.add_systems(programmator::programmator_system);

        let default_channels = vec![
            chat::ChatChannel::new("FED", "Федеральный чат", true),
            chat::ChatChannel::new("DNO", "Дно", true),
            chat::ChatChannel::new("LOC", "Локальный", false),
        ];

        let state = Arc::new(Self {
            world,
            db: database,
            config,
            active_players: DashMap::new(),
            chunk_players: DashMap::new(),
            building_index: DashMap::new(),
            chat_channels: RwLock::new(default_channels),
            ecs: RwLock::new(EcsWorld::new()),
            schedule: RwLock::new(schedule),
            auth_failures: DashMap::new(),
        });

        if let Ok(all_rows) = state.db.load_all_buildings() {
            let count = all_rows.len();
            let mut ecs = state.ecs.write();
            for row in all_rows {
                let pack_type = buildings::PackType::from_str(&row.type_code).unwrap_or(buildings::PackType::Resp);
                let entity = ecs.spawn((
                    BuildingMetadata { id: row.id, pack_type },
                    GridPosition { x: row.x, y: row.y },
                    BuildingStats {
                        charge: row.charge,
                        max_charge: row.max_charge,
                        cost: row.cost,
                        hp: row.hp,
                        max_hp: row.max_hp,
                    },
                    BuildingStorage {
                        money: row.money_inside,
                        crystals: row.crystals_inside,
                        items: row.items_inside.clone(),
                    },
                    BuildingOwnership {
                        owner_id: row.owner_id,
                        clan_id: row.clan_id,
                    },
                    BuildingCrafting {
                        recipe_id: row.craft_recipe_id,
                        num: row.craft_num,
                        end_ts: row.craft_end_ts,
                    },
                    BuildingFlags { dirty: false },
                )).id();
                state.building_index.insert((row.x, row.y), entity);
            }
            info!("Loaded {count} buildings into ECS from DB");
        }
        state
    }

    pub fn get_player_entity(&self, pid: PlayerId) -> Option<Entity> {
        self.active_players.get(&pid).map(|p| p.ecs_entity)
    }

    pub fn query_player<F, R>(&self, pid: PlayerId, f: F) -> Option<R>
    where F: FnOnce(&EcsWorld, Entity) -> R,
    {
        let entity = self.get_player_entity(pid)?;
        let ecs = self.ecs.read();
        Some(f(&ecs, entity))
    }

    pub fn modify_player<F, R>(&self, pid: PlayerId, f: F) -> Option<R>
    where F: FnOnce(&mut EcsWorld, Entity) -> R,
    {
        let entity = self.get_player_entity(pid)?;
        let mut ecs = self.ecs.write();
        Some(f(&mut ecs, entity))
    }

    pub fn modify_building<F, R>(&self, entity: Entity, f: F) -> Option<R>
    where F: FnOnce(&mut EcsWorld, Entity) -> R,
    {
        let mut ecs = self.ecs.write();
        Some(f(&mut ecs, entity))
    }

    pub const AUTH_FAILURE_LIMIT: u32 = 6;
    pub const AUTH_FAILURE_WINDOW: Duration = Duration::from_secs(30);
    pub const AUTH_BLOCK_DURATION: Duration = Duration::from_secs(20);

    pub fn auth_blocked_remaining_by_addr(&self, addr: &std::net::IpAddr, now: Instant) -> Option<Duration> {
        let entry = self.auth_failures.get(addr)?;
        let (fails, last) = *entry.value();
        if fails >= Self::AUTH_FAILURE_LIMIT {
            let elapsed = now.duration_since(last);
            if elapsed < Self::AUTH_BLOCK_DURATION { return Some(Self::AUTH_BLOCK_DURATION - elapsed); }
        }
        None
    }

    pub fn record_auth_failure_by_addr(&self, addr: &std::net::IpAddr, now: Instant) -> Option<Duration> {
        let mut entry = self.auth_failures.entry(*addr).or_insert((0, now));
        let (fails, last) = entry.value_mut();
        if now.duration_since(*last) > Self::AUTH_FAILURE_WINDOW { *fails = 1; }
        else { *fails += 1; }
        *last = now;
        if *fails >= Self::AUTH_FAILURE_LIMIT { Some(Self::AUTH_BLOCK_DURATION) } else { None }
    }

    pub fn clear_auth_failure_by_addr(&self, addr: &std::net::IpAddr) {
        self.auth_failures.remove(addr);
    }

    pub fn prune_auth_failures_by_addr(&self, now: Instant) {
        self.auth_failures.retain(|_, (_, last)| now.duration_since(*last) < Self::AUTH_FAILURE_WINDOW);
    }

    pub fn get_pack_at(&self, x: i32, y: i32) -> Option<PackView> {
        let entity = *self.building_index.get(&(x, y))?;
        let ecs = self.ecs.read();
        let meta = ecs.get::<BuildingMetadata>(entity)?;
        let pos = ecs.get::<GridPosition>(entity)?;
        let ownership = ecs.get::<BuildingOwnership>(entity)?;
        let stats = ecs.get::<BuildingStats>(entity)?;
        
        Some(PackView {
            id: meta.id,
            pack_type: meta.pack_type,
            x: pos.x,
            y: pos.y,
            owner_id: ownership.owner_id,
            clan_id: ownership.clan_id,
            charge: stats.charge,
            max_charge: stats.max_charge,
            hp: stats.hp,
            max_hp: stats.max_hp,
        })
    }

    pub fn find_pack_covering(&self, x: i32, y: i32) -> Option<(i32, i32)> {
        let mut ecs = self.ecs.write();
        let mut query = ecs.query::<(&GridPosition, &BuildingMetadata)>();
        for (pos, meta) in query.iter(&ecs) {
            for (dx, dy, _) in meta.pack_type.building_cells() {
                if pos.x + dx == x && pos.y + dy == y { return Some((pos.x, pos.y)); }
            }
        }
        None
    }

    pub fn pack_block_pos(&self, x: i32, y: i32) -> Option<i32> {
        if x < 0 || y < 0 { return None; }
        let cx = x / 32; let cy = y / 32;
        let w = self.world.chunks_w() as i32;
        if cx >= w || cy >= self.world.chunks_h() as i32 { return None; }
        Some(cy * w + cx)
    }

    pub fn get_packs_in_chunk_area(&self, cx: u32, cy: u32) -> Vec<(u8, u16, u16, u16, u8)> {
        let mut result = Vec::new();
        let mut ecs = self.ecs.write();
        let mut query = ecs.query::<(&GridPosition, &BuildingMetadata, &BuildingOwnership, &BuildingStats)>();
        for (pos, meta, ownership, stats) in query.iter(&ecs) {
            let (pcx, pcy) = crate::world::World::chunk_pos(pos.x, pos.y);
            if (pcx as i64 - cx as i64).abs() <= 1 && (pcy as i64 - cy as i64).abs() <= 1 {
                let cid = ownership.clan_id as u16;
                if self.pack_block_pos(pos.x, pos.y).is_none() { continue; }
                result.push((meta.pack_type.code(), pos.x as u16, pos.y as u16, cid, u8::from(stats.charge > 0.0)));
            }
        }
        result
    }

    pub fn send_to_player(&self, id: PlayerId, data: Vec<u8>) {
        self.query_player(id, |ecs, entity| {
            if let Some(conn) = ecs.get::<PlayerConnection>(entity) { let _ = conn.tx.send(data); }
        });
    }

    pub fn broadcast_to_nearby(&self, cx: u32, cy: u32, data: &[u8], exclude_id: Option<PlayerId>) {
        for (ncx, ncy) in self.visible_chunks_around(cx, cy) {
            if let Some(players) = self.chunk_players.get(&(ncx, ncy)) {
                for &pid in players.value() { if Some(pid) == exclude_id { continue; } self.send_to_player(pid, data.to_vec()); }
            }
        }
    }

    pub fn visible_chunks_around(&self, cx: u32, cy: u32) -> Vec<(u32, u32)> {
        let mut res = Vec::new();
        for dx in -2..=2 { for dy in -2..=2 {
            let ncx = cx as i64 + dx; let ncy = cy as i64 + dy;
            if ncx >= 0 && ncy >= 0 && ncx < self.world.chunks_w() as i64 && ncy < self.world.chunks_h() as i64 {
                res.push((ncx as u32, ncy as u32));
            }
        }}
        res
    }

    pub fn online_count(&self) -> usize { self.active_players.len() }
    pub fn generate_hash() -> String { "TODO_HASH".to_string() }
    pub fn generate_session_id() -> String {
        use rand::Rng;
        let mut rng = rand::rng();
        (0..16).map(|_| rng.sample(rand::distr::Alphanumeric) as char).collect()
    }
    pub fn encode_password_hash(p: &str, h: &str) -> String { format!("{p}:{h}") }
    pub fn verify_password(p: &str, hp: &str, h: &str) -> bool { format!("{p}:{h}") == hp }
    pub fn map_profile_name(n: &str) -> String { n.to_string() }
    pub fn auth_token_hash(s: &str) -> String { s.to_string() }
    pub fn token_matches(t: &str, e: &str) -> bool { t == e }

    pub fn tick(&self) {
        let mut ecs = self.ecs.write();
        let mut schedule = self.schedule.write();
        schedule.run(&mut ecs);
    }
}

pub fn broadcast_cell_update(state: &Arc<GameState>, x: i32, y: i32) {
    use crate::protocol::packets::{hb_cell, hb_bundle};
    use crate::net::session::wire::encode_hb_bundle;
    use crate::world::WorldProvider;
    let sub = hb_cell(x as u16, y as u16, state.world.get_cell(x, y));
    let data = encode_hb_bundle(&hb_bundle(&[sub]).1);
    let (cx, cy) = crate::world::World::chunk_pos(x, y);
    state.broadcast_to_nearby(cx, cy, &data, None);
}
