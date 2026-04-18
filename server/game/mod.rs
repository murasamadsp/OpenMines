pub mod buildings;
pub mod chat;
pub mod combat;
pub mod crafting;
pub mod direction;
pub mod player;
pub mod programmator;
pub mod sand;
pub mod skills;

pub use buildings::*;
pub use chat::*;
pub use player::*;

use crate::config::Config;
use crate::db::DatabaseProvider;
use crate::world::WorldProvider;
use bevy_ecs::prelude::{Component, Resource, Schedule, World as EcsWorld};
use dashmap::DashMap;
use parking_lot::RwLock;

#[derive(Component)]
pub struct PlayerComponent {
    pub pid: i32,
}
use std::collections::VecDeque;
use std::fmt::Write;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct AuthFailureState {
    attempts: u32,
    window_start: Instant,
    blocked_until: Option<Instant>,
}

#[derive(Resource, Clone)]
pub struct GameStateResource(pub Arc<GameState>);

pub fn broadcast_cell_update(state: &GameState, x: i32, y: i32) {
    let new_cell = state.world.get_cell(x, y);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let sub = crate::protocol::packets::hb_cell(x.max(0) as u16, y.max(0) as u16, new_cell);
    let hb_data =
        crate::net::session::wire::encode_hb_bundle(&crate::protocol::packets::hb_bundle(&[sub]).1);
    let (cx, cy) = crate::world::World::chunk_pos(x, y);
    state.broadcast_to_nearby(cx, cy, &hb_data, None);
}

/// Central game state shared across all sessions
pub struct GameState {
    pub world: Arc<dyn WorldProvider>,
    pub db: Arc<dyn DatabaseProvider>,
    pub config: Config,
    pub active_players: DashMap<PlayerId, ActivePlayer>,
    /// Buildings indexed by (x, y) world position
    pub packs: DashMap<(i32, i32), PackData>,
    /// Spatial index: chunk coords -> list of player ids in that chunk
    pub chunk_players: DashMap<(u32, u32), Vec<PlayerId>>,
    auth_failures_by_addr: DashMap<IpAddr, AuthFailureState>,
    /// Chat channels (global and clan)
    pub chat_channels: RwLock<Vec<ChatChannel>>,
    /// ECS World
    pub ecs: RwLock<EcsWorld>,
    /// ECS Schedule
    pub schedule: RwLock<Schedule>,
}

impl GameState {
    pub const AUTH_FAILURE_LIMIT: u32 = 6;
    pub const AUTH_FAILURE_WINDOW: Duration = Duration::from_secs(30);
    pub const AUTH_BLOCK_DURATION: Duration = Duration::from_secs(20);

    const PASSWORD_HASH_ITERATIONS: u32 = 50_000;

    const CHUNK_VIEW_RADIUS: i32 = 2; // 5x5 chunk window with corners disabled by mask

    #[allow(clippy::too_many_lines)]
    pub fn new(
        world: Arc<dyn WorldProvider>,
        db: Arc<dyn DatabaseProvider>,
        config: Config,
    ) -> Arc<Self> {
        tracing::info!(
            world = %world.name(),
            port = config.port,
            chunks_w = world.chunks_w(),
            chunks_h = world.chunks_h(),
            "GameState init"
        );
        let mut default_channels = vec![
            ChatChannel {
                tag: "FED".to_string(),
                name: "Федеральный чат".to_string(),
                global: true,
                clan_id: None,
                messages: VecDeque::new(),
            },
            ChatChannel {
                tag: "TOR".to_string(),
                name: "Торговля".to_string(),
                global: true,
                clan_id: None,
                messages: VecDeque::new(),
            },
            ChatChannel {
                tag: "DNO".to_string(),
                name: "Дно".to_string(),
                global: true,
                clan_id: None,
                messages: VecDeque::new(),
            },
        ];

        // Load history from DB
        for ch in &mut default_channels {
            if let Ok(msgs) = db.get_recent_chat_messages(&ch.tag, 50) {
                for (name, text, ts) in msgs {
                    ch.messages.push_back(ChatMessage {
                        time: ts / 60, // minutes
                        clan_id: 0,
                        user_id: 0, // We don't store user_id in chat_messages table yet, but it's not strictly needed for history display
                        nickname: name,
                        text,
                        color: 1,
                    });
                }
            }
        }

        let mut schedule = Schedule::default();
        schedule.add_systems(crate::game::sand::sand_physics_system);
        schedule.add_systems(crate::game::combat::gun_firing_system);
        schedule.add_systems(crate::game::programmator::programmator_system);

        let state = Arc::new(Self {
            world,
            db,
            config,
            active_players: DashMap::new(),
            packs: DashMap::new(),
            chunk_players: DashMap::new(),
            auth_failures_by_addr: DashMap::new(),
            chat_channels: RwLock::new(default_channels),
            ecs: RwLock::new(EcsWorld::new()),
            schedule: RwLock::new(schedule),
        });
        // Load buildings from DB
        match state.db.load_all_buildings() {
            Ok(buildings) => {
                let mut loaded_count = 0usize;
                for b in &buildings {
                    match PackType::from_str(&b.type_code) {
                        Some(pt) => {
                            let ecs_entity = state
                                .ecs
                                .write()
                                .spawn((
                                    Position { x: b.x, y: b.y },
                                    Building {
                                        id: b.id,
                                        type_code: pt.code(),
                                    },
                                    Owner {
                                        pid: b.owner_id,
                                        clan_id: b.clan_id,
                                    },
                                    Health {
                                        state: b.hp,
                                        max_state: b.max_hp,
                                    },
                                ))
                                .id();

                            let pack = PackData {
                                id: b.id,
                                ecs_entity,
                                pack_type: pt,
                                x: b.x,
                                y: b.y,
                                owner_id: b.owner_id,
                                clan_id: b.clan_id,
                                charge: b.charge,
                                max_charge: b.max_charge,
                                cost: b.cost,
                                hp: b.hp,
                                max_hp: b.max_hp,
                                money_inside: b.money_inside,
                                crystals_inside: b.crystals_inside,
                                items_inside: b.items_inside.clone(),
                                craft_recipe_id: b.craft_recipe_id,
                                craft_num: b.craft_num,
                                craft_end_ts: b.craft_end_ts,
                            };
                            if state.pack_block_pos(pack.x, pack.y).is_none() {
                                tracing::warn!(
                                    "Removing out-of-bounds building id={} type={} at ({}, {})",
                                    b.id,
                                    b.type_code,
                                    b.x,
                                    b.y
                                );
                                if let Err(err) = state.db.delete_building(b.id) {
                                    tracing::warn!(
                                        "Failed to remove out-of-bounds building id={} from DB: {err}",
                                        b.id
                                    );
                                }
                                continue;
                            }
                            if !state.is_pack_inside_world(&pack) {
                                tracing::warn!(
                                    "Removing building with invalid footprint id={} type={} at ({}, {})",
                                    b.id,
                                    b.type_code,
                                    b.x,
                                    b.y
                                );
                                if let Err(err) = state.db.delete_building(b.id) {
                                    tracing::warn!(
                                        "Failed to remove invalid-footprint building id={} from DB: {err}",
                                        b.id
                                    );
                                }
                                continue;
                            }
                            state.packs.insert((b.x, b.y), pack.clone());
                            state.restore_pack_cells(&pack);
                            loaded_count += 1;
                        }
                        None => {
                            tracing::warn!(
                                "Skipping unknown building type={} at ({}, {})",
                                b.type_code,
                                b.x,
                                b.y
                            );
                        }
                    }
                }
                tracing::info!(
                    "Loaded {loaded_count} buildings from DB (total rows: {})",
                    buildings.len()
                );
            }
            Err(e) => {
                tracing::error!("Failed to load buildings: {e}");
            }
        }
        state
    }

    pub fn tick(&self) {
        let mut ecs = self.ecs.write();
        let mut schedule = self.schedule.write();
        schedule.run(&mut ecs);
    }

    /// Client-side map cache name derived from current chunk view profile.
    /// Helps forcing map cache rotation when the visible chunk pattern changes.
    pub fn map_profile_name(&self) -> String {
        format!("{}-v{}", self.world.name(), Self::CHUNK_VIEW_RADIUS)
    }

    fn restore_pack_cells(&self, pack: &PackData) {
        for (dx, dy, cell) in pack.pack_type.building_cells() {
            self.world.set_cell(pack.x + dx, pack.y + dy, cell);
        }
    }

    fn is_pack_inside_world(&self, pack: &PackData) -> bool {
        for (dx, dy, _) in pack.pack_type.building_cells() {
            if !self.world.valid_coord(pack.x + dx, pack.y + dy) {
                return false;
            }
        }
        true
    }

    pub fn pack_block_pos(&self, x: i32, y: i32) -> Option<i32> {
        if x < 0 || y < 0 {
            return None;
        }
        let chunk_x = x / 32;
        let chunk_y = y / 32;
        let width = i32::try_from(self.world.chunks_w()).ok()?;
        let height = i32::try_from(self.world.chunks_h()).ok()?;
        if chunk_x >= width || chunk_y >= height {
            return None;
        }
        chunk_y.checked_mul(width)?.checked_add(chunk_x)
    }

    /// Get pack at world position
    pub fn get_pack_at(
        &self,
        x: i32,
        y: i32,
    ) -> Option<dashmap::mapref::one::Ref<'_, (i32, i32), PackData>> {
        self.packs.get(&(x, y))
    }

    /// Check if any pack occupies cell (x, y) — checks all packs' `building_cells`
    pub fn find_pack_covering(&self, x: i32, y: i32) -> Option<(i32, i32)> {
        // First check direct position
        if self.packs.contains_key(&(x, y)) {
            return Some((x, y));
        }
        // Check if this cell belongs to a nearby pack's footprint
        for entry in &self.packs {
            let pack = entry.value();
            for (dx, dy, _) in pack.pack_type.building_cells() {
                if pack.x + dx == x && pack.y + dy == y {
                    return Some((pack.x, pack.y));
                }
            }
        }
        None
    }

    /// Get all packs visible in a chunk (by chunk coords)
    pub fn get_packs_in_chunk_area(
        &self,
        chunk_x: u32,
        chunk_y: u32,
    ) -> Vec<(u8, u16, u16, u16, u8)> {
        let min_x = i64::from(chunk_x) * 32;
        let min_y = i64::from(chunk_y) * 32;
        let max_x = min_x + 32;
        let max_y = min_y + 32;
        let mut result = Vec::new();
        for entry in &self.packs {
            let p = entry.value();
            let p_x = i64::from(p.x);
            let p_y = i64::from(p.y);
            if p_x >= min_x && p_x < max_x && p_y >= min_y && p_y < max_y {
                let Ok(px) = u16::try_from(p.x.max(0)) else {
                    continue;
                };
                let Ok(py) = u16::try_from(p.y.max(0)) else {
                    continue;
                };
                let Ok(cid) = u16::try_from(p.clan_id.max(0)) else {
                    continue;
                };
                if self.pack_block_pos(p.x, p.y).is_none() {
                    continue;
                }
                result.push((p.pack_type.code(), px, py, cid, p.off()));
            }
        }
        result
    }

    /// Send a raw encoded packet to a specific player
    pub fn send_to_player(&self, id: PlayerId, data: Vec<u8>) {
        if let Some(p) = self.active_players.get(&id) {
            let _ = p.tx.send(data);
        }
    }

    /// Broadcast packet bytes to all players in chunks visible from (`chunk_x`, `chunk_y`)
    pub fn broadcast_to_nearby(
        &self,
        chunk_x: u32,
        chunk_y: u32,
        data: &[u8],
        exclude_id: Option<PlayerId>,
    ) {
        for dx in -(Self::CHUNK_VIEW_RADIUS)..=(Self::CHUNK_VIEW_RADIUS) {
            for dy in -(Self::CHUNK_VIEW_RADIUS)..=(Self::CHUNK_VIEW_RADIUS) {
                let cx = i64::from(chunk_x) + i64::from(dx);
                let cy = i64::from(chunk_y) + i64::from(dy);
                #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                if cx >= 0
                    && cy >= 0
                    && let Some(players) = self.chunk_players.get(&(cx as u32, cy as u32))
                {
                    for &pid in players.value() {
                        if exclude_id == Some(pid) {
                            continue;
                        }
                        self.send_to_player(pid, data.to_vec());
                    }
                }
            }
        }
    }

    /// Get chunks visible around a position (`5x5` area, corner chunks disabled).
    pub fn visible_chunks_around(&self, chunk_x: u32, chunk_y: u32) -> Vec<(u32, u32)> {
        let radius = Self::CHUNK_VIEW_RADIUS;
        #[allow(clippy::cast_sign_loss)]
        let mut result = Vec::with_capacity((radius * 2 + 1) as usize * (radius * 2 + 1) as usize);
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx.abs() == radius && dy.abs() == radius {
                    continue;
                }
                let cx = i64::from(chunk_x) + i64::from(dx);
                let cy = i64::from(chunk_y) + i64::from(dy);
                if cx >= 0 && cy >= 0 {
                    let Ok(cxu) = u32::try_from(cx) else {
                        continue;
                    };
                    let Ok(cyu) = u32::try_from(cy) else {
                        continue;
                    };
                    if cxu < self.world.chunks_w() && cyu < self.world.chunks_h() {
                        result.push((cxu, cyu));
                    }
                }
            }
        }
        result
    }

    pub fn online_count(&self) -> usize {
        self.active_players.len()
    }

    pub fn auth_blocked_remaining_by_addr(&self, addr: &IpAddr, now: Instant) -> Option<Duration> {
        self.auth_failures_by_addr.get(addr).and_then(|entry| {
            entry
                .blocked_until
                .and_then(|until| (now < until).then_some(until - now))
        })
    }

    pub fn record_auth_failure_by_addr(&self, addr: &IpAddr, now: Instant) -> Option<Duration> {
        let mut entry =
            self.auth_failures_by_addr
                .entry(*addr)
                .or_insert_with(|| AuthFailureState {
                    attempts: 0,
                    window_start: now,
                    blocked_until: None,
                });

        if let Some(until) = entry.blocked_until {
            if now < until {
                drop(entry);
                return None;
            }
            entry.blocked_until = None;
            entry.attempts = 0;
            entry.window_start = now;
        }

        if now.duration_since(entry.window_start) > Self::AUTH_FAILURE_WINDOW {
            entry.attempts = 0;
            entry.window_start = now;
        }

        entry.attempts = entry.attempts.saturating_add(1);
        if entry.attempts >= Self::AUTH_FAILURE_LIMIT {
            let until = now + Self::AUTH_BLOCK_DURATION;
            entry.blocked_until = Some(until);
            drop(entry);
            return Some(until.duration_since(now));
        }

        None
    }

    pub fn clear_auth_failure_by_addr(&self, addr: &IpAddr) {
        self.auth_failures_by_addr.remove(addr);
    }

    pub fn prune_auth_failures_by_addr(&self, now: Instant) {
        let mut stale_keys = Vec::<IpAddr>::new();
        for entry in &self.auth_failures_by_addr {
            let state = entry.value();
            let is_stale = state.blocked_until.is_some_and(|until| now >= until)
                || now.duration_since(state.window_start) > Self::AUTH_FAILURE_WINDOW;
            if is_stale {
                stale_keys.push(*entry.key());
            }
        }

        for key in stale_keys {
            self.auth_failures_by_addr.remove(&key);
        }
    }

    pub fn generate_hash() -> String {
        use rand::Rng;
        const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let mut rng = rand::rng();
        (0..12)
            .map(|_| CHARS[rng.random_range(0..CHARS.len())] as char)
            .collect()
    }

    pub fn generate_session_id() -> String {
        use rand::Rng;
        const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let mut rng = rand::rng();
        (0..32)
            .map(|_| CHARS[rng.random_range(0..CHARS.len())] as char)
            .collect()
    }

    pub fn auth_token_hash(input: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut sha2_hasher = Sha256::new();
        sha2_hasher.update(input.as_bytes());
        format!("{:x}", sha2_hasher.finalize())
    }

    pub fn password_hash(password: &str, salt: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut digest = Sha256::new();
        digest.update(salt.as_bytes());
        digest.update(password.as_bytes());
        let mut hash_state = digest.finalize().to_vec();

        for _ in 1..Self::PASSWORD_HASH_ITERATIONS {
            let mut hasher = Sha256::new();
            hasher.update(&hash_state);
            hash_state = hasher.finalize().to_vec();
        }

        let mut password_hash = String::with_capacity(hash_state.len() * 2);
        for value in hash_state {
            write!(&mut password_hash, "{value:02x}").expect("writing to String cannot fail");
        }
        password_hash
    }

    pub fn encode_password_hash(password: &str, salt: &str) -> String {
        format!("p${}", Self::password_hash(password, salt))
    }

    pub fn verify_password(password: &str, stored: &str, salt: &str) -> bool {
        stored.strip_prefix("p$").map_or_else(
            || stored == password,
            |hash| Self::constant_time_eq(password, hash, salt),
        )
    }

    pub fn token_matches(token: &str, expected: &str) -> bool {
        if token.len() != expected.len() {
            return false;
        }
        let mut diff = 0u8;
        let token_bytes = token.as_bytes();
        let expected_bytes = expected.as_bytes();
        for i in 0..token_bytes.len() {
            diff |= token_bytes[i] ^ expected_bytes[i];
        }
        diff == 0
    }

    fn constant_time_eq(password: &str, expected_hash: &str, salt: &str) -> bool {
        let expected = Self::password_hash(password, salt);
        if expected.len() != expected_hash.len() {
            return false;
        }
        let mut diff = 0u8;
        let expected_bytes = expected.as_bytes();
        let actual_bytes = expected_hash.as_bytes();
        for i in 0..expected_bytes.len() {
            diff |= expected_bytes[i] ^ actual_bytes[i];
        }
        diff == 0
    }
}
