//! Главный модуль игры: состояние мира, игроки, здания и ECS-системы.
//! Архитектура стремится к 1:1 соответствию логике C# сервера (World.cs, MServer.cs).

pub mod acid;
pub mod alive;
pub mod botspot;
pub mod building_damage;
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
use bevy_ecs::prelude::{Entity, Resource, Schedule, World as EcsWorld};
use dashmap::DashMap;
use parking_lot::{Mutex, RwLock};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub use buildings::{
    BuildingFlags, BuildingMetadata, BuildingOwnership, BuildingStats, GridPosition, PackType,
    PackView,
};
pub use player::{
    ActivePlayer, PlayerConnection, PlayerFlags, PlayerId, PlayerMetadata, PlayerStats,
};

// ─── ECS Resources ──────────────────────────────────────────────────────────

#[derive(Resource)]
pub struct GameStateResource(pub Arc<GameState>);

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

#[derive(Resource, Default)]
pub struct PendingCellConversions(pub Vec<PendingConversion>);

/// Координаты зданий, которым нужен HB O re-broadcast после обнуления charge (C# `ResendPack`).
#[derive(Resource, Default)]
pub struct PackResendQueue(pub Vec<(i32, i32)>);

pub struct PendingConversion {
    pub x: i32,
    pub y: i32,
    pub target_cell: u8,
    pub required_cell: u8,
    pub durability: f32,
    pub ticks_left: u32,
}

// ─── Incoming Actions Queue ──────────────────────────────────────────────────

/// Входящее игровое действие: (игрок, канал ответа, TY-пакет).
pub type IncomingAction = (
    PlayerId,
    tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    crate::protocol::packets::TyPacket,
);
/// Запись персистенции бокса: (координата, `Some`=upsert | `None`=delete).
type BoxPersist = ((i32, i32), Option<[i64; 6]>);
/// Пакет в HB-overlay: (code, x, y, clan, charged).
type PackOverlay = (u8, u16, u16, u8, u8);

pub struct IncomingActionsQueue {
    pub(crate) queue: Mutex<Vec<IncomingAction>>,
}

impl IncomingActionsQueue {
    pub fn new() -> Self {
        Self {
            queue: Mutex::new(Vec::with_capacity(128)),
        }
    }
    pub fn push(
        &self,
        pid: PlayerId,
        tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
        ty: crate::protocol::packets::TyPacket,
    ) {
        self.queue.lock().push((pid, tx, ty));
    }
    pub fn drain(&self) -> Vec<IncomingAction> {
        let mut q = self.queue.lock();
        std::mem::take(&mut *q)
    }
}

// ─── GameState ───────────────────────────────────────────────────────────────

pub struct GameState {
    pub world: Arc<World>,
    pub db: Arc<Database>,
    pub config: Config,
    pub active_players: DashMap<PlayerId, ActivePlayer>,
    pub chunk_players: DashMap<(u32, u32), Vec<PlayerId>>,
    pub building_index: DashMap<(i32, i32), Entity>,
    pub botspot_index: DashMap<PlayerId, Entity>,
    pub chunk_botspots: DashMap<(u32, u32), Vec<Entity>>,
    pub chunk_buildings: DashMap<(u32, u32), Vec<Entity>>,
    pub chat_channels: RwLock<Vec<chat::ChatChannel>>,
    pub ecs: RwLock<EcsWorld>,
    pub schedule: RwLock<Schedule>,
    pub auth_failures: DashMap<std::net::IpAddr, (u32, Instant)>,
    pub incoming_actions: IncomingActionsQueue,
    pub player_sessions: DashMap<PlayerId, tokio::sync::mpsc::UnboundedSender<Vec<u8>>>,
    /// Боксы (ячейка 90) в памяти — авторитетно. Read/изменение без `SQLite`
    /// (был фриз: sync `SQLite` по боксам под `ecs.write()` в physics-системе
    /// каждые 10ms — `combat.rs` C-1). Персистенция отложена в `box_persist_q`.
    pub box_index: DashMap<(i32, i32), [i64; 6]>,
    /// Очередь персистенции боксов: `(coord, Some(crystals)=upsert | None=delete)`.
    /// Дренится в game-tick ВНЕ `ecs.write()` → `spawn_blocking` (`SQLite`).
    pub box_persist_q: Mutex<Vec<BoxPersist>>,
}

impl GameState {
    pub const CHUNK_VIEW_RADIUS: i32 = 2;

    pub fn new(world: Arc<World>, database: Arc<Database>, config: Config) -> Arc<Self> {
        let mut schedule = Schedule::default();
        schedule.add_systems(sand::sand_physics_system);
        schedule.add_systems(combat::standing_cell_hazard_system);
        schedule.add_systems(combat::gun_firing_system);
        schedule.add_systems(programmator::programmator_system);
        schedule.add_systems(alive::alive_physics_system);
        schedule.add_systems(acid::acid_physics_system);
        schedule.add_systems(building_damage::building_hourly_damage_system);
        schedule.add_systems(building_damage::building_effect_tick_system);

        let mut default_channels = vec![
            chat::ChatChannel::new("FED", "Федеральный чат", true),
            chat::ChatChannel::new("DNO", "Дно", true),
            chat::ChatChannel::new("LOC", "Локальный", false),
        ];
        // Восстанавливаем историю глобальных каналов из БД: в C# `Chat.messages`
        // — персистентный список (EF), а live-путь (`handle_channel_chat`) и
        // история при логине (`send_chat_login_per_reference`) читают in-mem
        // `ChatChannel.messages`. Без загрузки FED/DNO теряли бы всю историю
        // после рестарта сервера. `id` сохраняется для дедупа клиента.
        for ch in default_channels.iter_mut().filter(|c| c.global) {
            if let Ok(rows) = database.get_recent_chat_messages(&ch.tag, 50) {
                for (id, name, text, ts, player_id, color, clan_id) in rows {
                    ch.messages.push_back(chat::ChatMessage {
                        id,
                        // .NET-минуты (не unix /60) — иначе live ≠ история,
                        // клиент рисует не то время. CLIENT_PROTOCOL_GAPS §1.
                        time: chat::dotnet_epoch_minutes(ts),
                        clan_id,
                        // gid = author player_id: `gid<=0` → клиент fontSize
                        // 10 + без времени/id (легаси-строки player_id=0
                        // остаются мелкими — автор невосстановим).
                        user_id: player_id,
                        nickname: name,
                        text,
                        color,
                    });
                }
            }
        }

        let state = Arc::new(Self {
            world,
            db: database,
            config,
            active_players: DashMap::new(),
            chunk_players: DashMap::new(),
            building_index: DashMap::new(),
            botspot_index: DashMap::new(),
            chunk_botspots: DashMap::new(),
            chunk_buildings: DashMap::new(),
            chat_channels: RwLock::new(default_channels),
            ecs: RwLock::new(EcsWorld::new()),
            schedule: RwLock::new(schedule),
            auth_failures: DashMap::new(),
            incoming_actions: IncomingActionsQueue::new(),
            player_sessions: DashMap::new(),
            box_index: DashMap::new(),
            box_persist_q: Mutex::new(Vec::new()),
        });

        // Боксы из БД → in-memory индекс (один раз; на hot-path SQLite по
        // боксам больше не дёргаем).
        match state.db.load_all_boxes() {
            Ok(rows) => {
                for (bx, by, crystals) in rows {
                    state.box_index.insert((bx, by), crystals);
                }
                tracing::info!(
                    "Loaded {} boxes into in-memory index",
                    state.box_index.len()
                );
            }
            Err(e) => tracing::error!("Failed to load boxes into index: {e}"),
        }

        {
            let mut ecs = state.ecs.write();
            ecs.insert_resource(GameStateResource(state.clone()));
            ecs.insert_resource(combat::DeathQueue::default());
            ecs.insert_resource(BroadcastQueue::default());
            ecs.insert_resource(ProgrammatorQueue::default());
            ecs.insert_resource(alive::AliveTickTimer::default());
            ecs.insert_resource(acid::AcidTickTimer::default());
            ecs.insert_resource(sand::SandTickTimer::default());
            ecs.insert_resource(building_damage::BuildingDamageTimer::default());
            ecs.insert_resource(PendingCellConversions::default());
            ecs.insert_resource(PackResendQueue::default());
        }

        Self::load_buildings_into_ecs(&state);
        state
    }

    /// Загрузить все здания из БД в ECS (вынесено из `new` — лимит строк).
    fn load_buildings_into_ecs(state: &Arc<Self>) {
        let Ok(all_rows) = state.db.load_all_buildings() else {
            return;
        };
        let count = all_rows.len();
        let mut ecs = state.ecs.write();
        let mut spot_count = 0u32;
        for row in all_rows {
            let entity = buildings::spawn_building_from_row(&mut ecs, &row);
            state.building_index.insert((row.x, row.y), entity);
            let (cx, cy) = World::chunk_pos(row.x, row.y);
            state
                .chunk_buildings
                .entry((cx, cy))
                .or_default()
                .push(entity);

            let pack_type =
                buildings::PackType::from_str(&row.type_code).unwrap_or(buildings::PackType::Resp);
            if pack_type == buildings::PackType::Spot {
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
                state
                    .chunk_botspots
                    .entry((cx, cy))
                    .or_default()
                    .push(botspot_entity);
                spot_count += 1;
            }
        }
        drop(ecs);
        tracing::info!(
            "Loaded {count} buildings into ECS from DB ({spot_count} Spot BotSpots spawned)"
        );
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

    pub fn modify_building<F, R>(&self, entity: Entity, f: F) -> R
    where
        F: FnOnce(&mut EcsWorld, Entity) -> R,
    {
        let mut ecs = self.ecs.write();
        f(&mut ecs, entity)
    }

    pub const AUTH_FAILURE_LIMIT: u32 = 6;
    pub const AUTH_FAILURE_WINDOW: Duration = Duration::from_secs(30);
    pub const AUTH_BLOCK_DURATION: Duration = Duration::from_secs(20);

    pub fn auth_blocked_remaining_by_addr(
        &self,
        addr: &std::net::IpAddr,
        now: Instant,
    ) -> Option<Duration> {
        let (fails, last) = {
            let entry = self.auth_failures.get(addr)?;
            *entry.value()
        };
        if fails >= Self::AUTH_FAILURE_LIMIT {
            let elapsed = now.duration_since(last);
            if elapsed < Self::AUTH_BLOCK_DURATION {
                return Some(Self::AUTH_BLOCK_DURATION.saturating_sub(elapsed));
            }
        }
        None
    }

    pub fn record_auth_failure_by_addr(
        &self,
        addr: &std::net::IpAddr,
        now: Instant,
    ) -> Option<Duration> {
        let exceeded = {
            let mut entry = self.auth_failures.entry(*addr).or_insert((0, now));
            let (fails, last) = entry.value_mut();
            if now.duration_since(*last) > Self::AUTH_FAILURE_WINDOW {
                *fails = 1;
            } else {
                *fails += 1;
            }
            *last = now;
            let e = *fails >= Self::AUTH_FAILURE_LIMIT;
            drop(entry);
            e
        };
        if exceeded {
            Some(Self::AUTH_BLOCK_DURATION)
        } else {
            None
        }
    }

    pub fn prune_auth_failures_by_addr(&self, now: Instant) {
        self.auth_failures
            .retain(|_, (_, last)| now.duration_since(*last) < Self::AUTH_FAILURE_WINDOW);
    }

    pub fn get_pack_at(&self, x: i32, y: i32) -> Option<PackView> {
        let entity = *self.building_index.get(&(x, y))?;
        let view = {
            let ecs = self.ecs.read();
            let meta = ecs.get::<BuildingMetadata>(entity)?;
            let pos = ecs.get::<GridPosition>(entity)?;
            let ownership = ecs.get::<BuildingOwnership>(entity)?;
            let stats = ecs.get::<BuildingStats>(entity)?;
            let v = PackView {
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
            };
            drop(ecs);
            v
        };
        Some(view)
    }

    pub(crate) fn find_pack_covering_with(
        ecs: &EcsWorld,
        chunk_buildings: &DashMap<(u32, u32), Vec<Entity>>,
        x: i32,
        y: i32,
    ) -> Option<(i32, i32)> {
        let (cx, cy) = World::chunk_pos(x, y);
        for ncy in (cy.cast_signed() - 1)..=(cy.cast_signed() + 1) {
            for ncx in (cx.cast_signed() - 1)..=(cx.cast_signed() + 1) {
                if ncx < 0 || ncy < 0 {
                    continue;
                }
                if let Some(entities) =
                    chunk_buildings.get(&(ncx.cast_unsigned(), ncy.cast_unsigned()))
                {
                    let ents = entities.value().clone();
                    drop(entities);
                    for entity in ents {
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
                }
            }
        }
        None
    }

    pub fn find_pack_covering(&self, x: i32, y: i32) -> Option<(i32, i32)> {
        let ecs = self.ecs.read();
        Self::find_pack_covering_with(&ecs, &self.chunk_buildings, x, y)
    }

    pub fn pack_block_pos(&self, x: i32, y: i32) -> Option<i32> {
        if !self.world.valid_coord(x, y) {
            return None;
        }
        let w = self.world.chunks_w().cast_signed();
        y.checked_mul(w)?.checked_add(x)
    }

    pub(crate) fn access_gun_with(
        ecs: &EcsWorld,
        chunk_buildings: &DashMap<(u32, u32), Vec<Entity>>,
        x: i32,
        y: i32,
        player_clan_id: i32,
    ) -> bool {
        let mut ret = true;
        let (cx, cy) = World::chunk_pos(x, y);
        for ncy in (cy.cast_signed() - 1)..=(cy.cast_signed() + 1) {
            for ncx in (cx.cast_signed() - 1)..=(cx.cast_signed() + 1) {
                if ncx < 0 || ncy < 0 {
                    continue;
                }
                if let Some(entities) =
                    chunk_buildings.get(&(ncx.cast_unsigned(), ncy.cast_unsigned()))
                {
                    let ents = entities.value().clone();
                    drop(entities);
                    for entity in ents {
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
                            let ddx = f64::from(bx - x);
                            let ddy = f64::from(by - y);
                            if ddx.hypot(ddy) <= 20.0 {
                                ret = ret && own.clan_id == player_clan_id;
                            }
                        }
                    }
                }
            }
        }
        ret
    }

    pub fn access_gun(&self, x: i32, y: i32, player_clan_id: i32) -> bool {
        let ecs = self.ecs.read();
        Self::access_gun_with(&ecs, &self.chunk_buildings, x, y, player_clan_id)
    }

    pub fn get_packs_in_chunk_area(&self, cx: u32, cy: u32) -> Vec<PackOverlay> {
        let mut results = Vec::new();
        let ecs = self.ecs.read();
        for (ucx, ucy) in self.visible_chunks_around(cx, cy) {
            if let Some(entities) = self.chunk_buildings.get(&(ucx, ucy)) {
                let ents = entities.value().clone();
                drop(entities);
                for entity in ents {
                    let pos = ecs.get::<GridPosition>(entity);
                    let meta = ecs.get::<BuildingMetadata>(entity);
                    let own = ecs.get::<BuildingOwnership>(entity);
                    let stats = ecs.get::<BuildingStats>(entity);
                    if let (Some(pos), Some(meta), Some(own), Some(stats)) = (pos, meta, own, stats)
                        && meta.pack_type.included_in_hb_overlay()
                    {
                        results.push((
                            meta.pack_type.code(),
                            u16::try_from(pos.x.rem_euclid(65536)).unwrap_or(0),
                            u16::try_from(pos.y.rem_euclid(65536)).unwrap_or(0),
                            u8::try_from(own.clan_id.clamp(0, 255)).unwrap_or(0),
                            u8::from(stats.charge > 0.0),
                        ));
                    }
                }
            }
        }
        drop(ecs);
        results
    }

    pub fn visible_chunks_around(&self, cx: u32, cy: u32) -> Vec<(u32, u32)> {
        let mut chunks = Vec::new();
        let (w, h) = (self.world.chunks_w(), self.world.chunks_h());
        for dy in -Self::CHUNK_VIEW_RADIUS..=Self::CHUNK_VIEW_RADIUS {
            for dx in -Self::CHUNK_VIEW_RADIUS..=Self::CHUNK_VIEW_RADIUS {
                let ncx = cx.cast_signed() + dx;
                let ncy = cy.cast_signed() + dy;
                if ncx >= 0 && ncx < w.cast_signed() && ncy >= 0 && ncy < h.cast_signed() {
                    chunks.push((ncx.cast_unsigned(), ncy.cast_unsigned()));
                }
            }
        }
        chunks
    }

    pub fn send_to_player(&self, pid: PlayerId, data: Vec<u8>) {
        if let Some(tx) = self.player_sessions.get(&pid) {
            let _ = tx.send(data);
        }
    }

    // ─── Боксы: in-memory, без SQLite на hot-path (фикс фриза C-1/C-2/H-1) ──

    /// Атомарно забрать бокс (pickup): удалить из индекса, вернуть кристаллы,
    /// поставить delete в очередь персистенции. Без `SQLite` (lock-free `DashMap`).
    pub fn box_take(&self, x: i32, y: i32) -> Option<[i64; 6]> {
        let removed = self.box_index.remove(&(x, y)).map(|(_, v)| v);
        if removed.is_some() {
            self.box_persist_q.lock().push(((x, y), None));
        }
        removed
    }

    /// Положить/обновить бокс (death drop): индекс + upsert в очередь.
    pub fn box_put(&self, x: i32, y: i32, crystals: [i64; 6]) {
        self.box_index.insert((x, y), crystals);
        self.box_persist_q.lock().push(((x, y), Some(crystals)));
    }

    /// Слить очередь персистенции боксов (вызывать ВНЕ `ecs.write()`).
    pub fn drain_box_persist(&self) -> Vec<BoxPersist> {
        let mut q = self.box_persist_q.lock();
        std::mem::take(&mut *q)
    }

    pub fn broadcast_to_nearby(&self, cx: u32, cy: u32, data: &[u8], exclude_id: Option<PlayerId>) {
        for (ncx, ncy) in self.visible_chunks_around(cx, cy) {
            if let Some(players) = self.chunk_players.get(&(ncx, ncy)) {
                let pids = players.value().clone();
                drop(players);
                for pid in pids {
                    if Some(pid) == exclude_id {
                        continue;
                    }
                    self.send_to_player(pid, data.to_vec());
                }
            }
        }
    }

    pub fn broadcast_to_nearby_specific_chunk(
        &self,
        cx: u32,
        cy: u32,
        data: &[u8],
        exclude_id: Option<PlayerId>,
    ) {
        if let Some(players) = self.chunk_players.get(&(cx, cy)) {
            let pids = players.value().clone();
            drop(players);
            for pid in pids {
                if Some(pid) == exclude_id {
                    continue;
                }
                self.send_to_player(pid, data.to_vec());
            }
        }
    }

    pub fn generate_hash() -> String {
        use rand::Rng;
        const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let mut rng = rand::rng();
        (0..12)
            .map(|_| CHARSET[rng.random_range(0..CHARSET.len())] as char)
            .collect()
    }
    pub fn generate_session_id() -> String {
        use rand::Rng;
        const CHARSET: &[u8] = b"abcdefghijklmnoprtsuxyz0123456789";
        let mut rng = rand::rng();
        (0..5)
            .map(|_| CHARSET[rng.random_range(0..CHARSET.len())] as char)
            .collect()
    }

    pub fn auth_token_hash_md5(hash: &str, sid: &str) -> String {
        let digest = md5::compute(format!("{hash}{sid}").as_bytes());
        format!("{digest:x}")
    }

    pub fn auth_token_hash_sha256(hash: &str, sid: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(format!("{hash}{sid}").as_bytes());
        format!("{:x}", hasher.finalize())
    }

    pub fn token_matches_legacy_auth(token: &str, hash: &str, sid: &str) -> bool {
        token == Self::auth_token_hash_md5(hash, sid)
            || token == Self::auth_token_hash_sha256(hash, sid)
    }
}

pub fn broadcast_cell_update(state: &Arc<GameState>, x: i32, y: i32) {
    use crate::net::session::wire::encode_hb_bundle;
    use crate::protocol::packets::{hb_bundle, hb_cell};
    let sub = hb_cell(
        u16::try_from(x.rem_euclid(65536)).unwrap_or(0),
        u16::try_from(y.rem_euclid(65536)).unwrap_or(0),
        state.world.get_cell(x, y),
    );
    let data = encode_hb_bundle(&hb_bundle(&[sub]).1);
    let (cx, cy) = World::chunk_pos(x, y);
    state.broadcast_to_nearby(cx, cy, &data, None);
}
