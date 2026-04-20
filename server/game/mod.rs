pub mod acid;
pub mod alive;
pub mod botspot;
pub mod buildings;
pub mod chat;
pub mod combat;
pub mod crafting;
pub mod direction;
pub mod market;
pub mod player;
pub mod programmator;
pub mod sand;
pub mod skills;

use crate::config::Config;
use crate::db::Database;
use crate::world::{World, WorldProvider};
use bevy_ecs::prelude::{Entity, Resource, World as EcsWorld};
use bevy_ecs::schedule::Schedule;
use dashmap::DashMap;
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::info;

pub use buildings::{
    BuildingCrafting, BuildingFlags, BuildingMetadata, BuildingOwnership, BuildingStats,
    BuildingStorage, GridPosition, PackType, PackView,
};
pub use player::{
    ActivePlayer, PlayerConnection, PlayerFlags, PlayerId, PlayerMetadata, PlayerStats,
};

#[derive(Resource)]
pub struct GameStateResource(pub Arc<GameState>);

/// Отложенные broadcast'ы из ECS-систем.
/// Нельзя вызывать `broadcast_to_nearby`/`broadcast_cell_update` изнутри `schedule.run()`:
/// они ре-лочат `ecs.read()` а schedule уже держит `ecs.write()` → self-deadlock.
#[derive(Resource, Default)]
pub struct BroadcastQueue(pub Vec<BroadcastEffect>);

pub enum BroadcastEffect {
    CellUpdate(i32, i32),
    Nearby {
        cx: u32,
        cy: u32,
        data: Vec<u8>,
        exclude: Option<PlayerId>,
    },
}

/// Отложенные команды программатора (handle_move/handle_dig ре-лочат ecs).
#[derive(Resource, Default)]
pub struct ProgrammatorQueue(pub Vec<ProgrammatorAction>);

pub enum ProgrammatorAction {
    Move {
        pid: PlayerId,
        tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
        x: i32,
        y: i32,
        dir: i32,
    },
    Dig {
        pid: PlayerId,
        tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
        dir: i32,
    },
    Build {
        pid: PlayerId,
        tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
        dir: i32,
        block_type: String,
    },
    Geo {
        pid: PlayerId,
        tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    },
    Heal {
        pid: PlayerId,
        tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    },
    SetAutoDig {
        pid: PlayerId,
        tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
        enabled: bool,
    },
}

pub struct GameState {
    pub world: Arc<World>,
    pub db: Arc<Database>,
    pub config: Config,
    pub active_players: DashMap<PlayerId, ActivePlayer>,
    pub chunk_players: DashMap<(u32, u32), Vec<PlayerId>>,
    pub building_index: DashMap<(i32, i32), Entity>,
    /// BotSpot entities indexed by owner player ID.
    /// Each Spot building spawns one BotSpot entity.
    pub botspot_index: DashMap<PlayerId, Entity>,
    pub chat_channels: RwLock<Vec<chat::ChatChannel>>,
    pub ecs: RwLock<EcsWorld>,
    pub schedule: RwLock<Schedule>,
    pub auth_failures: DashMap<std::net::IpAddr, (u32, Instant)>,
}

impl GameState {
    // TODO: will be used when chunk-visibility logic is refactored
    #[allow(dead_code)]
    pub const CHUNK_VIEW_RADIUS: i32 = 2;

    pub fn new(world: Arc<World>, database: Arc<Database>, config: Config) -> Arc<Self> {
        let mut schedule = Schedule::default();
        schedule.add_systems(sand::sand_physics_system);
        schedule.add_systems(combat::standing_cell_hazard_system);
        schedule.add_systems(combat::gun_firing_system);
        schedule.add_systems(programmator::programmator_system);
        schedule.add_systems(alive::alive_physics_system);
        schedule.add_systems(acid::acid_physics_system);

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
            botspot_index: DashMap::new(),
            chat_channels: RwLock::new(default_channels),
            ecs: RwLock::new(EcsWorld::new()),
            schedule: RwLock::new(schedule),
            auth_failures: DashMap::new(),
        });

        {
            let mut ecs = state.ecs.write();
            ecs.insert_resource(combat::DeathQueue::default());
            ecs.insert_resource(BroadcastQueue::default());
            ecs.insert_resource(ProgrammatorQueue::default());
            ecs.insert_resource(alive::AliveTickTimer::default());
            ecs.insert_resource(acid::AcidTickTimer::default());
        }

        if let Ok(all_rows) = state.db.load_all_buildings() {
            let count = all_rows.len();
            let mut ecs = state.ecs.write();
            let mut spot_count = 0u32;
            for row in all_rows {
                let pack_type = buildings::PackType::from_str(&row.type_code)
                    .unwrap_or(buildings::PackType::Resp);
                let entity = ecs
                    .spawn((
                        BuildingMetadata {
                            id: row.id,
                            pack_type,
                        },
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
                    ))
                    .id();
                state.building_index.insert((row.x, row.y), entity);

                // Spawn BotSpot entity for Spot buildings loaded from DB.
                if pack_type == PackType::Spot {
                    let botspot_entity = ecs
                        .spawn((
                            botspot::BotSpotMarker,
                            botspot::BotSpotData {
                                bot_id: -row.owner_id,
                                owner_id: row.owner_id,
                                clan_id: row.clan_id,
                                x: row.x,
                                y: row.y,
                                dir: 0,
                                building_entity: entity,
                            },
                            botspot::BotSpotBasket::default(),
                            programmator::ProgrammatorState::new(),
                        ))
                        .id();
                    state.botspot_index.insert(row.owner_id, botspot_entity);
                    spot_count += 1;
                }
            }
            info!("Loaded {count} buildings into ECS from DB ({spot_count} Spot BotSpots spawned)");
        }
        state
    }

    pub fn get_player_entity(&self, pid: PlayerId) -> Option<Entity> {
        self.active_players.get(&pid).map(|p| p.ecs_entity)
    }

    pub fn query_player<F, R>(&self, pid: PlayerId, f: F) -> Option<R>
    where
        F: FnOnce(&EcsWorld, Entity) -> R,
    {
        let entity = self.get_player_entity(pid)?;
        let ecs = self.ecs.read();
        Some(f(&ecs, entity))
    }

    pub fn modify_player<F, R>(&self, pid: PlayerId, f: F) -> Option<R>
    where
        F: FnOnce(&mut EcsWorld, Entity) -> R,
    {
        let entity = self.get_player_entity(pid)?;
        let mut ecs = self.ecs.write();
        Some(f(&mut ecs, entity))
    }

    pub fn modify_building<F, R>(&self, entity: Entity, f: F) -> Option<R>
    where
        F: FnOnce(&mut EcsWorld, Entity) -> R,
    {
        let mut ecs = self.ecs.write();
        Some(f(&mut ecs, entity))
    }

    pub const AUTH_FAILURE_LIMIT: u32 = 6;
    pub const AUTH_FAILURE_WINDOW: Duration = Duration::from_secs(30);
    pub const AUTH_BLOCK_DURATION: Duration = Duration::from_secs(20);

    pub fn auth_blocked_remaining_by_addr(
        &self,
        addr: &std::net::IpAddr,
        now: Instant,
    ) -> Option<Duration> {
        let entry = self.auth_failures.get(addr)?;
        let (fails, last) = *entry.value();
        if fails >= Self::AUTH_FAILURE_LIMIT {
            let elapsed = now.duration_since(last);
            if elapsed < Self::AUTH_BLOCK_DURATION {
                return Some(Self::AUTH_BLOCK_DURATION - elapsed);
            }
        }
        None
    }

    pub fn record_auth_failure_by_addr(
        &self,
        addr: &std::net::IpAddr,
        now: Instant,
    ) -> Option<Duration> {
        let mut entry = self.auth_failures.entry(*addr).or_insert((0, now));
        let (fails, last) = entry.value_mut();
        if now.duration_since(*last) > Self::AUTH_FAILURE_WINDOW {
            *fails = 1;
        } else {
            *fails += 1;
        }
        *last = now;
        if *fails >= Self::AUTH_FAILURE_LIMIT {
            Some(Self::AUTH_BLOCK_DURATION)
        } else {
            None
        }
    }

    // TODO: will be used when auth-failure clearing on successful login is wired up
    #[allow(dead_code)]
    pub fn clear_auth_failure_by_addr(&self, addr: &std::net::IpAddr) {
        self.auth_failures.remove(addr);
    }

    pub fn prune_auth_failures_by_addr(&self, now: Instant) {
        self.auth_failures
            .retain(|_, (_, last)| now.duration_since(*last) < Self::AUTH_FAILURE_WINDOW);
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

    /// Покрытие клетки (x,y) зданием — без отдельного `ecs` lock (для вызова из `modify_player`).
    pub(crate) fn find_pack_covering_with(
        ecs: &EcsWorld,
        building_index: &DashMap<(i32, i32), Entity>,
        x: i32,
        y: i32,
    ) -> Option<(i32, i32)> {
        for entry in building_index.iter() {
            let entity = *entry.value();
            let Some(pos) = ecs.get::<GridPosition>(entity) else {
                continue;
            };
            let Some(meta) = ecs.get::<BuildingMetadata>(entity) else {
                continue;
            };
            for (dx, dy, _) in meta.pack_type.building_cells() {
                if pos.x + dx == x && pos.y + dy == y {
                    return Some((pos.x, pos.y));
                }
            }
        }
        None
    }

    pub fn find_pack_covering(&self, x: i32, y: i32) -> Option<(i32, i32)> {
        let ecs = self.ecs.read();
        Self::find_pack_covering_with(&ecs, &self.building_index, x, y)
    }

    pub fn pack_block_pos(&self, x: i32, y: i32) -> Option<i32> {
        // 1:1 reference (`Chunk.PACKPOS`): `x + y * World.ChunksW`
        // IMPORTANT: x/y are world cell coordinates (not chunk coords).
        if !self.world.valid_coord(x, y) {
            return None;
        }
        let w = self.world.chunks_w() as i32;
        y.checked_mul(w)?.checked_add(x)
    }

    /// Как `World.AccessGun` — без отдельного lock ECS.
    pub(crate) fn access_gun_with(
        ecs: &EcsWorld,
        building_index: &DashMap<(i32, i32), Entity>,
        x: i32,
        y: i32,
        player_clan_id: i32,
    ) -> bool {
        let mut ret = true;
        for entry in building_index.iter() {
            let entity = *entry.value();
            let Some(pos) = ecs.get::<GridPosition>(entity) else {
                continue;
            };
            let Some(meta) = ecs.get::<BuildingMetadata>(entity) else {
                continue;
            };
            let Some(stats) = ecs.get::<BuildingStats>(entity) else {
                continue;
            };
            let Some(own) = ecs.get::<BuildingOwnership>(entity) else {
                continue;
            };
            if meta.pack_type != PackType::Gun || stats.charge <= 0.0 {
                continue;
            }
            for (dx, dy, _) in meta.pack_type.building_cells() {
                let bx = pos.x + dx;
                let by = pos.y + dy;
                let ddx = (bx - x) as f32;
                let ddy = (by - y) as f32;
                if (ddx * ddx + ddy * ddy).sqrt() <= 20.0 {
                    ret = ret && own.clan_id == player_clan_id;
                }
            }
        }
        ret
    }

    /// Как `World.AccessGun` (`World.cs`): заряженные пушки `Gun` в радиусе 20 клеток от (x, y)
    /// должны принадлежать клану `player_clan_id` (0 — без клана).
    pub fn access_gun(&self, x: i32, y: i32, player_clan_id: i32) -> bool {
        let ecs = self.ecs.read();
        Self::access_gun_with(&ecs, &self.building_index, x, y, player_clan_id)
    }

    /// Как `Chunk.pPacks`: только паки с типом != `PackType.None` (ворота в референсе — `None`, в HB не попадают).
    /// Для `HBPack` как в референсе: `(byte)cid` на проводе.
    pub fn get_packs_in_chunk_area(&self, cx: u32, cy: u32) -> Vec<(u8, u16, u16, u8, u8)> {
        let mut result = Vec::new();
        let ecs = self.ecs.read();
        for entry in self.building_index.iter() {
            let entity = *entry.value();
            let Some(pos) = ecs.get::<GridPosition>(entity) else {
                continue;
            };
            let Some(meta) = ecs.get::<BuildingMetadata>(entity) else {
                continue;
            };
            let Some(ownership) = ecs.get::<BuildingOwnership>(entity) else {
                continue;
            };
            let Some(stats) = ecs.get::<BuildingStats>(entity) else {
                continue;
            };
            let (pcx, pcy) = crate::world::World::chunk_pos(pos.x, pos.y);
            if (pcx as i64 - cx as i64).abs() <= 1 && (pcy as i64 - cy as i64).abs() <= 1 {
                if !meta.pack_type.included_in_hb_overlay() {
                    continue;
                }
                let cid = ownership.clan_id.clamp(0, 255) as u8;
                if self.pack_block_pos(pos.x, pos.y).is_none() {
                    continue;
                }
                result.push((
                    meta.pack_type.code(),
                    pos.x as u16,
                    pos.y as u16,
                    cid,
                    u8::from(stats.charge > 0.0),
                ));
            }
        }
        result
    }

    pub fn send_to_player(&self, id: PlayerId, data: Vec<u8>) {
        self.query_player(id, |ecs, entity| {
            if let Some(conn) = ecs.get::<PlayerConnection>(entity) {
                let _ = conn.tx.send(data);
            }
        });
    }

    pub fn broadcast_to_nearby(&self, cx: u32, cy: u32, data: &[u8], exclude_id: Option<PlayerId>) {
        for (ncx, ncy) in self.visible_chunks_around(cx, cy) {
            if let Some(players) = self.chunk_players.get(&(ncx, ncy)) {
                for &pid in players.value() {
                    if Some(pid) == exclude_id {
                        continue;
                    }
                    self.send_to_player(pid, data.to_vec());
                }
            }
        }
    }

    pub fn visible_chunks_around(&self, cx: u32, cy: u32) -> Vec<(u32, u32)> {
        let mut res = Vec::new();
        for dx in -2..=2 {
            for dy in -2..=2 {
                let ncx = cx as i64 + dx;
                let ncy = cy as i64 + dy;
                if ncx >= 0
                    && ncy >= 0
                    && ncx < self.world.chunks_w() as i64
                    && ncy < self.world.chunks_h() as i64
                {
                    res.push((ncx as u32, ncy as u32));
                }
            }
        }
        res
    }

    // TODO: will be used when online/status endpoint is wired
    #[allow(dead_code)]
    pub fn online_count(&self) -> usize {
        self.active_players.len()
    }

    /// Как `Player.GenerateHash()` в `Player.cs`: 12 символов `A-Z0-9`.
    pub fn generate_hash() -> String {
        use rand::Rng;
        const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let mut rng = rand::rng();
        (0..12)
            .map(|_| CHARSET[rng.random_range(0..CHARSET.len())] as char)
            .collect()
    }
    /// Как `Auth.GenerateSessionId()` в server_reference: длина 5, алфавит без `q`/`v`/`w`.
    pub fn generate_session_id() -> String {
        use rand::Rng;
        const CHARSET: &[u8] = b"abcdefghijklmnoprtsuxyz0123456789";
        let mut rng = rand::rng();
        (0..5)
            .map(|_| CHARSET[rng.random_range(0..CHARSET.len())] as char)
            .collect()
    }
    // TODO: will be used when password hashing is fully wired into auth
    #[allow(dead_code)]
    pub fn encode_password_hash(p: &str, h: &str) -> String {
        format!("{p}:{h}")
    }
    // TODO: will be used when password verification is fully wired into auth
    #[allow(dead_code)]
    pub fn verify_password(p: &str, hp: &str, h: &str) -> bool {
        format!("{p}:{h}") == hp
    }
    // TODO: will be used when profile name mapping is wired
    #[allow(dead_code)]
    pub fn map_profile_name(n: &str) -> String {
        n.to_string()
    }

    /// Как `Auth.CalculateMD5Hash` в `server_reference/Server/Auth.cs` (ASCII `hash+sid`, hex lowercase).
    pub fn auth_token_hash_md5(hash: &str, sid: &str) -> String {
        let input = format!("{hash}{sid}");
        let digest = md5::compute(input.as_bytes());
        format!("{digest:x}")
    }

    /// Некоторые клиенты считают SHA256 — оставляем второй вариант для совместимости.
    pub fn auth_token_hash_sha256(hash: &str, sid: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(format!("{hash}{sid}").as_bytes());
        format!("{:x}", hasher.finalize())
    }

    #[must_use]
    pub fn token_matches_legacy_auth(token: &str, hash: &str, sid: &str) -> bool {
        token == Self::auth_token_hash_md5(hash, sid)
            || token == Self::auth_token_hash_sha256(hash, sid)
    }

    // TODO: will be used when tick-based game loop is fully connected
    #[allow(dead_code)]
    pub fn tick(&self) {
        let mut ecs = self.ecs.write();
        let mut schedule = self.schedule.write();
        schedule.run(&mut ecs);
    }
}

pub fn broadcast_cell_update(state: &Arc<GameState>, x: i32, y: i32) {
    use crate::net::session::wire::encode_hb_bundle;
    use crate::protocol::packets::{hb_bundle, hb_cell};
    use crate::world::WorldProvider;
    let sub = hb_cell(x as u16, y as u16, state.world.get_cell(x, y));
    let data = encode_hb_bundle(&hb_bundle(&[sub]).1);
    let (cx, cy) = crate::world::World::chunk_pos(x, y);
    state.broadcast_to_nearby(cx, cy, &data, None);
}
