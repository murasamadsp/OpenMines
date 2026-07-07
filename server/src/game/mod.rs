//! Главный модуль игры: состояние мира, игроки, здания и ECS-системы.
//! Архитектура стремится к 1:1 соответствию логике C# сервера (World.cs, MServer.cs).

pub mod actors;
pub mod economy;
pub mod mechanics;
pub mod structures;
pub mod world;

pub use actors::{alive, botspot, player, programmator};
pub use economy::{crafting, market};
pub use mechanics::{building_damage, chat, combat, skills};
pub use structures::buildings;
pub use world::{direction, sand};

use crate::config::Config;
use crate::config::ProgrammatorConfig;
use crate::db::{Database, buildings::BuildingExtra};
use crate::world::{World, WorldProvider};
use anyhow::Context as _;
use bevy_ecs::prelude::{Entity, Resource, Schedule, World as EcsWorld};
use dashmap::DashMap;
use parking_lot::{Mutex, RwLock};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub use actors::player::{
    ActivePlayer, PlayerConnection, PlayerFlags, PlayerId, PlayerMetadata, PlayerStats,
};
pub use mechanics::events::{ActiveEvent, ActiveEvents, ExpContext};
pub use structures::buildings::{
    BuildingFlags, BuildingMetadata, BuildingOwnership, BuildingSpawnSpec, BuildingStats,
    GridPosition, PackType, PackView,
};
pub use world::coords::{ChunkPos, WorldPos};

// ─── ECS Resources (вместо Arc<GameState> для ECS-систем) ───────────────────

/// Мир (карта/клетки) — выделен из `GameState`, чтобы ECS-системы не зависели
/// от всего `GameState`. Каждая система берёт только то, что реально использует.
#[derive(Resource)]
pub struct WorldResource(pub Arc<crate::world::World>);

#[derive(Resource, Clone, Copy)]
pub struct ProgrammatorConfigResource(pub ProgrammatorConfig);

/// Индекс боксов (crystal loot на земле) — lock-free `DashMap`, общий с `GameState`
/// для консистентности между ECS-системами и async-хендлерами.
#[derive(Resource, Clone)]
pub struct BoxIndexResource(pub Arc<DashMap<WorldPos, [i64; 6]>>);

/// Очередь персистенции боксов — общая с `GameState`.
#[derive(Resource, Clone)]
pub struct BoxPersistQueue(pub Arc<Mutex<Vec<BoxPersist>>>);

#[derive(Resource, Default)]
pub struct BroadcastQueue(pub Vec<BroadcastEffect>);

pub enum BroadcastEffect {
    CellUpdate(WorldPos),
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
    SetAggression {
        pid: PlayerId,
        tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
        enabled: bool,
    },
    SetHandMode {
        tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
        enabled: bool,
    },
    FillGun {
        pid: PlayerId,
        tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
        x: i32,
        y: i32,
    },
    SetProgrammatorStatus {
        tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
        running: bool,
    },
}

#[derive(Resource, Default)]
pub struct PendingCellConversions(pub Vec<PendingConversion>);

/// Координаты зданий, которым нужен HB O re-broadcast после обнуления charge (C# `ResendPack`).
#[derive(Resource, Default)]
pub struct PackResendQueue(pub Vec<(i32, i32)>);

pub struct PendingConversion {
    pub pos: WorldPos,
    pub target_cell: crate::world::CellType,
    pub required_cell: crate::world::CellType,
    pub durability: f32,
    pub ticks_left: u32,
    /// Игрок, поставивший блок — для начисления 2-го build-exp при конвертации
    /// (1:1 C# `Player.Build("V")`: `AddExp` на frame И внутри `StupidAction`-колбэка).
    pub owner_pid: PlayerId,
}

// ─── Incoming Actions Queue ──────────────────────────────────────────────────

/// Входящее игровое действие: (игрок, канал ответа, TY-пакет).
pub type IncomingAction = (
    PlayerId,
    tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    crate::protocol::packets::TyPacket,
);
/// Запись персистенции бокса: (координата, `Some`=upsert | `None`=delete).
type BoxPersist = (WorldPos, Option<[i64; 6]>);
/// Пакет в HB-overlay здания: поля именованы для читаемости (IR-3).
#[derive(Clone, Copy, Debug)]
pub struct PackOverlay {
    /// Код типа здания (`PackType::code()`).
    pub code: u8,
    /// X-координата здания (сетевой u16, `rem_euclid(65536)`).
    pub x: u16,
    /// Y-координата здания (сетевой u16).
    pub y: u16,
    /// Клановый ID (`clan_id.clamp(0,255) as u8`).
    pub clan: u8,
    /// 1 если `charge > 0`, 0 иначе ("charged" flag для оверлея).
    pub charged: u8,
}

pub struct BuildingInsertSpec<'a> {
    pub type_code: &'a str,
    pub pack_type: PackType,
    pub x: i32,
    pub y: i32,
    pub owner_id: PlayerId,
    pub clan_id: i32,
    pub extra: &'a BuildingExtra,
}

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

// ─── Lifecycle Queue ─────────────────────────────────────────────────────────

/// Команда жизненного цикла сессии — исполняется в game-tick (НЕ через TY).
/// Переносит ecs-доступ login/disconnect из conn-тасков в единый tick-таск,
/// чтобы `ecs`-`RwLock` не контендился между conn-тасками и тиком.
/// `token` идентифицирует конкретный сеанс (guard от reconnect-гонки).
pub enum LifeCmd {
    Connect {
        row: Box<crate::db::players::PlayerRow>,
        tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
        token: u64,
    },
    Disconnect {
        pid: PlayerId,
        token: u64,
    },
}

pub struct GameSchedule {
    pub name: String,
    pub schedule: RwLock<Schedule>,
    pub interval_ms: std::sync::atomic::AtomicU64,
    pub last_run: Mutex<Instant>,
}

// ─── GameState ───────────────────────────────────────────────────────────────

pub struct GameState {
    pub world: Arc<World>,
    pub db: Arc<Database>,
    pub config: Config,
    pub active_players: DashMap<PlayerId, ActivePlayer>,
    pub player_entities: DashMap<PlayerId, Entity>,
    pub chunk_players: DashMap<ChunkPos, Vec<PlayerId>>,
    pub building_index: DashMap<WorldPos, Entity>,
    pub botspot_index: DashMap<PlayerId, Entity>,
    pub chunk_botspots: DashMap<ChunkPos, Vec<Entity>>,
    pub chunk_buildings: DashMap<ChunkPos, Vec<Entity>>,
    pub chat_channels: RwLock<Vec<chat::ChatChannel>>,
    /// Активные игровые ивенты (множители опыта, дропа и т.д.).
    /// Хранится в `GameState` (не в ECS), чтобы HTTP-API мог менять их
    /// без конкуренции с `ecs.write()` из сессий.
    pub active_events: RwLock<ActiveEvents>,
    pub ecs: RwLock<EcsWorld>,
    pub schedules: Vec<GameSchedule>,
    pub auth_failures: DashMap<std::net::IpAddr, (u32, Instant)>,
    pub incoming_actions: IncomingActionsQueue,
    /// Очередь команд жизненного цикла (Connect/Disconnect). Дренится в
    /// game-tick ДО `incoming_actions` (entity спавнится раньше своих TY).
    pub life_queue: Mutex<Vec<LifeCmd>>,
    /// Монотонный счётчик токенов сеанса (см. `LifeCmd`/`ActivePlayer`).
    session_token_seq: std::sync::atomic::AtomicU64,
    pub player_tx: DashMap<PlayerId, tokio::sync::mpsc::UnboundedSender<Vec<u8>>>,
    /// Боксы (ячейка 90) в памяти — авторитетно. Read/изменение без `SQLite`
    /// (был фриз: sync `SQLite` по боксам под `ecs.write()` в physics-системе
    /// каждые 10ms — `combat.rs` C-1). Персистенция отложена в `box_persist_q`.
    pub box_index: Arc<DashMap<WorldPos, [i64; 6]>>,
    /// Очередь персистенции боксов: `(coord, Some(crystals)=upsert | None=delete)`.
    /// Arc — общий с ECS-ресурсами, чтобы не расходились.
    pub box_persist_q: Arc<Mutex<Vec<BoxPersist>>>,
    /// Динамика цен кристаллов (C# `World.cryscostmod`/`summary`), в памяти.
    pub crystal_economy: Mutex<crate::game::market::CrystalEconomy>,
    /// Oneshot-каналы для принудительного кика: `remove` → drop sender → connection-таск
    /// выходит из select!-loop. Также разрешает зомби-соединения при reconnect (новый
    /// insert вытесняет старый sender → старая connection-задача чисто завершается).
    pub kick_channels: DashMap<PlayerId, tokio::sync::oneshot::Sender<()>>,
    /// Активные расходники-спрайты (boom/protector/razryadka) по клетке `(x,y)` →
    /// `(type, off)`. Клиентский `O`-пакет авторитетен для ВСЕГО чанк-`block_pos`
    /// (`RemoveObjectInBlock` чистит блок целиком), поэтому каждый `O` обязан нести
    /// и здания, и все активные расходники блока — иначе один бум стирает здания и
    /// другие бумы. `gather_block_packs` читает этот реестр. В памяти, transient.
    pub consumable_packs: DashMap<(i32, i32), (u8, u8)>,
    /// Счётчик активных фоновых транзакций/записей в базу данных (используется при shutdown).
    pub db_pending_tasks: std::sync::atomic::AtomicUsize,
}

impl GameState {
    pub const CHUNK_VIEW_RADIUS: i32 = 2;

    #[allow(clippy::too_many_lines)]
    pub async fn new(
        world: Arc<World>,
        database: Arc<Database>,
        config: Config,
    ) -> anyhow::Result<Arc<Self>> {
        let mut schedule_hazards = Schedule::default();
        schedule_hazards.add_systems(combat::standing_cell_hazard_system);

        let mut schedule_physics = Schedule::default();
        schedule_physics.add_systems(sand::sand_physics_system);

        let mut schedule_guns = Schedule::default();
        schedule_guns.add_systems(combat::gun_firing_system);

        let mut schedule_programmator = Schedule::default();
        schedule_programmator.add_systems(programmator::programmator_system);

        let mut schedule_alive = Schedule::default();
        schedule_alive.add_systems(alive::alive_physics_system);

        let mut schedule_building_effects = Schedule::default();
        schedule_building_effects.add_systems(building_damage::building_effect_tick_system);

        let mut schedule_hourly_damage = Schedule::default();
        schedule_hourly_damage.add_systems(building_damage::building_hourly_damage_system);

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
            if let Ok(rows) = database
                .get_recent_chat_messages(&ch.tag, chat::CHAT_HISTORY_LIMIT)
                .await
            {
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

        let schedule_intervals = config.gameplay.schedules;
        let schedules = vec![
            GameSchedule {
                name: "hazards".to_string(),
                schedule: RwLock::new(schedule_hazards),
                interval_ms: std::sync::atomic::AtomicU64::new(schedule_intervals.hazards_ms),
                last_run: Mutex::new(Instant::now()),
            },
            GameSchedule {
                name: "physics".to_string(),
                schedule: RwLock::new(schedule_physics),
                interval_ms: std::sync::atomic::AtomicU64::new(schedule_intervals.physics_ms),
                last_run: Mutex::new(Instant::now()),
            },
            GameSchedule {
                name: "guns".to_string(),
                schedule: RwLock::new(schedule_guns),
                interval_ms: std::sync::atomic::AtomicU64::new(schedule_intervals.guns_ms),
                last_run: Mutex::new(Instant::now()),
            },
            GameSchedule {
                name: "programmator".to_string(),
                schedule: RwLock::new(schedule_programmator),
                interval_ms: std::sync::atomic::AtomicU64::new(schedule_intervals.programmator_ms),
                last_run: Mutex::new(Instant::now()),
            },
            GameSchedule {
                name: "alive".to_string(),
                schedule: RwLock::new(schedule_alive),
                interval_ms: std::sync::atomic::AtomicU64::new(schedule_intervals.alive_ms),
                last_run: Mutex::new(Instant::now()),
            },
            GameSchedule {
                name: "building_effects".to_string(),
                schedule: RwLock::new(schedule_building_effects),
                interval_ms: std::sync::atomic::AtomicU64::new(
                    schedule_intervals.building_effects_ms,
                ),
                last_run: Mutex::new(Instant::now()),
            },
            GameSchedule {
                name: "hourly_damage".to_string(),
                schedule: RwLock::new(schedule_hourly_damage),
                interval_ms: std::sync::atomic::AtomicU64::new(schedule_intervals.hourly_damage_ms),
                last_run: Mutex::new(Instant::now()),
            },
        ];

        let state = Arc::new(Self {
            world,
            db: database,
            config,
            active_players: DashMap::new(),
            player_entities: DashMap::new(),
            chunk_players: DashMap::new(),
            building_index: DashMap::new(),
            botspot_index: DashMap::new(),
            chunk_botspots: DashMap::new(),
            chunk_buildings: DashMap::new(),
            chat_channels: RwLock::new(default_channels),
            active_events: RwLock::new(ActiveEvents::default()),
            ecs: RwLock::new(EcsWorld::new()),
            schedules,
            auth_failures: DashMap::new(),
            incoming_actions: IncomingActionsQueue::new(),
            life_queue: Mutex::new(Vec::new()),
            session_token_seq: std::sync::atomic::AtomicU64::new(1),
            player_tx: DashMap::new(),
            box_index: Arc::new(DashMap::new()),
            box_persist_q: Arc::new(Mutex::new(Vec::new())),
            crystal_economy: Mutex::new(crate::game::market::CrystalEconomy::default()),
            kick_channels: DashMap::new(),
            consumable_packs: DashMap::new(),
            db_pending_tasks: std::sync::atomic::AtomicUsize::new(0),
        });

        // Боксы из БД → in-memory индекс (один раз; на hot-path SQLite по
        // боксам больше не дёргаем).
        match state.db.load_all_boxes().await {
            Ok(rows) => {
                for (bx, by, crystals) in rows {
                    state.box_index.insert((bx, by).into(), crystals);
                }
                tracing::info!(
                    "Loaded {} boxes into in-memory index",
                    state.box_index.len()
                );
            }
            Err(e) => tracing::error!(error = ?e, "Failed to load boxes into index"),
        }

        match state.db.load_all_events().await {
            Ok(rows) => {
                let mut events = state.active_events.write();
                for r in rows {
                    #[derive(serde::Deserialize)]
                    struct Config {
                        xp_mult: f64,
                        drop_mult: f64,
                    }
                    let cfg: Config = serde_json::from_str(&r.config_json).unwrap_or(Config {
                        xp_mult: 1.0,
                        drop_mult: 1.0,
                    });
                    events.list.push(ActiveEvent {
                        id: r.id,
                        title: r.title,
                        starts_at: r.starts_at,
                        ends_at: r.ends_at,
                        xp_mult: cfg.xp_mult,
                        drop_mult: cfg.drop_mult,
                    });
                }
                tracing::info!(
                    count = events.list.len(),
                    "Loaded active events from database"
                );
            }
            Err(e) => tracing::error!(error = ?e, "Failed to load active events from database"),
        }

        {
            let mut ecs = state.ecs.write();
            ecs.insert_resource(WorldResource(state.world.clone()));
            ecs.insert_resource(ProgrammatorConfigResource(
                state.config.gameplay.programmator,
            ));
            ecs.insert_resource(BoxIndexResource(state.box_index.clone()));
            ecs.insert_resource(BoxPersistQueue(state.box_persist_q.clone()));
            ecs.insert_resource(combat::DeathQueue::default());
            ecs.insert_resource(BroadcastQueue::default());
            ecs.insert_resource(ProgrammatorQueue::default());
            ecs.insert_resource(alive::AliveTickTimer::default());
            ecs.insert_resource(sand::SandTickTimer::default());
            ecs.insert_resource(combat::GunTickTimer::default());
            ecs.insert_resource(building_damage::BuildingDamageTimer::default());
            ecs.insert_resource(PendingCellConversions::default());
            ecs.insert_resource(PackResendQueue::default());
        }

        Self::load_buildings_into_ecs(&state).await?;
        Ok(state)
    }

    /// Загрузить все здания из БД в ECS (вынесено из `new` — лимит строк).
    async fn load_buildings_into_ecs(state: &Arc<Self>) -> anyhow::Result<()> {
        let all_rows = state
            .db
            .load_all_buildings()
            .await
            .context("load buildings from database")?;
        let count = all_rows.len();
        let mut ecs = state.ecs.write();
        let mut spot_count = 0u32;
        for row in all_rows {
            let (entity, pack_type) = buildings::spawn_building_from_row(&mut ecs, &row)?;
            state.register_building_entity(row.x, row.y, entity);

            if pack_type == buildings::PackType::Spot {
                let botspot_entity = ecs
                    .spawn((
                        botspot::BotSpotMarker,
                        botspot::BotSpotData {
                            bot_id: -row.owner_id,
                            owner_id: row.owner_id.into(),
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
                state
                    .botspot_index
                    .insert(row.owner_id.into(), botspot_entity);
                let (cx, cy) = World::chunk_pos(row.x, row.y);
                state
                    .chunk_botspots
                    .entry((cx, cy).into())
                    .or_default()
                    .push(botspot_entity);
                spot_count += 1;
            }
        }
        drop(ecs);
        tracing::info!(
            "Loaded {count} buildings into ECS from DB ({spot_count} Spot BotSpots spawned)"
        );
        Ok(())
    }

    pub fn get_player_entity(&self, pid: PlayerId) -> Option<Entity> {
        self.player_entities.get(&pid).map(|p| *p)
    }

    /// Выдать новый токен сеанса (монотонный, уникальный на процесс).
    pub fn next_session_token(&self) -> u64 {
        self.session_token_seq
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// Поставить команду жизненного цикла в очередь (из conn-таска).
    pub fn enqueue_life(&self, cmd: LifeCmd) {
        self.life_queue.lock().push(cmd);
    }

    /// Забрать все команды жизненного цикла (в game-tick).
    pub fn drain_life(&self) -> Vec<LifeCmd> {
        std::mem::take(&mut *self.life_queue.lock())
    }

    pub fn query_player<F, R>(&self, pid: PlayerId, f: F) -> Option<R>
    where
        F: FnOnce(&EcsWorld, Entity) -> R,
    {
        let entity = self.get_player_entity(pid)?;
        let ecs = self.ecs.read();
        if !ecs.entities().contains(entity) {
            tracing::warn!(player_id = %pid, ?entity, "Player entity exists in active_players but is missing from ECS world!");
            drop(ecs);
            return None;
        }
        let res = f(&ecs, entity);
        drop(ecs);
        Some(res)
    }

    /// Как [`query_player`](Self::query_player), но для замыканий, возвращающих
    /// `Option<T>`: флэтит `Option<Option<T>>` → `Option<T>`. Убирает `.flatten()`
    /// на стороне вызова (offline-игрок и `None` из замыкания → один `None`).
    pub fn query_player_opt<F, T>(&self, pid: PlayerId, f: F) -> Option<T>
    where
        F: FnOnce(&EcsWorld, Entity) -> Option<T>,
    {
        self.query_player(pid, f).flatten()
    }

    /// Query a player and log a debug message if they are not found (expected to be online).
    pub fn query_player_expected<F, T>(&self, pid: PlayerId, context: &str, f: F) -> Option<T>
    where
        F: FnOnce(&EcsWorld, Entity) -> Option<T>,
    {
        let Some(entity) = self.get_player_entity(pid) else {
            tracing::debug!(player_id = %pid, context = context, "Expected player entity not found in active_players (player offline)");
            return None;
        };
        let ecs = self.ecs.read();
        if !ecs.entities().contains(entity) {
            tracing::warn!(player_id = %pid, ?entity, context = context, "Player exists in active_players but entity is missing from ECS world!");
            drop(ecs);
            return None;
        }
        let res = f(&ecs, entity);
        drop(ecs);
        if res.is_none() {
            tracing::debug!(player_id = %pid, ?entity, context = context, "Player query returned None in expected context");
        }
        res
    }

    pub fn modify_player<F, R>(&self, pid: PlayerId, f: F) -> Option<R>
    where
        F: FnOnce(&mut EcsWorld, Entity) -> R,
    {
        let entity = self.get_player_entity(pid)?;
        let mut ecs = self.ecs.write();
        if !ecs.entities().contains(entity) {
            tracing::warn!(player_id = %pid, ?entity, "Player entity exists in active_players but is missing from ECS world during modify!");
            drop(ecs);
            return None;
        }
        let res = f(&mut ecs, entity);
        drop(ecs);
        Some(res)
    }

    pub fn set_schedule_interval(&self, name: &str, interval_ms: u64) -> bool {
        self.schedules
            .iter()
            .find(|s| s.name == name)
            .is_some_and(|gs| {
                gs.interval_ms
                    .store(interval_ms, std::sync::atomic::Ordering::Relaxed);
                true
            })
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
        let entity = self.building_entity_at(x, y)?;
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

    /// Найти origin building entity по typed world position boundary.
    pub fn building_entity_at(&self, x: i32, y: i32) -> Option<Entity> {
        self.building_index
            .get(&((x, y).into()))
            .map(|entry| *entry.value())
    }

    pub fn has_building_origin(&self, x: i32, y: i32) -> bool {
        self.building_index.contains_key(&((x, y).into()))
    }

    pub fn query_building_opt<R>(
        &self,
        x: i32,
        y: i32,
        f: impl FnOnce(&EcsWorld, Entity) -> Option<R>,
    ) -> Option<R> {
        let entity = self.building_entity_at(x, y)?;
        let ecs = self.ecs.read();
        f(&ecs, entity)
    }

    pub fn building_entities_snapshot(&self) -> Vec<Entity> {
        self.building_index
            .iter()
            .map(|entry| *entry.value())
            .collect()
    }

    pub(crate) fn find_pack_covering_with(
        ecs: &EcsWorld,
        chunk_buildings: &DashMap<ChunkPos, Vec<Entity>>,
        x: i32,
        y: i32,
    ) -> Option<(i32, i32)> {
        let (cx, cy) = World::chunk_pos(x, y);
        let check_range_x = (cx.cast_signed() - 1)..=(cx.cast_signed() + 1);
        let check_range_y = (cy.cast_signed() - 1)..=(cy.cast_signed() + 1);

        for (ncx, ncy) in check_range_x.flat_map(|x| check_range_y.clone().map(move |y| (x, y))) {
            if ncx < 0 || ncy < 0 {
                continue;
            }
            let key: ChunkPos = (ncx.cast_unsigned(), ncy.cast_unsigned()).into();
            if let Some(entities) = chunk_buildings.get(&key) {
                for &entity in entities.value() {
                    let Some(pos) = ecs.get::<GridPosition>(entity) else {
                        continue;
                    };
                    let Some(meta) = ecs.get::<BuildingMetadata>(entity) else {
                        continue;
                    };
                    for (dx, dy, _) in meta
                        .pack_type
                        .building_cells()
                        .expect("loaded building pack type must have config")
                    {
                        if pos.x + dx == x && pos.y + dy == y {
                            return Some((pos.x, pos.y));
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
        block_pos_from_cell(x, y, self.world.chunks_w().cast_signed())
    }

    /// C# `World.AccessGun` → `(access, anygun)`. `access`: нет вражеской ЗАРЯЖЕННОЙ
    /// пушки в радиусе 20. `anygun`: есть ЛЮБАЯ пушка в радиусе (для Gate-item).
    pub(crate) fn access_gun_with(
        ecs: &EcsWorld,
        chunk_buildings: &DashMap<ChunkPos, Vec<Entity>>,
        x: i32,
        y: i32,
        player_clan_id: i32,
    ) -> (bool, bool) {
        let mut ret = true;
        let mut anygun = false;
        let (cx, cy) = World::chunk_pos(x, y);
        for ncy in (cy.cast_signed() - 1)..=(cy.cast_signed() + 1) {
            for ncx in (cx.cast_signed() - 1)..=(cx.cast_signed() + 1) {
                if ncx < 0 || ncy < 0 {
                    continue;
                }
                if let Some(entities) =
                    chunk_buildings.get(&(ncx.cast_unsigned(), ncy.cast_unsigned()).into())
                {
                    for &entity in entities.value() {
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
                        if meta.pack_type != PackType::Gun {
                            continue;
                        }
                        for (dx, dy, _) in meta
                            .pack_type
                            .building_cells()
                            .expect("loaded building pack type must have config")
                        {
                            let bx = pos.x + dx;
                            let by = pos.y + dy;
                            let ddx = f64::from(bx - x);
                            let ddy = f64::from(by - y);
                            if ddx.hypot(ddy) <= 20.0 {
                                // C# anygun ставится для любой пушки (до проверки charge).
                                anygun = true;
                                if stats.charge > 0 {
                                    ret = ret && own.clan_id == player_clan_id;
                                }
                            }
                        }
                    }
                }
            }
        }
        (ret, anygun)
    }

    pub fn access_gun(&self, x: i32, y: i32, player_clan_id: i32) -> bool {
        self.access_gun_full(x, y, player_clan_id).0
    }

    /// C# `World.AccessGun` целиком: `(access, anygun)`.
    pub fn access_gun_full(&self, x: i32, y: i32, player_clan_id: i32) -> (bool, bool) {
        let ecs = self.ecs.read();
        Self::access_gun_with(&ecs, &self.chunk_buildings, x, y, player_clan_id)
    }

    /// Паки (HB-оверлей) ровно в ОДНОМ чанке `(cx, cy)`. В отличие от
    /// `get_packs_in_chunk_area` (5×5 область), не захватывает соседние чанки —
    /// нужно при per-чанковой отправке/очистке HB (`chunks.rs`), иначе очистка
    /// ушедшего чанка затирала бы оверлеи паков в ещё видимых соседних чанках
    /// (баг «паки мерцают/пропадают на границе чанка»).
    pub fn get_packs_in_single_chunk(&self, cx: u32, cy: u32) -> Vec<PackOverlay> {
        let mut results = Vec::new();
        let ecs = self.ecs.read();
        if let Some(entities) = self.chunk_buildings.get(&(cx, cy).into()) {
            for &entity in entities.value() {
                let pos = ecs.get::<GridPosition>(entity);
                let meta = ecs.get::<BuildingMetadata>(entity);
                let own = ecs.get::<BuildingOwnership>(entity);
                let stats = ecs.get::<BuildingStats>(entity);
                if let (Some(pos), Some(meta), Some(own), Some(stats)) = (pos, meta, own, stats)
                    && meta.pack_type.included_in_hb_overlay()
                {
                    results.push(PackOverlay {
                        code: meta.pack_type.code(),
                        x: u16::try_from(pos.x.rem_euclid(65536)).unwrap_or(0),
                        y: u16::try_from(pos.y.rem_euclid(65536)).unwrap_or(0),
                        clan: u8::try_from(own.clan_id.clamp(0, 255)).unwrap_or(0),
                        charged: u8::from(stats.charge > 0),
                    });
                }
            }
        }
        drop(ecs);
        results
    }

    pub fn get_packs_in_chunk_area(&self, cx: u32, cy: u32) -> Vec<PackOverlay> {
        let mut results = Vec::new();
        let ecs = self.ecs.read();
        for (ucx, ucy) in self.visible_chunks_around(cx, cy) {
            if let Some(entities) = self.chunk_buildings.get(&(ucx, ucy).into()) {
                for &entity in entities.value() {
                    let pos = ecs.get::<GridPosition>(entity);
                    let meta = ecs.get::<BuildingMetadata>(entity);
                    let own = ecs.get::<BuildingOwnership>(entity);
                    let stats = ecs.get::<BuildingStats>(entity);
                    if let (Some(pos), Some(meta), Some(own), Some(stats)) = (pos, meta, own, stats)
                        && meta.pack_type.included_in_hb_overlay()
                    {
                        results.push(PackOverlay {
                            code: meta.pack_type.code(),
                            x: u16::try_from(pos.x.rem_euclid(65536)).unwrap_or(0),
                            y: u16::try_from(pos.y.rem_euclid(65536)).unwrap_or(0),
                            clan: u8::try_from(own.clan_id.clamp(0, 255)).unwrap_or(0),
                            charged: u8::from(stats.charge > 0),
                        });
                    }
                }
            }
        }
        drop(ecs);
        results
    }

    pub fn visible_chunks_iter(&self, cx: u32, cy: u32) -> impl Iterator<Item = (u32, u32)> + '_ {
        let (w, h) = (self.world.chunks_w(), self.world.chunks_h());
        (-Self::CHUNK_VIEW_RADIUS..=Self::CHUNK_VIEW_RADIUS).flat_map(move |dy| {
            (-Self::CHUNK_VIEW_RADIUS..=Self::CHUNK_VIEW_RADIUS).filter_map(move |dx| {
                let ncx = cx.cast_signed() + dx;
                let ncy = cy.cast_signed() + dy;
                (ncx >= 0 && ncx < w.cast_signed() && ncy >= 0 && ncy < h.cast_signed())
                    .then_some((ncx.cast_unsigned(), ncy.cast_unsigned()))
            })
        })
    }

    /// Собирает видимые чанки в `Vec`. Используй `visible_chunks_iter` там, где
    /// можно обойтись без аллокации (broadcast-путь). `Vec`-версия остаётся для
    /// мест, где список нужен как owned (например, `bots_render` due-list).
    pub fn visible_chunks_around(&self, cx: u32, cy: u32) -> Vec<(u32, u32)> {
        self.visible_chunks_iter(cx, cy).collect()
    }

    pub fn send_to_player(&self, pid: PlayerId, data: Vec<u8>) {
        if let Some(tx) = self.player_tx.get(&pid) {
            let _ = tx.send(data);
        }
    }

    pub fn broadcast_cell_update(&self, x: i32, y: i32) {
        use crate::protocol::packets::hb_cell;
        let cell = self.world.get_cell_typed(x, y);
        let sub = hb_cell(
            u16::try_from(x.rem_euclid(65536)).unwrap_or(0),
            u16::try_from(y.rem_euclid(65536)).unwrap_or(0),
            cell.0,
        );
        self.broadcast_hb_at(x, y, &[sub], None);
    }

    /// Зарегистрировать building entity в обоих runtime-индексах.
    /// Callers не должны вручную синхронизировать `building_index` и
    /// `chunk_buildings`: это единый boundary для position→entity кэшей.
    pub fn register_building_entity(&self, x: i32, y: i32, entity: Entity) {
        self.building_index.insert((x, y).into(), entity);
        let (cx, cy) = World::chunk_pos(x, y);
        self.chunk_buildings
            .entry((cx, cy).into())
            .or_default()
            .push(entity);
    }

    /// Удалить building entity из обоих runtime-индексов.
    pub fn remove_building_entity(&self, x: i32, y: i32) -> Option<Entity> {
        let (_, entity) = self.building_index.remove(&((x, y).into()))?;
        let (cx, cy) = World::chunk_pos(x, y);
        if let Some(mut entities) = self.chunk_buildings.get_mut(&(cx, cy).into()) {
            entities.retain(|&ent| ent != entity);
        }
        Some(entity)
    }

    /// Перенести building entity между координатами в runtime-индексах.
    pub fn move_building_entity(&self, old_x: i32, old_y: i32, new_x: i32, new_y: i32) {
        if let Some(entity) = self.remove_building_entity(old_x, old_y) {
            self.register_building_entity(new_x, new_y, entity);
        }
    }

    /// Runtime commit нового здания: ECS entity + runtime индексы + mmap footprint.
    /// DB insert остаётся перед этим шагом, потому что call-sites по-разному
    /// обрабатывают ошибку БД и возврат ресурсов игроку.
    pub fn spawn_building_runtime(&self, spec: &BuildingSpawnSpec<'_>) -> Entity {
        let entity = {
            let mut ecs = self.ecs.write();
            buildings::spawn_building_from_extra(&mut ecs, spec)
        };
        self.register_building_entity(spec.x, spec.y, entity);
        self.place_building_footprint(spec.x, spec.y, spec.pack_type);
        entity
    }

    /// Persist + runtime commit нового здания.
    /// Ошибку БД возвращает caller'у: возврат денег/предметов остаётся на
    /// границе конкретного действия.
    pub async fn insert_building_runtime(
        &self,
        spec: &BuildingInsertSpec<'_>,
    ) -> anyhow::Result<(i32, Entity)> {
        let id = self
            .db
            .insert_building(
                spec.type_code,
                spec.x,
                spec.y,
                spec.owner_id.into(),
                spec.clan_id,
                spec.extra,
            )
            .await?;
        let spawn_spec = BuildingSpawnSpec {
            id,
            pack_type: spec.pack_type,
            x: spec.x,
            y: spec.y,
            owner_id: spec.owner_id,
            clan_id: spec.clan_id,
            extra: spec.extra,
        };
        let entity = self.spawn_building_runtime(&spawn_spec);
        Ok((id, entity))
    }

    /// Runtime removal здания: runtime индексы + ECS despawn + mmap footprint.
    /// DB delete остаётся перед этим шагом, потому что call-sites по-разному
    /// обрабатывают ошибку БД, дропы и async-контекст.
    pub fn remove_building_runtime(&self, view: &PackView) -> Option<Entity> {
        let entity = self.remove_building_entity(view.x, view.y);
        if entity.is_some() && view.pack_type == PackType::Spot {
            self.remove_botspot_runtime(view.owner_id);
        }
        if let Some(entity) = entity {
            self.ecs.write().despawn(entity);
        }
        self.clear_building_footprint(view);
        entity
    }

    /// Persisted removal здания: DB delete + runtime cleanup.
    /// Дропы/возврат предметов остаются у caller'а, потому что зависят от
    /// конкретной причины сноса.
    pub async fn delete_building_runtime(&self, view: &PackView) -> anyhow::Result<Option<Entity>> {
        self.db.delete_building(view.id).await?;
        Ok(self.remove_building_runtime(view))
    }

    /// Runtime removal `BotSpot`, связанного со Spot-зданием.
    pub fn remove_botspot_runtime(&self, owner_id: PlayerId) -> Option<Entity> {
        let (_, entity) = self.botspot_index.remove(&owner_id)?;
        self.chunk_botspots
            .iter_mut()
            .for_each(|mut e| e.value_mut().retain(|&ent| ent != entity));
        self.ecs.write().despawn(entity);
        Some(entity)
    }

    /// Поставить mmap-футпринт здания и разослать HB cell updates.
    pub fn place_building_footprint(&self, bx: i32, by: i32, pack_type: PackType) {
        for (cdx, cdy, cell) in pack_type
            .building_cells()
            .expect("loaded building pack type must have config")
        {
            let (x, y) = (bx + cdx, by + cdy);
            self.world
                .set_cell_typed(x, y, crate::world::CellType(cell));
            self.broadcast_cell_update(x, y);
        }
    }

    /// Очистить mmap-футпринт здания и разослать HB cell updates.
    pub fn clear_building_footprint(&self, view: &PackView) {
        for (cdx, cdy, _) in view
            .pack_type
            .building_cells()
            .expect("loaded building pack type must have config")
        {
            let (x, y) = (view.x + cdx, view.y + cdy);
            self.world.set_cell_typed(
                x,
                y,
                crate::world::CellType(crate::world::cells::cell_type::EMPTY),
            );
            self.broadcast_cell_update(x, y);
        }
    }

    // ─── Боксы: in-memory, без SQLite на hot-path (фикс фриза C-1/C-2/H-1) ──

    /// Атомарно забрать бокс (pickup): удалить из индекса, вернуть кристаллы,
    /// поставить delete в очередь персистенции. Без `SQLite` (lock-free `DashMap`).
    pub fn box_take(&self, x: i32, y: i32) -> Option<[i64; 6]> {
        let removed = self.box_index.remove(&(x, y).into()).map(|(_, v)| v);
        if removed.is_some() {
            self.box_persist_q.lock().push(((x, y).into(), None));
        }
        removed
    }

    /// Положить/обновить бокс (death drop): индекс + upsert в очередь.
    pub fn box_put(&self, x: i32, y: i32, crystals: [i64; 6]) {
        self.box_index.insert((x, y).into(), crystals);
        self.box_persist_q
            .lock()
            .push(((x, y).into(), Some(crystals)));
    }

    /// Положить box-клетку и её содержимое одной доменной операцией.
    /// Это первый слой boundary для `WorldCell { type, durability, pack/box }`:
    /// callers не должны отдельно помнить про mmap-клетку и `box_index`.
    pub fn put_box_cell(&self, x: i32, y: i32, crystals: [i64; 6]) {
        self.world.set_cell_typed(
            x,
            y,
            crate::world::CellType(crate::world::cells::cell_type::BOX),
        );
        self.box_put(x, y, crystals);
    }

    /// Убрать box-клетку и связанный индекс. Если индекс уже потерян, orphan BOX
    /// всё равно очищается из мира, как прежний `damage_cell`-path при копании.
    pub fn remove_box_cell(&self, x: i32, y: i32) -> Option<[i64; 6]> {
        let crystals = self.box_take(x, y);
        self.world.damage_cell(x, y, 1.0);
        crystals
    }

    /// Слить очередь персистенции боксов. На hot-path `BoxPersistQueue` дренится
    /// внутри `ecs.write()` в lifecycle.rs; этот метод — для финального drain при
    /// shutdown (`main::shutdown_flush`), чтобы не потерять последние upsert/delete.
    pub fn drain_box_persist(&self) -> Vec<BoxPersist> {
        let mut q = self.box_persist_q.lock();
        std::mem::take(&mut *q)
    }

    pub fn broadcast_to_nearby(&self, cx: u32, cy: u32, data: &[u8], exclude_id: Option<PlayerId>) {
        // PB-2: итерируем напрямую под guard'ом DashMap — не клонируем Vec<PlayerId>.
        // send_to_player берёт player_tx (другой DashMap-шард) → дедлок невозможен.
        for (ncx, ncy) in self.visible_chunks_iter(cx, cy) {
            if let Some(players) = self.chunk_players.get(&(ncx, ncy).into()) {
                for &pid in players.value() {
                    if Some(pid) == exclude_id {
                        continue;
                    }
                    self.send_to_player(pid, data.to_vec());
                }
            }
        }
    }

    /// Бродкаст HB-подпакетов игрокам рядом с клеткой `(x, y)`: считает чанк,
    /// собирает bundle и B-фрейм, шлёт через `broadcast_to_nearby`. Тонкая обёртка
    /// над повторяющимся паттерном — вывод байт-в-байт идентичен ручной форме.
    pub fn broadcast_hb_at(&self, x: i32, y: i32, subs: &[Vec<u8>], exclude_id: Option<PlayerId>) {
        use crate::net::session::wire::encode_hb_bundle;
        use crate::protocol::packets::hb_bundle;
        let (cx, cy) = World::chunk_pos(x, y);
        self.broadcast_to_nearby(cx, cy, &encode_hb_bundle(&hb_bundle(subs).1), exclude_id);
    }

    pub fn broadcast_to_nearby_specific_chunk(
        &self,
        cx: u32,
        cy: u32,
        data: &[u8],
        exclude_id: Option<PlayerId>,
    ) {
        // PB-2: то же — без клонирования Vec под guard'ом.
        if let Some(players) = self.chunk_players.get(&(cx, cy).into()) {
            for &pid in players.value() {
                if Some(pid) == exclude_id {
                    continue;
                }
                self.send_to_player(pid, data.to_vec());
            }
        }
    }

    pub fn generate_hash() -> String {
        const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        generate_random_string(12, CHARSET)
    }
    pub fn generate_session_id() -> String {
        // IR-8: восстановлены пропущенные символы q, v, w (были опечатки в оригинале).
        // 5 символов из 36 → 36^5 ≈ 60M комбинаций (было 33^5 ≈ 39M).
        const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
        generate_random_string(5, CHARSET)
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
    state.broadcast_cell_update(x, y);
}

fn generate_random_string(len: usize, charset: &[u8]) -> String {
    use rand::Rng as _;
    use rand::SeedableRng as _;
    // PB-6: переиспользуем thread-local SmallRng — инициализация один раз на поток,
    // а не при каждом вызове. SmallRng быстр и достаточен для non-crypto токенов.
    thread_local! {
        static RNG: std::cell::RefCell<rand::rngs::SmallRng> =
            std::cell::RefCell::new(rand::rngs::SmallRng::from_os_rng());
    }
    RNG.with(|rng| {
        let mut rng = rng.borrow_mut();
        (0..len)
            .map(|_| charset[rng.random_range(0..charset.len())] as char)
            .collect()
    })
}

/// Чанковый `block_pos` для клетки `(x, y)`. C# `PACKPOS = x + y * World.ChunksW`,
/// но эталон — клиент: `PackRenderer.IsPackOn` ключует `objectsInBlock` как
/// `(x>>5)+(y>>5)*(width>>5)` = `chunk_x + chunk_y*chunks_w`, и `ObjectsGarbageCollector`
/// восстанавливает origin чанка тем же образом. Поэтому `block_pos` ОБЯЗАН быть
/// чанковым, НЕ клеточным (см. 77033c5: клеточные координаты → клиентский GC
/// считал огромное расстояние и сносил все паки каждые 10 сек).
fn block_pos_from_cell(x: i32, y: i32, chunks_w: i32) -> Option<i32> {
    let cx = x.div_euclid(32);
    let cy = y.div_euclid(32);
    cy.checked_mul(chunks_w)?.checked_add(cx)
}

#[cfg(test)]
mod pack_block_pos_tests {
    use super::block_pos_from_cell;

    /// Регрессия 77033c5: `block_pos` должен быть чанковым (`chunk_x + chunk_y*chunks_w`),
    /// совпадая с клиентским ключом `objectsInBlock`. Раньше считался клеточным.
    #[test]
    fn block_pos_is_chunk_based_not_cell_based() {
        let w = 260; // World.ChunksW
        // Клетка (33, 65) лежит в чанке (1, 2) → 1 + 2*260 = 521.
        assert_eq!(block_pos_from_cell(33, 65, w), Some(521));
        // Все клетки одного чанка делят block_pos (origin и дальний угол).
        assert_eq!(
            block_pos_from_cell(64, 64, w),
            block_pos_from_cell(95, 95, w)
        );
        // Соседний чанк по X → +1, по Y → +chunks_w.
        assert_eq!(block_pos_from_cell(0, 0, w), Some(0));
        assert_eq!(block_pos_from_cell(32, 0, w), Some(1));
        assert_eq!(block_pos_from_cell(0, 32, w), Some(w));
    }
}
