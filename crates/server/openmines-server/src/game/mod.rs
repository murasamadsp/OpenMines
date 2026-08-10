//! Главный модуль игры: состояние мира, игроки, здания и ECS-системы.
//! Архитектура стремится к 1:1 соответствию логике C# сервера (World.cs, MServer.cs).

pub mod actors;
pub mod economy;
pub mod logic;
pub mod mechanics;
pub mod structures;
pub mod world;

pub use actors::{alive, botspot, player, programmator};
pub use economy::market;
pub use logic::contracts::{
    AdminMoneyAllRequest, AdminMoneyAllResult, AdminRoleRequest, AdminRoleResult,
    AdminSkillRequest, AdminSkillResult, AuctionBetRequest, AuctionBetResult, AuctionGridRequest,
    AuctionGridResult, AuctionItemOrdersRequest, AuctionItemOrdersResult,
    AuctionOrderCreateRequest, AuctionOrderCreateResult, AuctionOrderRequest, AuctionOrderResult,
    BuildingDeleteCause, BuildingDeleteOperationId, BuildingDeleteOrigin, BuildingDeleteRequest,
    BuildingDeleteResult, BuildingIdentity, BuildingMenuRequest, BuildingMenuResult,
    ChatAppendRequest, ChatColorCycleRequest, ChatColorCycleResult, ChatMenuRequest,
    ChatMenuResult, ChatPrivateRequest, ChatPrivateResult, ChatResyncRequest, ChatResyncResult,
    ClanAction, ClanCommandRequest, ClanCommandResult, ClanMemberEntry, ClanMenuAction,
    ClanMenuListEntry, ClanMenuRequest, ClanMenuResult, CommandEffects, CommandIngressClass,
    CommandSeq, GameCommand, GameEvent, GuiCommand, GuiView, PersistenceCompletion, PlayerCommand,
    PlayerInitView, ProgramCopyRequest, ProgramCopyResult, ProgramCreateRequest,
    ProgramCreateResult, ProgramMenuRequest, ProgramMenuResult, ProgramOpenRequest,
    ProgramOpenResult, ProgramRenameRequest, ProgramRenameResult, ProgramSaveRequest,
    ProgramSaveResult, QueuedGameCommand, RemovePack, SaveCommand, SaveKind, SessionId, SimTick,
    SlashCommand, SlashPackCommand, SpotGuiView, StorageGuiView, TeleportGuiView, WhoisRequest,
    WhoisResult,
};
pub use logic::{crafting, skills};
pub use mechanics::{building_damage, chat, combat};
pub use structures::buildings;
pub use world::{direction, granular};

use crate::config::Config;
use crate::db::Database;
use crate::game::kernel::guards::ECS_LOCK_PROFILE_THRESHOLD;
use crate::world::{World, WorldProvider};
use anyhow::Context as _;
use bevy_ecs::prelude::{Entity, Schedule, World as EcsWorld};
use dashmap::DashMap;
use parking_lot::{Mutex, RwLock};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

pub use actors::player::{
    ActivePlayer, DirtyPlayers, PlayerFlags, PlayerId, PlayerMetadata, PlayerPosition, PlayerStats,
};
pub use mechanics::events::{ActiveEvent, ActiveEvents, ExpContext};
pub use structures::buildings::{
    BuildingDeletePending, BuildingFlags, BuildingMetadata, BuildingOwnership, BuildingSpawnSpec,
    BuildingStats, DirtyBuildings, GridPosition, PackType, PackView,
};
pub use world::coords::{ChunkPos, WorldPos};

pub mod kernel;
pub use kernel::*;

// ─── GameState ───────────────────────────────────────────────────────────────

pub struct GameState {
    pub world: Arc<World>,
    pub db: Arc<Database>,
    pub config: Config,
    pub(crate) player_registry: PlayerRegistry,
    pub(crate) building_index: BuildingIndex,
    pub chat_channels: RwLock<Vec<chat::ChatChannel>>,
    /// Активные игровые ивенты (множители опыта, дропа и т.д.).
    /// Хранится в `GameState` (не в ECS), чтобы HTTP-API мог менять их
    /// без конкуренции с `ecs.write()` из сессий.
    pub active_events: RwLock<ActiveEvents>,
    pub(crate) ecs: RwLock<EcsWorld>,
    pub schedules: Vec<GameSchedule>,
    pub auth_failures: DashMap<std::net::IpAddr, (u32, Instant)>,
    pub commands_rx: Mutex<Option<CommandReceivers>>,
    pub(crate) command_ingress: CommandIngress,
    simulation_waker: crate::simulation_waker::SimulationWaker,
    due_schedules: DueSchedules,
    bots_render_schedule: Mutex<BotsRenderSchedule>,
    bots_render_slot_seq: std::sync::atomic::AtomicU64,
    pub tokio_handle: tokio::runtime::Handle,
    pub sessions: crate::net::session::hub::SessionHub,
    chat_id_seq: std::sync::atomic::AtomicI64,
    /// Боксы (ячейка 90) в памяти — авторитетно.
    box_index: Arc<DashMap<WorldPos, [i64; 6]>>,
    box_pickup_queue: BoxPickupQueue,
    death_queue: combat::DeathQueue,
    granular_wake_q: GranularWakeQueue,
    alive_work_q: alive::AliveWorkQueue,
    /// Динамика цен кристаллов (C# `World.cryscostmod`/`summary`), в памяти.
    pub crystal_economy: Mutex<crate::game::market::CrystalEconomy>,
    /// Активные расходники-спрайты (boom/protector/razryadka) по клетке `WorldPos` →
    /// `(type, off)`. Клиентский `O`-пакет авторитетен для ВСЕГО чанк-`block_pos`
    /// (`RemoveObjectInBlock` чистит блок целиком), поэтому каждый `O` обязан нести
    /// и здания, и все активные расходники блока — иначе один бум стирает здания и
    /// другие бумы. `gather_block_packs` читает этот реестр. В памяти, transient.
    pub consumable_packs: DashMap<WorldPos, (u8, u8)>,
    /// Счётчик активных фоновых транзакций/записей в базу данных (используется при shutdown).
    pub db_pending_tasks: std::sync::atomic::AtomicUsize,
    /// Per-player GCRA rate limiters (чат, GUI). Создаются лениво при первом пакете,
    /// удаляются при дисконнекте через `remove_rate_limiter`.
    pub rate_limiters: DashMap<PlayerId, crate::net::session::rate_limit::PlayerLimiters>,
    /// Неизменяемый `ReadSnapshot` для веб-API (stats/map), обновляемый симулятором.
    pub web_snapshot: WebSnapshotOwner,
}

impl GameState {
    pub const CHUNK_VIEW_RADIUS: i32 = 2;
    pub const BOTS_RENDER_INTERVAL: Duration = Duration::from_secs(4);
    pub const BOTS_RENDER_OBSERVER_BUDGET: usize = 32;
    pub const BOTS_RENDER_BYTE_BUDGET: usize = 1024 * 1024;
    pub const CRAFTING_DUE_BATCH_BUDGET: usize = 256;

    pub fn persistence_pending_task_count(&self) -> usize {
        self.db_pending_tasks
            .load(std::sync::atomic::Ordering::SeqCst)
    }
    pub const PROGRAMMATOR_DUE_BATCH_BUDGET: usize = 256;
    pub const HAZARD_DUE_BATCH_BUDGET: usize = 256;

    pub fn next_chat_id(&self) -> i64 {
        self.chat_id_seq
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1
    }

    pub fn update_web_snapshot(&self) {
        self.web_snapshot.update(self);
    }

    pub fn ecs_read_profiled(&self, label: &'static str) -> ProfiledEcsReadGuard<'_> {
        let wait_started_at = Instant::now();
        let guard = self.ecs.read();
        let wait = wait_started_at.elapsed();
        if wait > ECS_LOCK_PROFILE_THRESHOLD {
            tracing::warn!(
                target: "tickprof",
                label,
                wait = ?wait,
                threshold = ?ECS_LOCK_PROFILE_THRESHOLD,
                "ECS read lock wait over threshold"
            );
        }
        ProfiledEcsReadGuard {
            label,
            acquired_at: Instant::now(),
            guard: Some(guard),
        }
    }

    pub fn ecs_write_profiled(&self, label: &'static str) -> ProfiledEcsWriteGuard<'_> {
        let wait_started_at = Instant::now();
        let guard = self.ecs.write();
        let wait = wait_started_at.elapsed();
        if wait > ECS_LOCK_PROFILE_THRESHOLD {
            tracing::warn!(
                target: "tickprof",
                label,
                wait = ?wait,
                threshold = ?ECS_LOCK_PROFILE_THRESHOLD,
                "ECS write lock wait over threshold"
            );
        }
        ProfiledEcsWriteGuard {
            label,
            acquired_at: Instant::now(),
            guard: Some(guard),
        }
    }

    #[allow(clippy::too_many_lines)]
    pub async fn new(
        world: Arc<World>,
        database: Arc<Database>,
        config: Config,
    ) -> anyhow::Result<Arc<Self>> {
        let mut schedule_hazards = Schedule::default();
        schedule_hazards.add_systems(combat::standing_cell_hazard_system);

        let mut schedule_physics = Schedule::default();
        granular::add_granular_physics_system(&mut schedule_physics);

        let mut schedule_guns = Schedule::default();
        schedule_guns.add_systems(combat::gun_firing_system);

        let mut schedule_programmator = Schedule::default();
        schedule_programmator.add_systems(programmator::programmator_system);

        let mut schedule_alive = Schedule::default();
        schedule_alive.add_systems(alive::alive_physics_system);

        let mut schedule_building_visual_effects = Schedule::default();
        schedule_building_visual_effects.add_systems(building_damage::building_effect_tick_system);

        let mut schedule_building_crafting = Schedule::default();
        schedule_building_crafting.add_systems(building_damage::crafter_completion_resend_system);

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
                activity: ScheduleActivity::DueHazards,
                schedule: RwLock::new(schedule_hazards),
                interval_ms: std::sync::atomic::AtomicU64::new(schedule_intervals.hazards_ms),
            },
            GameSchedule {
                name: "physics".to_string(),
                activity: ScheduleActivity::ActiveGranular,
                schedule: RwLock::new(schedule_physics),
                interval_ms: std::sync::atomic::AtomicU64::new(schedule_intervals.physics_ms),
            },
            GameSchedule {
                name: "guns".to_string(),
                activity: ScheduleActivity::DueGuns,
                schedule: RwLock::new(schedule_guns),
                interval_ms: std::sync::atomic::AtomicU64::new(schedule_intervals.guns_ms),
            },
            GameSchedule {
                name: "programmator".to_string(),
                activity: ScheduleActivity::DueProgrammator,
                schedule: RwLock::new(schedule_programmator),
                interval_ms: std::sync::atomic::AtomicU64::new(schedule_intervals.programmator_ms),
            },
            GameSchedule {
                name: "alive".to_string(),
                activity: ScheduleActivity::ActiveAlive,
                schedule: RwLock::new(schedule_alive),
                interval_ms: std::sync::atomic::AtomicU64::new(schedule_intervals.alive_ms),
            },
            GameSchedule {
                name: "building_visual_effects".to_string(),
                activity: ScheduleActivity::OnlinePlayers,
                schedule: RwLock::new(schedule_building_visual_effects),
                interval_ms: std::sync::atomic::AtomicU64::new(
                    schedule_intervals.building_effects_ms,
                ),
            },
            GameSchedule {
                name: "building_crafting".to_string(),
                activity: ScheduleActivity::DueCrafting,
                schedule: RwLock::new(schedule_building_crafting),
                interval_ms: std::sync::atomic::AtomicU64::new(
                    schedule_intervals.building_effects_ms,
                ),
            },
            GameSchedule {
                name: "hourly_damage".to_string(),
                activity: ScheduleActivity::Always,
                schedule: RwLock::new(schedule_hourly_damage),
                interval_ms: std::sync::atomic::AtomicU64::new(schedule_intervals.hourly_damage_ms),
            },
        ];

        let ingress = config.gameplay.simulation;
        let (lifecycle_tx, lifecycle_rx) = mpsc::channel(ingress.lifecycle_ingress_capacity);
        let (gameplay_tx, gameplay_rx) = mpsc::channel(ingress.gameplay_ingress_capacity);
        let (internal_tx, internal_rx) = mpsc::channel(ingress.internal_ingress_capacity);
        let max_chat_id = database.get_max_chat_id().await.unwrap_or(0);
        let simulation_waker = crate::simulation_waker::SimulationWaker::default();
        let command_ingress = CommandIngress::new(
            CommandSenders {
                lifecycle: lifecycle_tx,
                gameplay: gameplay_tx,
                internal: internal_tx,
            },
            simulation_waker.clone(),
        );
        let due_schedules = DueSchedules::new(simulation_waker.clone());
        let state = Arc::new(Self {
            world,
            db: database,
            config,
            player_registry: PlayerRegistry::new(),
            building_index: BuildingIndex::new(),
            chat_channels: RwLock::new(default_channels),
            active_events: RwLock::new(ActiveEvents::default()),
            ecs: RwLock::new(EcsWorld::new()),
            schedules,
            auth_failures: DashMap::new(),
            commands_rx: Mutex::new(Some(CommandReceivers {
                lifecycle: lifecycle_rx,
                gameplay: gameplay_rx,
                internal: internal_rx,
                next_class: 0,
            })),
            command_ingress,
            simulation_waker,
            due_schedules,
            bots_render_schedule: Mutex::new(BotsRenderSchedule::default()),
            bots_render_slot_seq: std::sync::atomic::AtomicU64::new(0),
            tokio_handle: tokio::runtime::Handle::current(),
            sessions: crate::net::session::hub::SessionHub::default(),
            box_index: Arc::new(DashMap::new()),
            box_pickup_queue: BoxPickupQueue::default(),
            death_queue: combat::DeathQueue::default(),
            granular_wake_q: GranularWakeQueue::default(),
            alive_work_q: alive::AliveWorkQueue::default(),
            crystal_economy: Mutex::new(crate::game::market::CrystalEconomy::default()),
            consumable_packs: DashMap::new(),
            db_pending_tasks: std::sync::atomic::AtomicUsize::new(0),
            rate_limiters: DashMap::new(),
            chat_id_seq: std::sync::atomic::AtomicI64::new(max_chat_id),
            web_snapshot: WebSnapshotOwner::new(),
        });

        // Боксы из БД → in-memory индекс (один раз; на hot-path SQLite по
        // боксам больше не дёргаем).
        let box_rows = state
            .db
            .load_all_boxes()
            .await
            .context("load boxes into runtime index")?;
        for (bx, by, crystals) in box_rows {
            state.box_index.insert((bx, by).into(), crystals);
        }
        tracing::info!(
            "Loaded {} boxes into in-memory index",
            state.box_index.len()
        );

        let event_rows = state
            .db
            .load_all_events()
            .await
            .context("load active events from database")?;
        {
            let mut events = state.active_events.write();
            for r in event_rows {
                #[derive(serde::Deserialize)]
                struct Config {
                    xp_mult: f64,
                    drop_mult: f64,
                }
                let cfg: Config = serde_json::from_str(&r.config_json)
                    .with_context(|| format!("parse active event config id={}", r.id))?;
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

        {
            let mut ecs = state.ecs_write_profiled("game.init_resources");
            ecs.insert_resource(WorldResource(state.world.clone()));
            ecs.insert_resource(ProgrammatorConfigResource(
                state.config.gameplay.programmator,
            ));
            ecs.insert_resource(CombatConfigResource(state.config.gameplay.combat));
            ecs.insert_resource(ScheduleConfigResource(state.config.gameplay.schedules));
            ecs.insert_resource(state.box_pickup_queue.clone());
            ecs.insert_resource(state.granular_wake_q.clone());
            ecs.insert_resource(state.alive_work_q.clone());
            ecs.insert_resource(state.death_queue.clone());
            ecs.insert_resource(BroadcastQueue::default());
            ecs.insert_resource(ProgrammatorQueue::default());
            ecs.insert_resource(ProgrammatorDueQueue::new(
                state.due_schedules.programmator_schedule(),
            ));
            ecs.insert_resource(ProgrammatorDueBatch::default());
            ecs.insert_resource(HazardDueBatch::default());
            ecs.insert_resource(StandingCellHazardContext {
                box_pickups: state.box_pickup_queue.clone(),
                death_queue: state.death_queue.clone(),
                due_queue: HazardDueQueue::new(state.due_schedules.hazard_schedule()),
                interval: Duration::from_millis(state.config.gameplay.schedules.hazards_ms),
                slow_threshold: Duration::from_millis(
                    state.config.gameplay.schedules.schedule_warn_threshold_ms,
                )
                .min(Duration::from_millis(
                    state.config.gameplay.schedules.game_loop_tick_rate_ms,
                )),
            });
            ecs.insert_resource(combat::GunTickTimer::default());
            ecs.insert_resource(combat::GunCandidateBatch::default());
            ecs.insert_resource(PendingCellConversions::default());
            ecs.insert_resource(PackResendQueue::default());
            ecs.insert_resource(building_damage::CraftingDueBatch::default());
            ecs.insert_resource(DirtyBuildings::default());
            ecs.insert_resource(DirtyPlayers::default());
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
        let mut ecs = state.ecs_write_profiled("game.load_buildings_into_ecs");
        let mut spot_count = 0u32;
        for row in all_rows {
            let (entity, pack_type) = buildings::spawn_building_from_row(&mut ecs, &row)?;
            state.register_building_entity(row.x, row.y, entity);
            if row.craft_recipe_id.is_some() && !row.craft_ready {
                state.schedule_crafting_completion(entity, row.craft_end_ts);
            }

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
                state.register_botspot_entity(
                    row.owner_id.into(),
                    row.x,
                    row.y,
                    row.clan_id,
                    botspot_entity,
                );
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
        self.player_registry.get_player_entity(pid)
    }

    /// Выдать новый токен сеанса (монотонный, уникальный на процесс).
    pub fn schedule_crafting_completion(&self, entity: Entity, end_ts: i64) {
        self.due_schedules
            .schedule_crafting_completion(entity, end_ts);
    }

    pub fn next_crafting_due_ts(&self) -> Option<i64> {
        self.due_schedules.next_crafting_due_ts()
    }

    pub fn has_due_crafting(&self, now_ts: i64) -> bool {
        self.due_schedules.has_due_crafting(now_ts)
    }

    pub fn schedule_programmator(&self, entity: Entity, due_at: Instant) {
        self.due_schedules.schedule_programmator(entity, due_at);
    }

    pub fn next_programmator_due_at(&self) -> Option<Instant> {
        self.due_schedules.next_programmator_due_at()
    }

    pub fn has_due_programmator(&self, now: Instant) -> bool {
        self.due_schedules.has_due_programmator(now)
    }

    pub fn take_due_programmators(&self, now: Instant) -> Vec<(Entity, Instant)> {
        self.due_schedules
            .take_due_programmators(now, Self::PROGRAMMATOR_DUE_BATCH_BUDGET)
    }

    pub fn schedule_hazard(&self, entity: Entity, due_at: Instant) {
        self.due_schedules.schedule_hazard(entity, due_at);
    }

    pub fn next_hazard_due_at(&self) -> Option<Instant> {
        self.due_schedules.next_hazard_due_at()
    }

    pub fn take_due_hazards(&self, now: Instant) -> Vec<(Entity, Instant)> {
        self.due_schedules
            .take_due_hazards(now, Self::HAZARD_DUE_BATCH_BUDGET)
    }

    pub fn take_due_crafting(
        &self,
        now_ts: i64,
    ) -> (Vec<building_damage::CraftingDue>, bool, usize) {
        self.due_schedules
            .take_due_crafting(now_ts, Self::CRAFTING_DUE_BATCH_BUDGET)
    }

    /// Проверить chat rate limit для игрока. Возвращает `true` если разрешено.
    pub fn check_chat_rate(&self, pid: PlayerId) -> bool {
        let rl = &self.config.gameplay.rate_limits;
        let limiters = self.rate_limiters.entry(pid).or_insert_with(|| {
            crate::net::session::rate_limit::PlayerLimiters::new(
                rl.chat_per_sec,
                rl.chat_burst,
                rl.gui_per_sec,
                rl.gui_burst,
            )
        });
        limiters.chat.check().is_ok()
    }

    /// Проверить GUI rate limit для игрока. Возвращает `true` если разрешено.
    pub fn check_gui_rate(&self, pid: PlayerId) -> bool {
        let rl = &self.config.gameplay.rate_limits;
        let limiters = self.rate_limiters.entry(pid).or_insert_with(|| {
            crate::net::session::rate_limit::PlayerLimiters::new(
                rl.chat_per_sec,
                rl.chat_burst,
                rl.gui_per_sec,
                rl.gui_burst,
            )
        });
        limiters.gui.check().is_ok()
    }

    /// Удалить rate limiter при дисконнекте игрока (утечка памяти иначе).
    pub fn remove_rate_limiter(&self, pid: PlayerId) {
        self.rate_limiters.remove(&pid);
    }

    pub async fn enqueue_lifecycle(
        &self,
        player_id: PlayerId,
        session_id: SessionId,
        command: PlayerCommand,
    ) -> bool {
        self.command_ingress
            .enqueue_lifecycle(player_id, session_id, command)
            .await
    }

    pub fn enqueue_command(
        &self,
        player_id: PlayerId,
        session_id: SessionId,
        command: GameCommand,
    ) -> bool {
        self.command_ingress
            .enqueue_command(player_id, session_id, command)
    }

    pub async fn enqueue_internal(
        &self,
        player_id: PlayerId,
        session_id: SessionId,
        command: PlayerCommand,
    ) -> bool {
        self.command_ingress
            .enqueue_internal(player_id, session_id, command)
            .await
    }

    pub fn enqueue_command_received(
        &self,
        player_id: PlayerId,
        session_id: SessionId,
        command: GameCommand,
        received_at: Instant,
    ) -> bool {
        self.command_ingress
            .enqueue_command_received(player_id, session_id, command, received_at)
    }

    pub(crate) fn refresh_command_ingress_oldest_ages(&self) {
        self.command_ingress.refresh_command_ingress_oldest_ages();
    }

    pub(crate) fn allocate_command_sequence(&self) -> CommandSeq {
        self.command_ingress.allocate_command_sequence()
    }

    pub(crate) fn simulation_waker(&self) -> crate::simulation_waker::SimulationWaker {
        self.command_ingress.simulation_waker()
    }

    pub fn record_command_dequeued(&self, class: CommandIngressClass) {
        self.command_ingress.record_command_dequeued(class);
    }

    pub fn query_player<F, R>(&self, pid: PlayerId, f: F) -> Option<R>
    where
        F: FnOnce(&EcsWorld, Entity) -> R,
    {
        let entity = self.get_player_entity(pid)?;
        let ecs = self.ecs_read_profiled("game.query_player");
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
        let ecs = self.ecs_read_profiled("game.query_player_expected");
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
        let mut ecs = self.ecs_write_profiled("game.modify_player");
        if !ecs.entities().contains(entity) {
            tracing::warn!(player_id = %pid, ?entity, "Player entity exists in active_players but is missing from ECS world during modify!");
            drop(ecs);
            return None;
        }
        let res = f(&mut ecs, entity);
        if ecs
            .get::<PlayerFlags>(entity)
            .is_some_and(|flags| flags.dirty)
        {
            let incarnation = ecs.get::<PlayerFlags>(entity).unwrap().incarnation;
            ecs.resource_mut::<DirtyPlayers>()
                .0
                .insert((entity, incarnation));
        }
        drop(ecs);
        Some(res)
    }

    fn refresh_bots_render_player_in_ecs(&self, pid: PlayerId, entity: Entity, ecs: &EcsWorld) {
        let Some(position) = ecs.get::<PlayerPosition>(entity) else {
            self.player_registry.bots_render_players.remove(&pid);
            return;
        };
        let Some(stats) = ecs.get::<PlayerStats>(entity) else {
            self.player_registry.bots_render_players.remove(&pid);
            return;
        };
        let tail = ecs
            .get::<programmator::ProgrammatorState>(entity)
            .map_or(0, |program| u8::from(program.running));
        self.player_registry.bots_render_players.insert(
            pid,
            BotsRenderPlayer {
                x: position.x,
                y: position.y,
                dir: position.dir,
                skin: stats.skin,
                clan_id: stats.clan_id.unwrap_or(0),
                tail,
            },
        );
    }

    /// Synchronize active players after ECS schedules before a renderer due batch.
    /// The lock covers only component copies; visibility walk and wire encoding use
    /// the immutable cache below without touching ECS.
    pub fn refresh_active_bots_render_players(&self) {
        let active = self
            .player_registry
            .active_players
            .iter()
            .map(|entry| (*entry.key(), entry.ecs_entity))
            .collect::<Vec<_>>();
        let ecs = self.ecs_read_profiled("bots_render.cache_refresh");
        for (player_id, entity) in active {
            self.refresh_bots_render_player_in_ecs(player_id, entity, &ecs);
        }
    }

    pub fn bots_render_player(&self, pid: PlayerId) -> Option<BotsRenderPlayer> {
        self.player_registry
            .bots_render_players
            .get(&pid)
            .map(|entry| *entry)
    }

    pub fn bots_render_botspots_in_chunk(&self, cx: u32, cy: u32) -> Vec<BotSpotView> {
        self.player_registry
            .bots_render_botspots
            .get(&(cx, cy).into())
            .map(|spots| spots.clone())
            .unwrap_or_default()
    }

    pub fn take_dirty_player_entities(&self) -> Vec<(Entity, crate::game::SessionId)> {
        if self
            .ecs_read_profiled("game.peek_dirty_players")
            .resource::<DirtyPlayers>()
            .0
            .is_empty()
        {
            return Vec::new();
        }
        let mut ecs = self.ecs_write_profiled("game.take_dirty_players");
        std::mem::take(&mut ecs.resource_mut::<DirtyPlayers>().0)
            .into_iter()
            .collect()
    }

    pub fn requeue_dirty_player_entities(
        &self,
        entities: impl IntoIterator<Item = (Entity, crate::game::SessionId)>,
    ) {
        self.ecs_write_profiled("game.requeue_dirty_players")
            .resource_mut::<DirtyPlayers>()
            .0
            .extend(entities);
    }

    pub fn snapshot_dirty_players(
        &self,
        entities: &[(Entity, crate::game::SessionId)],
    ) -> Vec<Option<crate::db::PlayerRow>> {
        const BATCH_SIZE: usize = 16;
        let mut results = Vec::with_capacity(entities.len());
        for chunk in entities.chunks(BATCH_SIZE) {
            let mut ecs = self.ecs_write_profiled("game.snapshot_dirty_players");
            for &(entity, incarnation) in chunk {
                results.push(self.snapshot_dirty_player_in_ecs(&mut ecs, entity, incarnation));
            }
            drop(ecs);
        }
        results
    }

    fn snapshot_dirty_player_in_ecs(
        &self,
        ecs: &mut EcsWorld,
        entity: Entity,
        incarnation: crate::game::SessionId,
    ) -> Option<crate::db::PlayerRow> {
        if !ecs.entities().contains(entity) {
            return None;
        }
        let flags = ecs.get::<PlayerFlags>(entity)?;
        if flags.incarnation != incarnation || !flags.dirty {
            return None;
        }
        let player_id = ecs.get::<PlayerMetadata>(entity)?.id;
        if self.get_player_entity(player_id) != Some(entity) {
            return None;
        }
        let row = crate::game::player::extract_player_row(ecs, entity)?;
        ecs.get_mut::<PlayerFlags>(entity)?.dirty = false;
        Some(row)
    }

    pub fn set_schedule_interval(&self, name: &str, interval_ms: u64) -> bool {
        let mut updated = false;
        for gs in &self.schedules {
            let matches = gs.name == name
                || (name == "building_effects"
                    && matches!(
                        gs.name.as_str(),
                        "building_visual_effects" | "building_crafting"
                    ));
            if matches {
                gs.interval_ms
                    .store(interval_ms, std::sync::atomic::Ordering::Relaxed);
                updated = true;
            }
        }
        if updated {
            self.simulation_waker.wake();
        }
        updated
    }

    pub fn modify_building<F, R>(&self, entity: Entity, f: F) -> R
    where
        F: FnOnce(&mut EcsWorld, Entity) -> R,
    {
        let mut ecs = self.ecs_write_profiled("game.modify_building");
        f(&mut ecs, entity)
    }

    pub fn mark_building_dirty(&self, entity: Entity) -> bool {
        let mut ecs = self.ecs_write_profiled("game.mark_building_dirty");
        {
            let Some(mut flags) = ecs.get_mut::<BuildingFlags>(entity) else {
                return false;
            };
            flags.dirty = true;
        }
        ecs.resource_mut::<DirtyBuildings>().0.insert(entity);
        true
    }

    pub fn take_dirty_building_entities(&self) -> Vec<Entity> {
        if self
            .ecs_read_profiled("game.peek_dirty_buildings")
            .resource::<DirtyBuildings>()
            .0
            .is_empty()
        {
            return Vec::new();
        }
        let mut ecs = self.ecs_write_profiled("game.take_dirty_buildings");
        std::mem::take(&mut ecs.resource_mut::<DirtyBuildings>().0)
            .into_iter()
            .collect()
    }

    pub fn requeue_dirty_building_entities(&self, entities: impl IntoIterator<Item = Entity>) {
        self.ecs_write_profiled("game.requeue_dirty_buildings")
            .resource_mut::<DirtyBuildings>()
            .0
            .extend(entities);
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
            let ecs = self.ecs_read_profiled("game.get_pack_at");
            if ecs.get::<BuildingDeletePending>(entity).is_some() {
                return None;
            }
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
            .by_origin
            .get(&((x, y).into()))
            .map(|entry| *entry.value())
    }

    pub fn has_building_origin(&self, x: i32, y: i32) -> bool {
        self.building_index.by_origin.contains_key(&((x, y).into()))
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
            .by_origin
            .iter()
            .map(|entry| *entry.value())
            .collect()
    }

    pub fn building_entities_in_chunk_snapshot(&self, cx: u32, cy: u32) -> Vec<Entity> {
        self.building_index
            .chunk_buildings
            .get(&(cx, cy).into())
            .map_or_else(Vec::new, |entities| entities.value().clone())
    }

    fn find_pack_covering_with(
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
        Self::find_pack_covering_with(&ecs, &self.building_index.chunk_buildings, x, y)
    }

    pub fn find_pack_covering_in_ecs(&self, ecs: &EcsWorld, x: i32, y: i32) -> Option<(i32, i32)> {
        Self::find_pack_covering_with(ecs, &self.building_index.chunk_buildings, x, y)
    }

    pub fn pack_block_pos(&self, x: i32, y: i32) -> Option<i32> {
        if !self.world.valid_coord(x, y) {
            return None;
        }
        block_pos_from_cell(x, y, self.world.chunks_w().cast_signed())
    }

    pub fn put_consumable_pack(&self, x: i32, y: i32, typ: u8, off: u8) {
        self.consumable_packs.insert((x, y).into(), (typ, off));
    }

    pub fn remove_consumable_pack(&self, x: i32, y: i32) {
        self.consumable_packs.remove(&((x, y).into()));
    }

    pub fn consumable_packs_in_block(&self, block_pos: i32) -> Vec<(i32, i32, u8, u8)> {
        self.consumable_packs
            .iter()
            .filter_map(|entry| {
                let pos = *entry.key();
                let (x, y) = (pos.0, pos.1);
                (self.pack_block_pos(x, y) == Some(block_pos)).then(|| {
                    let (typ, off) = *entry.value();
                    (x, y, typ, off)
                })
            })
            .collect()
    }

    /// C# `World.AccessGun` → `(access, anygun)`. `access`: нет вражеской ЗАРЯЖЕННОЙ
    /// пушки в радиусе 20. `anygun`: есть ЛЮБАЯ пушка в радиусе (для Gate-item).
    fn access_gun_with(
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
                        if ecs.get::<BuildingDeletePending>(entity).is_some() {
                            continue;
                        }
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
        Self::access_gun_with(
            &ecs,
            &self.building_index.chunk_buildings,
            x,
            y,
            player_clan_id,
        )
    }

    pub fn access_gun_full_in_ecs(
        &self,
        ecs: &EcsWorld,
        x: i32,
        y: i32,
        player_clan_id: i32,
    ) -> (bool, bool) {
        Self::access_gun_with(
            ecs,
            &self.building_index.chunk_buildings,
            x,
            y,
            player_clan_id,
        )
    }

    /// Паки (HB-оверлей) ровно в ОДНОМ чанке `(cx, cy)`. В отличие от
    /// `get_packs_in_chunk_area` (5×5 область), не захватывает соседние чанки —
    /// нужно при per-чанковой отправке/очистке HB (`chunks.rs`), иначе очистка
    /// ушедшего чанка затирала бы оверлеи паков в ещё видимых соседних чанках
    /// (баг «паки мерцают/пропадают на границе чанка»).
    pub fn get_packs_in_single_chunk_with_ecs(
        &self,
        ecs: &EcsWorld,
        cx: u32,
        cy: u32,
    ) -> Vec<PackOverlay> {
        let mut results = Vec::new();
        let now = crate::time::now_unix();
        if let Some(entities) = self.building_index.chunk_buildings.get(&(cx, cy).into()) {
            for &entity in entities.value() {
                let pos = ecs.get::<GridPosition>(entity);
                let meta = ecs.get::<BuildingMetadata>(entity);
                let own = ecs.get::<BuildingOwnership>(entity);
                let stats = ecs.get::<BuildingStats>(entity);
                let craft = ecs.get::<structures::buildings::BuildingCrafting>(entity);
                if let (Some(pos), Some(meta), Some(own), Some(stats)) = (pos, meta, own, stats)
                    && meta.pack_type.included_in_hb_overlay()
                {
                    results.push(PackOverlay {
                        code: meta.pack_type.code(),
                        x: u16::try_from(pos.x.rem_euclid(65536)).unwrap_or(0),
                        y: u16::try_from(pos.y.rem_euclid(65536)).unwrap_or(0),
                        clan: u8::try_from(own.clan_id.clamp(0, 255)).unwrap_or(0),
                        off: pack_overlay_off(meta.pack_type, stats.charge, craft, now),
                    });
                }
            }
        }
        results
    }

    pub fn get_packs_in_chunk_area(&self, cx: u32, cy: u32) -> Vec<PackOverlay> {
        let mut results = Vec::new();
        let now = crate::time::now_unix();
        let ecs = self.ecs.read();
        for (ucx, ucy) in self.visible_chunks_around(cx, cy) {
            if let Some(entities) = self.building_index.chunk_buildings.get(&(ucx, ucy).into()) {
                for &entity in entities.value() {
                    let pos = ecs.get::<GridPosition>(entity);
                    let meta = ecs.get::<BuildingMetadata>(entity);
                    let own = ecs.get::<BuildingOwnership>(entity);
                    let stats = ecs.get::<BuildingStats>(entity);
                    let craft = ecs.get::<structures::buildings::BuildingCrafting>(entity);
                    if let (Some(pos), Some(meta), Some(own), Some(stats)) = (pos, meta, own, stats)
                        && meta.pack_type.included_in_hb_overlay()
                    {
                        results.push(PackOverlay {
                            code: meta.pack_type.code(),
                            x: u16::try_from(pos.x.rem_euclid(65536)).unwrap_or(0),
                            y: u16::try_from(pos.y.rem_euclid(65536)).unwrap_or(0),
                            clan: u8::try_from(own.clan_id.clamp(0, 255)).unwrap_or(0),
                            off: pack_overlay_off(meta.pack_type, stats.charge, craft, now),
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
        if let Some(tx) = self.sessions.outbox_for_player(pid) {
            let _ = tx.send(data);
        }
    }

    pub fn player_sender(&self, pid: PlayerId) -> Option<crate::net::session::outbox::Outbox> {
        self.sessions.outbox_for_player(pid)
    }

    pub fn is_player_connected(&self, pid: PlayerId) -> bool {
        self.sessions.is_player_connected(pid)
    }

    pub fn active_player_ids(&self) -> Vec<PlayerId> {
        self.player_registry.active_player_ids()
    }

    pub fn guns_due(&self, now: Instant) -> bool {
        let ecs = self.ecs.read();
        let interval = std::time::Duration::from_millis(
            ecs.resource::<CombatConfigResource>()
                .0
                .gun_fire_interval_ms,
        );
        ecs.resource::<combat::GunTickTimer>()
            .is_due_at(now, interval)
    }

    pub fn fill_gun_candidate_batch(&self, ecs: &EcsWorld) -> combat::GunCandidateBatch {
        let mut players = self
            .player_registry
            .active_players
            .iter()
            .map(|entry| entry.ecs_entity)
            .filter(|entity| ecs.get::<player::PlayerPosition>(*entity).is_some())
            .collect::<Vec<_>>();
        players.sort_unstable_by_key(|entity| entity.to_bits());

        let mut guns = Vec::new();
        for player_entity in &players {
            let Some(position) = ecs.get::<player::PlayerPosition>(*player_entity) else {
                continue;
            };
            let (cx, cy) = World::chunk_pos(position.x, position.y);
            for chunk in gun_candidate_chunks(cx, cy) {
                guns.extend(self.building_entities_in_chunk_snapshot(chunk.0, chunk.1));
            }
        }
        guns.sort_unstable_by_key(|entity| entity.to_bits());
        guns.dedup();
        combat::GunCandidateBatch { guns, players }
    }

    pub fn active_session_for_player(&self, pid: PlayerId) -> Option<SessionId> {
        self.player_registry.active_session_for_player(pid)
    }

    pub fn nearby_session_ids(
        &self,
        cx: u32,
        cy: u32,
        exclude_id: Option<PlayerId>,
    ) -> Vec<SessionId> {
        self.nearby_player_sessions(cx, cy)
            .into_iter()
            .filter(|(player_id, _)| Some(*player_id) != exclude_id)
            .map(|(_, session_id)| session_id)
            .collect()
    }

    pub fn nearby_player_sessions(&self, cx: u32, cy: u32) -> Vec<(PlayerId, SessionId)> {
        let mut sessions = Vec::new();
        for (ncx, ncy) in self.visible_chunks_iter(cx, cy) {
            if let Some(players) = self.player_registry.chunk_players.get(&(ncx, ncy).into()) {
                sessions.extend(players.iter().filter_map(|player_id| {
                    self.player_registry
                        .active_players
                        .get(player_id)
                        .map(|active| (*player_id, active.session_id))
                }));
            }
        }
        sessions
    }

    pub fn session_ids_in_chunk(
        &self,
        cx: u32,
        cy: u32,
        exclude_id: Option<PlayerId>,
    ) -> Vec<SessionId> {
        let Some(players) = self.player_registry.chunk_players.get(&(cx, cy).into()) else {
            return Vec::new();
        };
        players
            .iter()
            .copied()
            .filter(|player_id| Some(*player_id) != exclude_id)
            .filter_map(|player_id| {
                self.player_registry
                    .active_players
                    .get(&player_id)
                    .map(|active| active.session_id)
            })
            .collect()
    }

    pub fn is_player_active(&self, pid: PlayerId) -> bool {
        self.player_registry.is_player_active(pid)
    }

    pub fn online_count(&self) -> usize {
        self.player_registry.active_players.len()
    }

    pub fn register_active_player(&self, pid: PlayerId, entity: Entity, session_id: SessionId) {
        self.player_registry.active_players.insert(
            pid,
            ActivePlayer {
                ecs_entity: entity,
                session_id,
            },
        );
        let ecs = self.ecs_read_profiled("bots_render.player_register");
        self.refresh_bots_render_player_in_ecs(pid, entity, &ecs);
        drop(ecs);
        let tick_ms = self.config.gameplay.schedules.game_loop_tick_rate_ms.max(1);
        let interval_ms = u64::try_from(Self::BOTS_RENDER_INTERVAL.as_millis()).unwrap_or(u64::MAX);
        let slots = interval_ms.checked_div(tick_ms).unwrap_or(1).max(1);
        let slot = self
            .bots_render_slot_seq
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            % slots
            + 1;
        let due_at = Instant::now() + Duration::from_millis(tick_ms.saturating_mul(slot));
        self.bots_render_schedule.lock().schedule(BotsRenderDue {
            due_at,
            player_id: pid,
            session_token: session_id.get(),
        });
    }

    pub fn remove_active_player(&self, pid: PlayerId) -> Option<ActivePlayer> {
        self.player_registry.bots_render_players.remove(&pid);
        self.player_registry
            .active_players
            .remove(&pid)
            .map(|(_, active)| active)
    }

    pub fn active_player_entity_for_session(
        &self,
        pid: PlayerId,
        session_id: SessionId,
    ) -> Option<Entity> {
        self.player_registry
            .active_players
            .get(&pid)
            .filter(|active| active.session_id == session_id)
            .map(|active| active.ecs_entity)
    }

    pub fn player_entity_ids(&self) -> Vec<PlayerId> {
        self.player_registry
            .player_entities
            .iter()
            .map(|entry| *entry.key())
            .collect()
    }

    pub fn player_entity_count(&self) -> usize {
        self.player_registry.player_entities.len()
    }

    pub fn register_player_entity(&self, pid: PlayerId, entity: Entity) {
        self.player_registry.player_entities.insert(pid, entity);
    }

    pub fn unregister_player_entity(&self, pid: PlayerId) {
        self.player_registry.player_entities.remove(&pid);
    }

    pub fn take_due_bots_render(&self, now: Instant, limit: usize) -> Vec<BotsRenderDue> {
        let mut due = Vec::with_capacity(limit);
        while due.len() < limit {
            let Some(candidate) = self.bots_render_schedule.lock().pop_due(now) else {
                break;
            };
            if self
                .player_registry
                .active_players
                .get(&candidate.player_id)
                .is_some_and(|active| active.session_id.get() == candidate.session_token)
            {
                due.push(candidate);
            }
        }
        due
    }

    pub fn next_bots_render_at(&self) -> Option<Instant> {
        self.bots_render_schedule.lock().next_due_at()
    }

    pub fn reschedule_bots_render(&self, due: BotsRenderDue, next_at: Instant) {
        if self
            .player_registry
            .active_players
            .get(&due.player_id)
            .is_some_and(|active| active.session_id.get() == due.session_token)
        {
            self.bots_render_schedule.lock().schedule(BotsRenderDue {
                due_at: next_at,
                ..due
            });
        }
    }

    pub fn kick_player(&self, pid: PlayerId) -> bool {
        self.sessions.kick_player(pid)
    }

    pub fn wake_granular_neighborhood(&self, x: i32, y: i32) {
        self.granular_wake_q.wake_neighborhood(x, y);
        self.simulation_waker.wake();
    }

    pub fn seed_granular_region(&self, x: i32, y: i32) {
        self.granular_wake_q.seed_region(x, y);
        self.simulation_waker.wake();
    }

    pub fn has_granular_work(&self) -> bool {
        self.granular_wake_q.has_work()
    }

    pub fn seed_alive_region(&self, x: i32, y: i32) {
        self.alive_work_q.seed_region(x, y);
        self.simulation_waker.wake();
    }

    /// Incrementally discover only the strip newly entering an alive-cell view.
    pub fn wake_alive_movement(&self, from_x: i32, from_y: i32, to_x: i32, to_y: i32) {
        const RADIUS: i32 = 16;
        let dx = to_x - from_x;
        let dy = to_y - from_y;
        if dx.unsigned_abs() > 1 || dy.unsigned_abs() > 1 {
            self.seed_alive_region(to_x, to_y);
            return;
        }
        if dx != 0 {
            let x = to_x + dx.signum() * RADIUS;
            for y in to_y - RADIUS..=to_y + RADIUS {
                if self.world.valid_coord(x, y) {
                    self.alive_work_q
                        .note_cell(x, y, self.world.get_cell_typed(x, y));
                }
            }
        }
        if dy != 0 {
            let y = to_y + dy.signum() * RADIUS;
            for x in to_x - RADIUS..=to_x + RADIUS {
                if self.world.valid_coord(x, y) {
                    self.alive_work_q
                        .note_cell(x, y, self.world.get_cell_typed(x, y));
                }
            }
        }
        self.simulation_waker.wake();
    }

    pub fn has_alive_work(&self) -> bool {
        self.alive_work_q.has_work()
    }

    pub fn broadcast_cell_update(&self, x: i32, y: i32) {
        if let Some(sub) = self.cell_update_subpacket(x, y) {
            self.broadcast_hb_at(x, y, &[sub], None);
        }
    }

    pub fn queue_cell_update(&self, x: i32, y: i32) {
        if let Some(sub) = self.cell_update_subpacket(x, y) {
            self.queue_hb_at(x, y, &[sub], None);
        }
    }

    fn cell_update_subpacket(&self, x: i32, y: i32) -> Option<Vec<u8>> {
        use crate::protocol::packets::hb_cell;
        self.wake_granular_neighborhood(x, y);
        let cell = self.world.read_world_cell(x, y)?;
        self.alive_work_q.note_cell(x, y, cell.cell_type);
        Some(hb_cell(
            u16::try_from(x.rem_euclid(65536)).unwrap_or(0),
            u16::try_from(y.rem_euclid(65536)).unwrap_or(0),
            cell.cell_type.0,
        ))
    }

    /// Зарегистрировать building entity в обоих runtime-индексах.
    /// Callers не должны вручную синхронизировать `building_index` и
    /// `chunk_buildings`: это единый boundary для position→entity кэшей.
    pub fn register_building_entity(&self, x: i32, y: i32, entity: Entity) {
        self.building_index.by_origin.insert((x, y).into(), entity);
        let (cx, cy) = World::chunk_pos(x, y);
        self.building_index
            .chunk_buildings
            .entry((cx, cy).into())
            .or_default()
            .push(entity);
    }

    /// Удалить building entity из обоих runtime-индексов.
    pub fn remove_building_entity(&self, x: i32, y: i32) -> Option<Entity> {
        let (_, entity) = self.building_index.by_origin.remove(&((x, y).into()))?;
        let (cx, cy) = World::chunk_pos(x, y);
        if let Some(mut entities) = self
            .building_index
            .chunk_buildings
            .get_mut(&(cx, cy).into())
        {
            entities.retain(|&ent| ent != entity);
        }
        Some(entity)
    }

    fn remove_building_entity_if(&self, x: i32, y: i32, expected: Entity) -> Option<Entity> {
        let (_, entity) = self
            .building_index
            .by_origin
            .remove_if(&((x, y).into()), |_, entity| *entity == expected)?;
        let (cx, cy) = World::chunk_pos(x, y);
        if let Some(mut entities) = self
            .building_index
            .chunk_buildings
            .get_mut(&(cx, cy).into())
        {
            entities.retain(|&entity| entity != expected);
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
            let mut ecs = self.ecs_write_profiled("game.spawn_building_runtime");
            buildings::spawn_building_from_extra(&mut ecs, spec)
        };
        self.register_building_entity(spec.x, spec.y, entity);
        if spec.extra.craft_recipe_id.is_some() && !spec.extra.craft_ready {
            self.schedule_crafting_completion(entity, spec.extra.craft_end_ts);
        }
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

    /// Runtime apply подтверждённого persisted delete.
    pub fn remove_building_runtime(
        &self,
        view: &PackView,
        expected_entity: Entity,
    ) -> Option<Vec<WorldPos>> {
        let entity = self.remove_building_entity_if(view.x, view.y, expected_entity)?;
        if view.pack_type == PackType::Spot {
            self.remove_botspot_runtime(view.owner_id, view.x, view.y);
        }
        self.ecs_write_profiled("game.remove_building_runtime")
            .despawn(entity);
        Some(self.clear_building_footprint_authoritative(view))
    }

    /// Runtime removal `BotSpot`, связанного со Spot-зданием.
    pub fn remove_botspot_runtime(&self, owner_id: PlayerId, x: i32, y: i32) -> Option<Entity> {
        let (_, entity) = self.building_index.botspot_index.remove(&owner_id)?;
        let (cx, cy) = World::chunk_pos(x, y);
        let chunk_pos = ChunkPos::from((cx, cy));
        if let Some(mut spots) = self.building_index.chunk_botspots.get_mut(&chunk_pos) {
            spots.retain(|&ent| ent != entity);
        }
        if let Some(mut spots) = self
            .player_registry
            .bots_render_botspots
            .get_mut(&chunk_pos)
        {
            spots.retain(|spot| spot.bot_id != -i32::from(owner_id));
        }
        self.ecs_write_profiled("game.remove_botspot_runtime")
            .despawn(entity);
        Some(entity)
    }

    /// Runtime spawn `BotSpot`, связанного со Spot-зданием.
    pub fn spawn_botspot_runtime(
        &self,
        owner_id: PlayerId,
        x: i32,
        y: i32,
        clan_id: i32,
        building_entity: Entity,
    ) -> Entity {
        let botspot_entity = self
            .ecs_write_profiled("game.spawn_botspot_runtime")
            .spawn((
                botspot::BotSpotMarker,
                botspot::BotSpotData {
                    bot_id: -i32::from(owner_id),
                    owner_id,
                    clan_id,
                    x,
                    y,
                    dir: 0,
                    building_entity,
                },
                botspot::BotSpotBasket::default(),
                programmator::ProgrammatorState::new(),
            ))
            .id();
        self.register_botspot_entity(owner_id, x, y, clan_id, botspot_entity);
        tracing::info!(owner_id = %owner_id, x, y, "Spawned BotSpot entity for Spot building");
        botspot_entity
    }

    fn register_botspot_entity(
        &self,
        owner_id: PlayerId,
        x: i32,
        y: i32,
        clan_id: i32,
        entity: Entity,
    ) {
        self.building_index.botspot_index.insert(owner_id, entity);
        let (cx, cy) = World::chunk_pos(x, y);
        self.building_index
            .chunk_botspots
            .entry((cx, cy).into())
            .or_default()
            .push(entity);
        self.player_registry
            .bots_render_botspots
            .entry((cx, cy).into())
            .or_default()
            .push(BotSpotView {
                bot_id: -i32::from(owner_id),
                x,
                y,
                dir: 0,
                clan_id,
            });
    }

    pub fn botspots_in_chunk_with_ecs(&self, ecs: &EcsWorld, cx: u32, cy: u32) -> Vec<BotSpotView> {
        let entities = self
            .building_index
            .chunk_botspots
            .get(&(cx, cy).into())
            .map(|chunk| chunk.clone())
            .unwrap_or_default();
        if entities.is_empty() {
            return Vec::new();
        }

        entities
            .into_iter()
            .filter_map(|entity| {
                let data = ecs.get::<botspot::BotSpotData>(entity)?;
                Some(BotSpotView {
                    bot_id: data.bot_id,
                    x: data.x,
                    y: data.y,
                    dir: data.dir,
                    clan_id: data.clan_id,
                })
            })
            .collect()
    }

    pub fn players_in_chunk(&self, cx: u32, cy: u32) -> Vec<PlayerId> {
        self.player_registry
            .chunk_players
            .get(&(cx, cy).into())
            .map(|players| players.clone())
            .unwrap_or_default()
    }

    pub fn register_player_chunk(&self, pid: PlayerId, cx: u32, cy: u32) {
        let mut players = self
            .player_registry
            .chunk_players
            .entry((cx, cy).into())
            .or_default();
        if !players.contains(&pid) {
            players.push(pid);
        }
    }

    pub fn unregister_player_from_chunk(&self, pid: PlayerId, cx: u32, cy: u32) {
        if let Some(mut players) = self.player_registry.chunk_players.get_mut(&(cx, cy).into()) {
            players.retain(|&id| id != pid);
        }
    }

    #[allow(dead_code)]
    pub fn unregister_player_from_all_chunks(&self, pid: PlayerId) {
        self.player_registry
            .chunk_players
            .iter_mut()
            .for_each(|mut e| e.value_mut().retain(|&id| id != pid));
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
        for position in self.clear_building_footprint_authoritative(view) {
            self.broadcast_cell_update(position.0, position.1);
        }
    }

    fn clear_building_footprint_authoritative(&self, view: &PackView) -> Vec<WorldPos> {
        let mut changed_cells = Vec::new();
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
            changed_cells.push(WorldPos(x, y));
        }
        changed_cells
    }

    // ─── Боксы: in-memory, без SQLite на hot-path (фикс фриза C-1/C-2/H-1) ──

    pub fn put_box_cell_authoritative(&self, x: i32, y: i32, crystals: [i64; 6]) {
        self.world.set_cell_typed(
            x,
            y,
            crate::world::CellType(crate::world::cells::cell_type::BOX),
        );
        self.box_index.insert((x, y).into(), crystals);
    }

    pub fn remove_box_cell_authoritative(&self, x: i32, y: i32) -> Option<[i64; 6]> {
        let crystals = self
            .box_index
            .remove(&(x, y).into())
            .map(|(_, value)| value);
        if crystals.is_some() {
            self.world.damage_cell(x, y, 1.0);
        }
        crystals
    }

    pub fn request_player_death(&self, player_id: PlayerId) {
        if self.get_player_entity(player_id).is_some() {
            self.death_queue.push(player_id);
            self.simulation_waker.wake();
        }
    }

    pub fn drain_player_deaths(&self) -> Vec<PlayerId> {
        self.death_queue.drain()
    }

    pub fn has_pending_player_deaths(&self) -> bool {
        !self.death_queue.is_empty()
    }

    pub fn request_box_pickup(&self, intent: BoxPickupIntent) {
        if self.get_player_entity(intent.player_id).is_some() {
            self.box_pickup_queue.push(intent);
            self.simulation_waker.wake();
        }
    }

    pub fn drain_box_pickups(&self) -> Vec<BoxPickupIntent> {
        self.box_pickup_queue.drain()
    }

    pub fn has_pending_box_pickups(&self) -> bool {
        !self.box_pickup_queue.is_empty()
    }

    pub fn broadcast_to_nearby(&self, cx: u32, cy: u32, data: &[u8], exclude_id: Option<PlayerId>) {
        // PB-2: итерируем напрямую под guard'ом DashMap — не клонируем Vec<PlayerId>.
        // send_to_player берёт player_tx (другой DashMap-шард) → дедлок невозможен.
        for (ncx, ncy) in self.visible_chunks_iter(cx, cy) {
            if let Some(players) = self.player_registry.chunk_players.get(&(ncx, ncy).into()) {
                for &pid in players.value() {
                    if Some(pid) == exclude_id {
                        continue;
                    }
                    let Some(session_id) = self.active_session_for_player(pid) else {
                        continue;
                    };
                    if let Some(tx) = self.sessions.outbox_for_session(session_id) {
                        let _ = tx.send(data.to_vec());
                    }
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

    /// Отложенная delivery только для command-path: authoritative apply уже завершён,
    /// а одинаковые `HB` соседям можно склеить в один frame на сессию в side phase.
    pub fn queue_hb_at(&self, x: i32, y: i32, subs: &[Vec<u8>], exclude_id: Option<PlayerId>) {
        use crate::net::session::wire::encode_hb_bundle;
        use crate::protocol::packets::hb_bundle;
        let (cx, cy) = World::chunk_pos(x, y);
        self.command_ingress
            .queue_nearby(cx, cy, encode_hb_bundle(&hb_bundle(subs).1), exclude_id);
    }

    pub fn queue_direct(&self, session_id: SessionId, data: Vec<u8>) {
        self.command_ingress.queue_direct(session_id, data);
    }

    pub fn drain_command_broadcasts(&self) -> Vec<BroadcastEffect> {
        self.command_ingress.drain_command_broadcasts()
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

fn gun_candidate_chunks(cx: u32, cy: u32) -> Vec<ChunkPos> {
    let mut chunks = Vec::with_capacity(9);
    for y in cy.saturating_sub(1)..=cy.saturating_add(1) {
        for x in cx.saturating_sub(1)..=cx.saturating_add(1) {
            chunks.push((x, y).into());
        }
    }
    chunks
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
    use super::{block_pos_from_cell, pack_overlay_off};
    use crate::game::PackType;
    use crate::game::structures::buildings::BuildingCrafting;

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

    #[test]
    fn pack_overlay_off_keeps_charge_based_packs_binary() {
        assert_eq!(pack_overlay_off(PackType::Teleport, 0, None, 100), 0);
        assert_eq!(pack_overlay_off(PackType::Teleport, 1, None, 100), 1);
        assert_eq!(pack_overlay_off(PackType::Gun, 100, None, 100), 1);
    }

    #[test]
    fn pack_overlay_off_encodes_crafter_recipe_and_ready_state() {
        let craft = BuildingCrafting {
            recipe_id: Some(2),
            num: 1,
            end_ts: 1_000,
            ready: false,
        };

        assert_eq!(pack_overlay_off(PackType::Craft, 0, Some(&craft), 999), 6);
        assert_eq!(
            pack_overlay_off(PackType::Craft, 0, Some(&craft), 1_000),
            56
        );
    }
}

#[cfg(test)]
mod bots_render_schedule_tests {
    use super::{BotsRenderDue, BotsRenderSchedule};
    use crate::game::PlayerId;
    use std::time::{Duration, Instant};

    #[test]
    fn due_heap_orders_deadlines_and_does_not_run_early() {
        let now = Instant::now();
        let mut schedule = BotsRenderSchedule::default();
        schedule.schedule(BotsRenderDue {
            due_at: now + Duration::from_millis(20),
            player_id: PlayerId(2),
            session_token: 20,
        });
        schedule.schedule(BotsRenderDue {
            due_at: now + Duration::from_millis(10),
            player_id: PlayerId(1),
            session_token: 10,
        });

        assert!(schedule.pop_due(now).is_none());
        assert_eq!(
            schedule
                .pop_due(now + Duration::from_millis(10))
                .unwrap()
                .player_id,
            PlayerId(1)
        );
        assert!(schedule.pop_due(now + Duration::from_millis(19)).is_none());
        assert_eq!(
            schedule
                .pop_due(now + Duration::from_millis(20))
                .unwrap()
                .player_id,
            PlayerId(2)
        );
    }
}
