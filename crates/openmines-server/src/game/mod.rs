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
    BuildingDeleteCause, BuildingDeleteOperationId, BuildingDeleteOrigin, BuildingDeleteRequest,
    BuildingDeleteResult, BuildingIdentity, ChatAppendRequest, CommandEffects, CommandIngressClass,
    CommandSeq, GameCommand, GameEvent, GuiCommand, GuiView, PersistenceCompletion, PlayerCommand,
    PlayerInitView, ProgramCreateRequest, ProgramCreateResult, ProgramSaveRequest,
    ProgramSaveResult, QueuedGameCommand, RemovePack, SaveCommand, SaveKind, SessionId, SimTick,
    TeleportGuiView,
};
pub use logic::{crafting, skills};
pub use mechanics::{building_damage, chat, combat};
pub use structures::buildings;
pub use world::{direction, granular};

use crate::config::CombatConfig;
use crate::config::Config;
use crate::config::ProgrammatorConfig;
use crate::config::ScheduleConfig;
use crate::db::{Database, buildings::BuildingExtra};
use crate::world::{World, WorldProvider};
use anyhow::Context as _;
use bevy_ecs::prelude::{Entity, Resource, Schedule, World as EcsWorld};
use dashmap::DashMap;
use parking_lot::{Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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

// ─── ECS Resources (вместо Arc<GameState> для ECS-систем) ───────────────────

/// Мир (карта/клетки) — выделен из `GameState`, чтобы ECS-системы не зависели
/// от всего `GameState`. Каждая система берёт только то, что реально использует.
#[derive(Resource)]
pub struct WorldResource(pub Arc<crate::world::World>);

#[derive(Resource, Clone, Copy)]
pub struct ProgrammatorConfigResource(pub ProgrammatorConfig);

#[derive(Resource, Clone, Copy)]
pub struct CombatConfigResource(pub CombatConfig);

#[derive(Resource, Clone, Copy)]
pub struct ScheduleConfigResource(pub ScheduleConfig);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BoxPickupSource {
    Standing,
    Dig {
        session_id: Option<SessionId>,
        direction: i32,
        skin: i32,
        clan_id: i32,
        tail: u8,
        exclude_self: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoxPickupIntent {
    pub player_id: PlayerId,
    pub player_pos: WorldPos,
    pub box_pos: WorldPos,
    pub source: BoxPickupSource,
}

#[derive(Default)]
struct BoxPickupQueueState {
    queue: VecDeque<BoxPickupIntent>,
    players: HashSet<PlayerId>,
}

#[derive(Resource, Clone, Default)]
pub struct BoxPickupQueue(Arc<Mutex<BoxPickupQueueState>>);

impl BoxPickupQueue {
    pub fn push(&self, intent: BoxPickupIntent) {
        let mut state = self.0.lock();
        if state.players.insert(intent.player_id) {
            state.queue.push_back(intent);
        }
    }

    pub fn drain(&self) -> Vec<BoxPickupIntent> {
        let mut state = self.0.lock();
        state.players.clear();
        state.queue.drain(..).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.0.lock().queue.is_empty()
    }
}

#[derive(Default)]
struct GranularWakeState {
    points: HashSet<WorldPos>,
    region_seeds: HashSet<WorldPos>,
}

#[derive(Resource, Clone)]
pub struct GranularWakeQueue {
    state: Arc<Mutex<GranularWakeState>>,
    active: Arc<AtomicBool>,
}

impl Default for GranularWakeQueue {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(GranularWakeState::default())),
            active: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl GranularWakeQueue {
    pub fn wake_neighborhood(&self, x: i32, y: i32) {
        let mut state = self.state.lock();
        for dy in -3..=1 {
            for dx in -1..=1 {
                state.points.insert((x + dx, y + dy).into());
            }
        }
    }

    pub fn seed_region(&self, x: i32, y: i32) {
        self.state.lock().region_seeds.insert((x, y).into());
    }

    pub fn take(&self) -> (Vec<WorldPos>, Vec<WorldPos>) {
        let mut state = self.state.lock();
        (
            std::mem::take(&mut state.points).into_iter().collect(),
            std::mem::take(&mut state.region_seeds)
                .into_iter()
                .collect(),
        )
    }

    pub fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::Release);
    }

    pub fn has_work(&self) -> bool {
        self.active.load(Ordering::Acquire) || {
            let state = self.state.lock();
            !state.points.is_empty() || !state.region_seeds.is_empty()
        }
    }
}

#[derive(Resource, Default)]
pub struct BroadcastQueue(pub Vec<BroadcastEffect>);

const ECS_LOCK_PROFILE_THRESHOLD: Duration = Duration::from_millis(25);

pub struct ProfiledEcsReadGuard<'a> {
    label: &'static str,
    acquired_at: Instant,
    guard: Option<RwLockReadGuard<'a, EcsWorld>>,
}

impl Deref for ProfiledEcsReadGuard<'_> {
    type Target = EcsWorld;

    fn deref(&self) -> &Self::Target {
        self.guard
            .as_deref()
            .expect("profiled ECS read guard already dropped")
    }
}

impl Drop for ProfiledEcsReadGuard<'_> {
    fn drop(&mut self) {
        let held = self.acquired_at.elapsed();
        drop(self.guard.take());
        if held > ECS_LOCK_PROFILE_THRESHOLD {
            tracing::warn!(
                target: "tickprof",
                label = self.label,
                held = ?held,
                threshold = ?ECS_LOCK_PROFILE_THRESHOLD,
                "ECS read lock held over threshold"
            );
        }
    }
}

pub struct ProfiledEcsWriteGuard<'a> {
    label: &'static str,
    acquired_at: Instant,
    guard: Option<RwLockWriteGuard<'a, EcsWorld>>,
}

impl Deref for ProfiledEcsWriteGuard<'_> {
    type Target = EcsWorld;

    fn deref(&self) -> &Self::Target {
        self.guard
            .as_deref()
            .expect("profiled ECS write guard already dropped")
    }
}

impl DerefMut for ProfiledEcsWriteGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard
            .as_deref_mut()
            .expect("profiled ECS write guard already dropped")
    }
}

impl Drop for ProfiledEcsWriteGuard<'_> {
    fn drop(&mut self) {
        let held = self.acquired_at.elapsed();
        drop(self.guard.take());
        if held > ECS_LOCK_PROFILE_THRESHOLD {
            tracing::warn!(
                target: "tickprof",
                label = self.label,
                held = ?held,
                threshold = ?ECS_LOCK_PROFILE_THRESHOLD,
                "ECS write lock held over threshold"
            );
        }
    }
}

#[derive(Debug)]
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
pub struct ProgrammatorQueue(pub Vec<ProgrammatorAction>);

pub enum ProgrammatorAction {
    Move {
        pid: PlayerId,
        session_id: Option<SessionId>,
        x: i32,
        y: i32,
        dir: i32,
    },
    Dig {
        pid: PlayerId,
        session_id: Option<SessionId>,
        dir: i32,
    },
    Build {
        pid: PlayerId,
        session_id: Option<SessionId>,
        dir: i32,
        block_type: String,
    },
    Geo {
        pid: PlayerId,
        session_id: Option<SessionId>,
    },
    Heal {
        pid: PlayerId,
        session_id: Option<SessionId>,
    },
    SetAutoDig {
        pid: PlayerId,
        session_id: Option<SessionId>,
        enabled: bool,
    },
    SetAggression {
        pid: PlayerId,
        session_id: Option<SessionId>,
        enabled: bool,
    },
    SetHandMode {
        session_id: Option<SessionId>,
        enabled: bool,
    },
    FillGun {
        pid: PlayerId,
        session_id: Option<SessionId>,
        x: i32,
        y: i32,
    },
    SetProgrammatorStatus {
        session_id: Option<SessionId>,
        running: bool,
    },
    Send {
        session_id: SessionId,
        data: Vec<u8>,
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

/// Запись персистенции бокса: (координата, `Some`=upsert | `None`=delete).
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
    /// HB `O` entry off-byte. For charge-based packs: `charge > 0`; for Craft:
    /// `1 + recipe.result.id`, plus 50 when ready.
    pub off: u8,
}

fn pack_overlay_off(
    pack_type: PackType,
    charge: i32,
    craft: Option<&structures::buildings::BuildingCrafting>,
    now: i64,
) -> u8 {
    if pack_type != PackType::Craft {
        return u8::from(charge > 0);
    }
    let Some(recipe_id) = craft.and_then(|c| c.recipe_id) else {
        return 0;
    };
    let Some(recipe) = crafting::recipe_by_id(recipe_id) else {
        return 0;
    };
    let ready_bonus = if craft.is_some_and(|c| c.end_ts > 0 && now >= c.end_ts) {
        50
    } else {
        0
    };
    u8::try_from(1 + recipe.result.id + ready_bonus)
        .expect("craft recipe overlay item id must fit HB O off byte")
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

struct CommandSenders {
    lifecycle: mpsc::Sender<QueuedGameCommand>,
    gameplay: mpsc::Sender<QueuedGameCommand>,
    internal: mpsc::Sender<QueuedGameCommand>,
}

pub struct CommandReceivers {
    lifecycle: mpsc::Receiver<QueuedGameCommand>,
    gameplay: mpsc::Receiver<QueuedGameCommand>,
    internal: mpsc::Receiver<QueuedGameCommand>,
    #[cfg(test)]
    next_class: usize,
}

#[cfg(test)]
mod command_ingress_tests {
    use super::*;

    fn queued(sequence: u64) -> QueuedGameCommand {
        let now = Instant::now();
        QueuedGameCommand {
            player_id: PlayerId(1),
            session_id: SessionId::new(1),
            ingress_class: Some(CommandIngressClass::Gameplay),
            sequence: CommandSeq::new(sequence),
            received_at: now,
            enqueued_at: now,
            command: GameCommand::Player(PlayerCommand::KnownNoopTy {
                event: "test".to_owned(),
                payload: bytes::Bytes::new(),
            }),
        }
    }

    #[test]
    fn command_receivers_rotate_ready_workload_classes() {
        let (lifecycle_tx, lifecycle) = mpsc::channel(2);
        let (gameplay_tx, gameplay) = mpsc::channel(2);
        let (internal_tx, internal) = mpsc::channel(2);
        lifecycle_tx.try_send(queued(1)).unwrap();
        gameplay_tx.try_send(queued(2)).unwrap();
        internal_tx.try_send(queued(3)).unwrap();
        let mut receivers = CommandReceivers {
            lifecycle,
            gameplay,
            internal,
            next_class: 0,
        };

        assert_eq!(receivers.try_recv().unwrap().sequence.get(), 1);
        assert_eq!(receivers.try_recv().unwrap().sequence.get(), 2);
        assert_eq!(receivers.try_recv().unwrap().sequence.get(), 3);
    }

    #[tokio::test]
    async fn full_gameplay_ingress_rejects_without_consuming_lifecycle_reserve() {
        let mut gameplay = crate::config::GameplayConfig::runtime_baseline();
        gameplay.simulation.gameplay_ingress_capacity = 1;
        gameplay.simulation.lifecycle_ingress_capacity = 1;
        let test = crate::test_support::ServerTestHarness::with_gameplay(
            "bounded_ingress",
            "bounded-ingress-user",
            gameplay,
        )
        .await;
        let gameplay_command = || {
            GameCommand::Player(PlayerCommand::KnownNoopTy {
                event: "test".to_owned(),
                payload: bytes::Bytes::new(),
            })
        };

        assert!(test.state.enqueue_command(
            PlayerId(test.player.id),
            SessionId::new(1),
            gameplay_command()
        ));
        assert!(!test.state.enqueue_command(
            PlayerId(test.player.id),
            SessionId::new(1),
            gameplay_command()
        ));
        assert!(
            test.state
                .enqueue_lifecycle(
                    PlayerId(test.player.id),
                    SessionId::new(1),
                    PlayerCommand::Disconnect
                )
                .await
        );
    }
}

impl CommandReceivers {
    pub fn try_recv_class(
        &mut self,
        class: CommandIngressClass,
    ) -> Result<QueuedGameCommand, mpsc::error::TryRecvError> {
        match class {
            CommandIngressClass::Lifecycle => self.lifecycle.try_recv(),
            CommandIngressClass::Gameplay => self.gameplay.try_recv(),
            CommandIngressClass::Internal => self.internal.try_recv(),
        }
    }

    #[cfg(test)]
    pub fn try_recv(&mut self) -> Result<QueuedGameCommand, mpsc::error::TryRecvError> {
        for offset in 0..3 {
            let class = (self.next_class + offset) % 3;
            let result = self.try_recv_class(match class {
                0 => CommandIngressClass::Lifecycle,
                1 => CommandIngressClass::Gameplay,
                2 => CommandIngressClass::Internal,
                _ => unreachable!(),
            });
            if let Ok(command) = result {
                self.next_class = (class + 1) % 3;
                return Ok(command);
            }
        }
        Err(mpsc::error::TryRecvError::Empty)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.lifecycle
            .len()
            .saturating_add(self.gameplay.len())
            .saturating_add(self.internal.len())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn close(&mut self) {
        self.lifecycle.close();
        self.gameplay.close();
        self.internal.close();
    }

    #[cfg(test)]
    pub(crate) fn test_with_gameplay(gameplay: mpsc::Receiver<QueuedGameCommand>) -> Self {
        let (lifecycle_tx, lifecycle) = mpsc::channel(1);
        let (internal_tx, internal) = mpsc::channel(1);
        drop(lifecycle_tx);
        drop(internal_tx);
        Self {
            lifecycle,
            gameplay,
            internal,
            next_class: 0,
        }
    }

    #[cfg(test)]
    pub(crate) const fn test_with_ingress(
        lifecycle: mpsc::Receiver<QueuedGameCommand>,
        gameplay: mpsc::Receiver<QueuedGameCommand>,
        internal: mpsc::Receiver<QueuedGameCommand>,
    ) -> Self {
        Self {
            lifecycle,
            gameplay,
            internal,
            next_class: 0,
        }
    }
}

pub struct GameSchedule {
    pub name: String,
    pub activity: ScheduleActivity,
    pub schedule: RwLock<Schedule>,
    pub interval_ms: std::sync::atomic::AtomicU64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScheduleActivity {
    Always,
    OnlinePlayers,
    DueCrafting,
    DueGuns,
    DueProgrammator,
    DueHazards,
    ActiveGranular,
    ActiveAlive,
}

#[derive(Clone, Copy, Debug)]
pub struct BotSpotView {
    pub bot_id: i32,
    pub x: i32,
    pub y: i32,
    pub dir: i32,
    pub clan_id: i32,
}

/// Immutable attributes required for the legacy `HB/X` player packet.
/// This read model keeps periodic presentation outside the authoritative ECS lock.
#[derive(Clone, Copy, Debug)]
pub struct BotsRenderPlayer {
    pub x: i32,
    pub y: i32,
    pub dir: i32,
    pub skin: i32,
    pub clan_id: i32,
    pub tail: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BotsRenderDue {
    pub due_at: Instant,
    pub player_id: PlayerId,
    pub session_token: u64,
}

#[derive(Default)]
struct BotsRenderSchedule {
    due: BinaryHeap<Reverse<(Instant, PlayerId, u64)>>,
}

impl BotsRenderSchedule {
    fn schedule(&mut self, due: BotsRenderDue) {
        self.due
            .push(Reverse((due.due_at, due.player_id, due.session_token)));
    }

    fn pop_due(&mut self, now: Instant) -> Option<BotsRenderDue> {
        let Reverse((due_at, player_id, session_token)) = *self.due.peek()?;
        if due_at > now {
            return None;
        }
        self.due.pop();
        Some(BotsRenderDue {
            due_at,
            player_id,
            session_token,
        })
    }

    fn next_due_at(&self) -> Option<Instant> {
        self.due.peek().map(|Reverse((due_at, _, _))| *due_at)
    }
}

#[derive(Default)]
struct CraftingDueSchedule {
    due: BinaryHeap<Reverse<(i64, Entity)>>,
}

#[derive(Resource, Clone)]
pub struct ProgrammatorDueQueue(Arc<Mutex<ProgrammatorDueSchedule>>);

impl ProgrammatorDueQueue {
    pub fn schedule(&self, entity: Entity, due_at: Instant) {
        self.0.lock().schedule(entity, due_at);
    }
}

#[derive(Resource, Default)]
pub struct ProgrammatorDueBatch(pub Vec<(Entity, Instant)>);

#[derive(Resource, Clone)]
pub struct HazardDueQueue(Arc<Mutex<HazardDueSchedule>>);

impl HazardDueQueue {
    pub fn schedule(&self, entity: Entity, due_at: Instant) {
        self.0.lock().schedule(entity, due_at);
    }
}

#[derive(Resource, Default)]
pub struct HazardDueBatch(pub Vec<(Entity, Instant)>);

#[derive(Resource, Clone)]
pub struct StandingCellHazardContext {
    pub box_pickups: BoxPickupQueue,
    pub death_queue: combat::DeathQueue,
    pub due_queue: HazardDueQueue,
    pub interval: Duration,
    pub slow_threshold: Duration,
}

#[derive(Default)]
struct ProgrammatorDueSchedule {
    due: BinaryHeap<Reverse<(Instant, Entity)>>,
}

#[derive(Default)]
struct HazardDueSchedule {
    due: BinaryHeap<Reverse<(Instant, Entity)>>,
    scheduled: HashMap<Entity, Instant>,
}

impl HazardDueSchedule {
    fn schedule(&mut self, entity: Entity, due_at: Instant) {
        let Some(previous) = self.scheduled.insert(entity, due_at) else {
            self.due.push(Reverse((due_at, entity)));
            return;
        };
        if due_at < previous {
            self.due.push(Reverse((due_at, entity)));
        } else {
            self.scheduled.insert(entity, previous);
        }
    }

    fn discard_stale_head(&mut self) {
        while let Some(&Reverse((due_at, entity))) = self.due.peek() {
            if self
                .scheduled
                .get(&entity)
                .is_some_and(|current| *current == due_at)
            {
                break;
            }
            self.due.pop();
        }
    }

    fn next_due_at(&mut self) -> Option<Instant> {
        self.discard_stale_head();
        self.due.peek().map(|Reverse((due_at, _))| *due_at)
    }

    fn pop_due(&mut self, now: Instant, limit: usize) -> Vec<(Entity, Instant)> {
        let mut due = Vec::with_capacity(limit.min(self.scheduled.len()));
        while due.len() < limit {
            self.discard_stale_head();
            let Some(&Reverse((due_at, entity))) = self.due.peek() else {
                break;
            };
            if due_at > now {
                break;
            }
            self.due.pop();
            self.scheduled.remove(&entity);
            due.push((entity, due_at));
        }
        due
    }
}

#[cfg(test)]
mod hazard_due_schedule_tests {
    use super::*;

    #[test]
    fn keeps_one_earliest_deadline_per_entity() {
        let base = Instant::now();
        let entity = Entity::from_raw_u32(1).expect("non-placeholder entity id");
        let mut schedule = HazardDueSchedule::default();

        schedule.schedule(entity, base + Duration::from_millis(20));
        schedule.schedule(entity, base + Duration::from_millis(10));
        schedule.schedule(entity, base + Duration::from_millis(30));

        assert_eq!(
            schedule.next_due_at(),
            Some(base + Duration::from_millis(10))
        );
        assert_eq!(
            schedule.pop_due(base + Duration::from_millis(10), 1),
            vec![(entity, base + Duration::from_millis(10))]
        );
        assert!(schedule.next_due_at().is_none());
    }
}

impl ProgrammatorDueSchedule {
    fn schedule(&mut self, entity: Entity, due_at: Instant) {
        self.due.push(Reverse((due_at, entity)));
    }

    fn is_due(&self, now: Instant) -> bool {
        self.due
            .peek()
            .is_some_and(|Reverse((due_at, _))| *due_at <= now)
    }

    fn next_due_at(&self) -> Option<Instant> {
        self.due.peek().map(|Reverse((due_at, _))| *due_at)
    }

    fn pop_due(&mut self, now: Instant, limit: usize) -> Vec<(Entity, Instant)> {
        let mut due = Vec::with_capacity(limit.min(self.due.len()));
        while due.len() < limit {
            let Some(&Reverse((due_at, entity))) = self.due.peek() else {
                break;
            };
            if due_at > now {
                break;
            }
            self.due.pop();
            due.push((entity, due_at));
        }
        due
    }
}

impl CraftingDueSchedule {
    fn schedule(&mut self, entity: Entity, end_ts: i64) {
        if end_ts > 0 {
            self.due.push(Reverse((end_ts, entity)));
        }
    }

    fn is_due(&self, now_ts: i64) -> bool {
        self.due
            .peek()
            .is_some_and(|Reverse((end_ts, _))| *end_ts <= now_ts)
    }

    fn next_due_ts(&self) -> Option<i64> {
        self.due.peek().map(|Reverse((end_ts, _))| *end_ts)
    }

    fn pop_due(&mut self, now_ts: i64, limit: usize) -> Vec<building_damage::CraftingDue> {
        let mut due = Vec::with_capacity(limit.min(self.due.len()));
        while due.len() < limit {
            let Some(&Reverse((end_ts, entity))) = self.due.peek() else {
                break;
            };
            if end_ts > now_ts {
                break;
            }
            self.due.pop();
            due.push(building_damage::CraftingDue { entity, end_ts });
        }
        due
    }

    fn len(&self) -> usize {
        self.due.len()
    }
}

// ─── GameState ───────────────────────────────────────────────────────────────

pub struct GameState {
    pub world: Arc<World>,
    pub db: Arc<Database>,
    pub config: Config,
    active_players: DashMap<PlayerId, ActivePlayer>,
    player_entities: DashMap<PlayerId, Entity>,
    chunk_players: DashMap<ChunkPos, Vec<PlayerId>>,
    bots_render_players: DashMap<PlayerId, BotsRenderPlayer>,
    building_index: DashMap<WorldPos, Entity>,
    botspot_index: DashMap<PlayerId, Entity>,
    chunk_botspots: DashMap<ChunkPos, Vec<Entity>>,
    bots_render_botspots: DashMap<ChunkPos, Vec<BotSpotView>>,
    chunk_buildings: DashMap<ChunkPos, Vec<Entity>>,
    pub chat_channels: RwLock<Vec<chat::ChatChannel>>,
    /// Активные игровые ивенты (множители опыта, дропа и т.д.).
    /// Хранится в `GameState` (не в ECS), чтобы HTTP-API мог менять их
    /// без конкуренции с `ecs.write()` из сессий.
    pub active_events: RwLock<ActiveEvents>,
    pub ecs: RwLock<EcsWorld>,
    pub schedules: Vec<GameSchedule>,
    pub auth_failures: DashMap<std::net::IpAddr, (u32, Instant)>,
    commands_tx: CommandSenders,
    pub commands_rx: Mutex<Option<CommandReceivers>>,
    command_seq: std::sync::atomic::AtomicU64,
    command_queue_depth: std::sync::atomic::AtomicUsize,
    command_queue_high_water: std::sync::atomic::AtomicUsize,
    command_ingress_depth: [std::sync::atomic::AtomicUsize; 3],
    command_ingress_ages: [Mutex<VecDeque<Instant>>; 3],
    simulation_waker: crate::simulation_waker::SimulationWaker,
    crafting_due_schedule: Mutex<CraftingDueSchedule>,
    programmator_due_schedule: Arc<Mutex<ProgrammatorDueSchedule>>,
    hazard_due_schedule: Arc<Mutex<HazardDueSchedule>>,
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
}

impl GameState {
    pub const CHUNK_VIEW_RADIUS: i32 = 2;
    pub const BOTS_RENDER_INTERVAL: Duration = Duration::from_secs(4);
    pub const BOTS_RENDER_OBSERVER_BUDGET: usize = 32;
    pub const BOTS_RENDER_BYTE_BUDGET: usize = 1024 * 1024;
    pub const CRAFTING_DUE_BATCH_BUDGET: usize = 256;
    pub const PROGRAMMATOR_DUE_BATCH_BUDGET: usize = 256;
    pub const HAZARD_DUE_BATCH_BUDGET: usize = 256;

    pub fn next_chat_id(&self) -> i64 {
        self.chat_id_seq
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1
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
        let programmator_due_schedule = Arc::new(Mutex::new(ProgrammatorDueSchedule::default()));
        let hazard_due_schedule = Arc::new(Mutex::new(HazardDueSchedule::default()));
        let (lifecycle_tx, lifecycle_rx) = mpsc::channel(ingress.lifecycle_ingress_capacity);
        let (gameplay_tx, gameplay_rx) = mpsc::channel(ingress.gameplay_ingress_capacity);
        let (internal_tx, internal_rx) = mpsc::channel(ingress.internal_ingress_capacity);
        let max_chat_id = database.get_max_chat_id().await.unwrap_or(0);
        let state = Arc::new(Self {
            world,
            db: database,
            config,
            active_players: DashMap::new(),
            player_entities: DashMap::new(),
            chunk_players: DashMap::new(),
            bots_render_players: DashMap::new(),
            building_index: DashMap::new(),
            botspot_index: DashMap::new(),
            chunk_botspots: DashMap::new(),
            bots_render_botspots: DashMap::new(),
            chunk_buildings: DashMap::new(),
            chat_channels: RwLock::new(default_channels),
            active_events: RwLock::new(ActiveEvents::default()),
            ecs: RwLock::new(EcsWorld::new()),
            schedules,
            auth_failures: DashMap::new(),
            commands_tx: CommandSenders {
                lifecycle: lifecycle_tx,
                gameplay: gameplay_tx,
                internal: internal_tx,
            },
            commands_rx: Mutex::new(Some(CommandReceivers {
                lifecycle: lifecycle_rx,
                gameplay: gameplay_rx,
                internal: internal_rx,
                #[cfg(test)]
                next_class: 0,
            })),
            command_seq: std::sync::atomic::AtomicU64::new(1),
            command_queue_depth: std::sync::atomic::AtomicUsize::new(0),
            command_queue_high_water: std::sync::atomic::AtomicUsize::new(0),
            command_ingress_depth: std::array::from_fn(|_| std::sync::atomic::AtomicUsize::new(0)),
            command_ingress_ages: std::array::from_fn(|_| Mutex::new(VecDeque::new())),
            simulation_waker: crate::simulation_waker::SimulationWaker::default(),
            crafting_due_schedule: Mutex::new(CraftingDueSchedule::default()),
            programmator_due_schedule,
            hazard_due_schedule,
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
            ecs.insert_resource(ProgrammatorDueQueue(
                state.programmator_due_schedule.clone(),
            ));
            ecs.insert_resource(ProgrammatorDueBatch::default());
            ecs.insert_resource(HazardDueBatch::default());
            ecs.insert_resource(StandingCellHazardContext {
                box_pickups: state.box_pickup_queue.clone(),
                death_queue: state.death_queue.clone(),
                due_queue: HazardDueQueue(state.hazard_due_schedule.clone()),
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
        self.player_entities.get(&pid).map(|p| *p)
    }

    /// Выдать новый токен сеанса (монотонный, уникальный на процесс).
    pub fn schedule_crafting_completion(&self, entity: Entity, end_ts: i64) {
        let mut schedule = self.crafting_due_schedule.lock();
        schedule.schedule(entity, end_ts);
        let depth = schedule.len();
        drop(schedule);
        crate::metrics::CRAFTING_DUE_DEPTH.set(i64::try_from(depth).unwrap_or(i64::MAX));
        self.simulation_waker.wake();
    }

    pub fn next_crafting_due_ts(&self) -> Option<i64> {
        self.crafting_due_schedule.lock().next_due_ts()
    }

    pub fn has_due_crafting(&self, now_ts: i64) -> bool {
        self.crafting_due_schedule.lock().is_due(now_ts)
    }

    pub fn schedule_programmator(&self, entity: Entity, due_at: Instant) {
        self.programmator_due_schedule
            .lock()
            .schedule(entity, due_at);
        self.simulation_waker.wake();
    }

    pub fn next_programmator_due_at(&self) -> Option<Instant> {
        self.programmator_due_schedule.lock().next_due_at()
    }

    pub fn has_due_programmator(&self, now: Instant) -> bool {
        self.programmator_due_schedule.lock().is_due(now)
    }

    pub fn take_due_programmators(&self, now: Instant) -> Vec<(Entity, Instant)> {
        self.programmator_due_schedule
            .lock()
            .pop_due(now, Self::PROGRAMMATOR_DUE_BATCH_BUDGET)
    }

    pub fn schedule_hazard(&self, entity: Entity, due_at: Instant) {
        self.hazard_due_schedule.lock().schedule(entity, due_at);
        self.simulation_waker.wake();
    }

    pub fn next_hazard_due_at(&self) -> Option<Instant> {
        self.hazard_due_schedule.lock().next_due_at()
    }

    pub fn take_due_hazards(&self, now: Instant) -> Vec<(Entity, Instant)> {
        self.hazard_due_schedule
            .lock()
            .pop_due(now, Self::HAZARD_DUE_BATCH_BUDGET)
    }

    pub fn take_due_crafting(
        &self,
        now_ts: i64,
    ) -> (Vec<building_damage::CraftingDue>, bool, usize) {
        let mut schedule = self.crafting_due_schedule.lock();
        let due = schedule.pop_due(now_ts, Self::CRAFTING_DUE_BATCH_BUDGET);
        let due_remaining = schedule.is_due(now_ts);
        let depth = schedule.len();
        drop(schedule);
        (due, due_remaining, depth)
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
        use std::sync::atomic::Ordering;

        debug_assert_eq!(command.ingress_class(), CommandIngressClass::Lifecycle);
        let kind = command.name();
        let received_at = Instant::now();
        let Ok(permit) = self.commands_tx.lifecycle.reserve().await else {
            crate::metrics::COMMANDS_TOTAL
                .with_label_values(&[kind, "ingress_closed"])
                .inc();
            return false;
        };
        let enqueued_at = Instant::now();
        let sequence = self.allocate_command_sequence();
        let class = CommandIngressClass::Lifecycle;
        let queued = QueuedGameCommand {
            player_id,
            session_id,
            ingress_class: Some(class),
            sequence,
            received_at,
            enqueued_at,
            command: GameCommand::Player(command),
        };
        let depth = self
            .command_queue_depth
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let class_depth = self.command_ingress_depth[class.index()]
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let high_water = self
            .command_queue_high_water
            .fetch_max(depth, Ordering::Relaxed)
            .max(depth);
        crate::metrics::COMMANDS_TOTAL
            .with_label_values(&[kind, "enqueued"])
            .inc();
        crate::metrics::COMMAND_QUEUE_DEPTH.set(i64::try_from(depth).unwrap_or(i64::MAX));
        crate::metrics::COMMAND_QUEUE_HIGH_WATER.set(i64::try_from(high_water).unwrap_or(i64::MAX));
        crate::metrics::COMMAND_INGRESS_DEPTH
            .with_label_values(&[class.metric_name()])
            .set(i64::try_from(class_depth).unwrap_or(i64::MAX));
        self.push_command_ingress_age(class, enqueued_at);
        permit.send(queued);
        self.simulation_waker.wake();
        true
    }

    pub fn enqueue_command(
        &self,
        player_id: PlayerId,
        session_id: SessionId,
        command: GameCommand,
    ) -> bool {
        self.enqueue_command_received(player_id, session_id, command, Instant::now())
    }

    pub async fn enqueue_internal(
        &self,
        player_id: PlayerId,
        session_id: SessionId,
        command: PlayerCommand,
    ) -> bool {
        use std::sync::atomic::Ordering;

        debug_assert_eq!(command.ingress_class(), CommandIngressClass::Internal);
        let kind = command.name();
        let received_at = Instant::now();
        let Ok(permit) = self.commands_tx.internal.reserve().await else {
            crate::metrics::COMMANDS_TOTAL
                .with_label_values(&[kind, "ingress_closed"])
                .inc();
            return false;
        };
        let enqueued_at = Instant::now();
        let sequence = self.allocate_command_sequence();
        let class = CommandIngressClass::Internal;
        let queued = QueuedGameCommand {
            player_id,
            session_id,
            ingress_class: Some(class),
            sequence,
            received_at,
            enqueued_at,
            command: GameCommand::Player(command),
        };
        let depth = self
            .command_queue_depth
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let class_depth = self.command_ingress_depth[class.index()]
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let high_water = self
            .command_queue_high_water
            .fetch_max(depth, Ordering::Relaxed)
            .max(depth);
        crate::metrics::COMMANDS_TOTAL
            .with_label_values(&[kind, "enqueued"])
            .inc();
        crate::metrics::COMMAND_QUEUE_DEPTH.set(i64::try_from(depth).unwrap_or(i64::MAX));
        crate::metrics::COMMAND_QUEUE_HIGH_WATER.set(i64::try_from(high_water).unwrap_or(i64::MAX));
        crate::metrics::COMMAND_INGRESS_DEPTH
            .with_label_values(&[class.metric_name()])
            .set(i64::try_from(class_depth).unwrap_or(i64::MAX));
        self.push_command_ingress_age(class, enqueued_at);
        permit.send(queued);
        self.simulation_waker.wake();
        true
    }

    pub fn enqueue_command_received(
        &self,
        player_id: PlayerId,
        session_id: SessionId,
        command: GameCommand,
        received_at: Instant,
    ) -> bool {
        use std::sync::atomic::Ordering;

        let GameCommand::Player(action) = &command;
        let (kind, class) = (action.name(), action.ingress_class());
        assert_ne!(
            class,
            CommandIngressClass::Internal,
            "internal follow-up must use awaitable GameState::enqueue_internal"
        );
        let enqueued_at = Instant::now();
        let sequence = self.allocate_command_sequence();
        let queued = QueuedGameCommand {
            player_id,
            session_id,
            ingress_class: Some(class),
            sequence,
            received_at,
            enqueued_at,
            command,
        };
        crate::metrics::COMMAND_RECEIVE_TO_ENQUEUE_SECONDS
            .with_label_values(&[kind])
            .observe(
                enqueued_at
                    .saturating_duration_since(received_at)
                    .as_secs_f64(),
            );
        let sender = match class {
            CommandIngressClass::Lifecycle => &self.commands_tx.lifecycle,
            CommandIngressClass::Gameplay => &self.commands_tx.gameplay,
            CommandIngressClass::Internal => &self.commands_tx.internal,
        };
        let Ok(permit) = sender.try_reserve() else {
            crate::metrics::COMMANDS_TOTAL
                .with_label_values(&[kind, "ingress_rejected"])
                .inc();
            crate::metrics::COMMANDS_TOTAL
                .with_label_values(&[class.metric_name(), "ingress_rejected"])
                .inc();
            return false;
        };
        let depth = self
            .command_queue_depth
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let class_depth = self.command_ingress_depth[class.index()]
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let high_water = self
            .command_queue_high_water
            .fetch_max(depth, Ordering::Relaxed)
            .max(depth);
        crate::metrics::COMMANDS_TOTAL
            .with_label_values(&[kind, "enqueued"])
            .inc();
        crate::metrics::COMMAND_QUEUE_DEPTH.set(i64::try_from(depth).unwrap_or(i64::MAX));
        crate::metrics::COMMAND_QUEUE_HIGH_WATER.set(i64::try_from(high_water).unwrap_or(i64::MAX));
        crate::metrics::COMMAND_INGRESS_DEPTH
            .with_label_values(&[class.metric_name()])
            .set(i64::try_from(class_depth).unwrap_or(i64::MAX));
        self.push_command_ingress_age(class, enqueued_at);
        permit.send(queued);
        self.simulation_waker.wake();
        true
    }

    fn push_command_ingress_age(&self, class: CommandIngressClass, enqueued_at: Instant) {
        let mut ages = self.command_ingress_ages[class.index()].lock();
        ages.push_back(enqueued_at);
        Self::record_oldest_command_ingress_age(class, ages.front().copied());
    }

    fn pop_command_ingress_age(&self, class: CommandIngressClass) {
        let mut ages = self.command_ingress_ages[class.index()].lock();
        assert!(ages.pop_front().is_some(), "command ingress age underflow");
        Self::record_oldest_command_ingress_age(class, ages.front().copied());
    }

    pub(crate) fn refresh_command_ingress_oldest_ages(&self) {
        for class in [
            CommandIngressClass::Lifecycle,
            CommandIngressClass::Gameplay,
            CommandIngressClass::Internal,
        ] {
            let ages = self.command_ingress_ages[class.index()].lock();
            Self::record_oldest_command_ingress_age(class, ages.front().copied());
        }
    }

    fn record_oldest_command_ingress_age(class: CommandIngressClass, oldest: Option<Instant>) {
        let age = oldest.map_or(Duration::ZERO, |timestamp| timestamp.elapsed());
        crate::metrics::COMMAND_INGRESS_OLDEST_AGE_SECONDS
            .with_label_values(&[class.metric_name()])
            .set(age.as_secs_f64());
    }

    pub(crate) fn allocate_command_sequence(&self) -> CommandSeq {
        CommandSeq::new(
            self.command_seq
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        )
    }

    pub(crate) fn simulation_waker(&self) -> crate::simulation_waker::SimulationWaker {
        self.simulation_waker.clone()
    }

    pub fn record_command_dequeued(&self, class: CommandIngressClass) {
        use std::sync::atomic::Ordering;

        let previous = self.command_queue_depth.fetch_sub(1, Ordering::Relaxed);
        debug_assert!(previous > 0, "command queue depth underflow");
        let depth = previous.saturating_sub(1);
        crate::metrics::COMMAND_QUEUE_DEPTH.set(i64::try_from(depth).unwrap_or(i64::MAX));
        let previous_class =
            self.command_ingress_depth[class.index()].fetch_sub(1, Ordering::Relaxed);
        debug_assert!(previous_class > 0, "command ingress depth underflow");
        crate::metrics::COMMAND_INGRESS_DEPTH
            .with_label_values(&[class.metric_name()])
            .set(i64::try_from(previous_class.saturating_sub(1)).unwrap_or(i64::MAX));
        self.pop_command_ingress_age(class);
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
        self.refresh_bots_render_player_in_ecs(pid, entity, &ecs);
        drop(ecs);
        Some(res)
    }

    fn refresh_bots_render_player_in_ecs(&self, pid: PlayerId, entity: Entity, ecs: &EcsWorld) {
        let Some(position) = ecs.get::<PlayerPosition>(entity) else {
            self.bots_render_players.remove(&pid);
            return;
        };
        let Some(stats) = ecs.get::<PlayerStats>(entity) else {
            self.bots_render_players.remove(&pid);
            return;
        };
        let tail = ecs
            .get::<programmator::ProgrammatorState>(entity)
            .map_or(0, |program| u8::from(program.running));
        self.bots_render_players.insert(
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
        self.bots_render_players.get(&pid).map(|entry| *entry)
    }

    pub fn bots_render_botspots_in_chunk(&self, cx: u32, cy: u32) -> Vec<BotSpotView> {
        self.bots_render_botspots
            .get(&(cx, cy).into())
            .map(|spots| spots.clone())
            .unwrap_or_default()
    }

    pub fn take_dirty_player_entities(&self) -> Vec<(Entity, crate::game::SessionId)> {
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

    pub fn snapshot_dirty_player(
        &self,
        entity: Entity,
        incarnation: crate::game::SessionId,
    ) -> Option<crate::db::PlayerRow> {
        let mut ecs = self.ecs_write_profiled("game.snapshot_dirty_player");
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
        let row = crate::game::player::extract_player_row(&ecs, entity)?;
        ecs.get_mut::<PlayerFlags>(entity)?.dirty = false;
        drop(ecs);
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
            let ecs = self.ecs.read();
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

    pub fn building_entities_in_chunk_snapshot(&self, cx: u32, cy: u32) -> Vec<Entity> {
        self.chunk_buildings
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
        Self::find_pack_covering_with(&ecs, &self.chunk_buildings, x, y)
    }

    pub fn find_pack_covering_in_ecs(&self, ecs: &EcsWorld, x: i32, y: i32) -> Option<(i32, i32)> {
        Self::find_pack_covering_with(ecs, &self.chunk_buildings, x, y)
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
        Self::access_gun_with(&ecs, &self.chunk_buildings, x, y, player_clan_id)
    }

    pub fn access_gun_full_in_ecs(
        &self,
        ecs: &EcsWorld,
        x: i32,
        y: i32,
        player_clan_id: i32,
    ) -> (bool, bool) {
        Self::access_gun_with(ecs, &self.chunk_buildings, x, y, player_clan_id)
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
        if let Some(entities) = self.chunk_buildings.get(&(cx, cy).into()) {
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
            if let Some(entities) = self.chunk_buildings.get(&(ucx, ucy).into()) {
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
        self.active_players
            .iter()
            .map(|entry| *entry.key())
            .collect()
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
        self.active_players
            .get(&pid)
            .map(|active| active.session_id)
    }

    pub fn nearby_session_ids(
        &self,
        cx: u32,
        cy: u32,
        exclude_id: Option<PlayerId>,
    ) -> Vec<SessionId> {
        let mut player_ids = Vec::new();
        for (ncx, ncy) in self.visible_chunks_iter(cx, cy) {
            if let Some(players) = self.chunk_players.get(&(ncx, ncy).into()) {
                player_ids.extend(players.iter().copied());
            }
        }
        player_ids
            .into_iter()
            .filter(|player_id| Some(*player_id) != exclude_id)
            .filter_map(|player_id| {
                self.active_players
                    .get(&player_id)
                    .map(|active| active.session_id)
            })
            .collect()
    }

    pub fn session_ids_in_chunk(
        &self,
        cx: u32,
        cy: u32,
        exclude_id: Option<PlayerId>,
    ) -> Vec<SessionId> {
        let Some(players) = self.chunk_players.get(&(cx, cy).into()) else {
            return Vec::new();
        };
        players
            .iter()
            .copied()
            .filter(|player_id| Some(*player_id) != exclude_id)
            .filter_map(|player_id| {
                self.active_players
                    .get(&player_id)
                    .map(|active| active.session_id)
            })
            .collect()
    }

    pub fn is_player_active(&self, pid: PlayerId) -> bool {
        self.active_players.contains_key(&pid)
    }

    pub fn online_count(&self) -> usize {
        self.active_players.len()
    }

    pub fn register_active_player(&self, pid: PlayerId, entity: Entity, session_id: SessionId) {
        self.active_players.insert(
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
        self.bots_render_players.remove(&pid);
        self.active_players.remove(&pid).map(|(_, active)| active)
    }

    pub fn active_player_entity_for_session(
        &self,
        pid: PlayerId,
        session_id: SessionId,
    ) -> Option<Entity> {
        self.active_players
            .get(&pid)
            .filter(|active| active.session_id == session_id)
            .map(|active| active.ecs_entity)
    }

    pub fn player_entity_ids(&self) -> Vec<PlayerId> {
        self.player_entities
            .iter()
            .map(|entry| *entry.key())
            .collect()
    }

    pub fn player_entity_count(&self) -> usize {
        self.player_entities.len()
    }

    pub fn register_player_entity(&self, pid: PlayerId, entity: Entity) {
        self.player_entities.insert(pid, entity);
    }

    pub fn unregister_player_entity(&self, pid: PlayerId) {
        self.player_entities.remove(&pid);
    }

    pub fn take_due_bots_render(&self, now: Instant, limit: usize) -> Vec<BotsRenderDue> {
        let mut due = Vec::with_capacity(limit);
        while due.len() < limit {
            let Some(candidate) = self.bots_render_schedule.lock().pop_due(now) else {
                break;
            };
            if self
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

    pub fn has_alive_work(&self) -> bool {
        self.alive_work_q.has_work()
    }

    pub fn broadcast_cell_update(&self, x: i32, y: i32) {
        use crate::protocol::packets::hb_cell;
        self.wake_granular_neighborhood(x, y);
        let Some(cell) = self.world.read_world_cell(x, y) else {
            return;
        };
        self.alive_work_q.note_cell(x, y, cell.cell_type);
        let sub = hb_cell(
            u16::try_from(x.rem_euclid(65536)).unwrap_or(0),
            u16::try_from(y.rem_euclid(65536)).unwrap_or(0),
            cell.cell_type.0,
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

    fn remove_building_entity_if(&self, x: i32, y: i32, expected: Entity) -> Option<Entity> {
        let (_, entity) = self
            .building_index
            .remove_if(&((x, y).into()), |_, entity| *entity == expected)?;
        let (cx, cy) = World::chunk_pos(x, y);
        if let Some(mut entities) = self.chunk_buildings.get_mut(&(cx, cy).into()) {
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
            self.remove_botspot_runtime(view.owner_id);
        }
        self.ecs_write_profiled("game.remove_building_runtime")
            .despawn(entity);
        Some(self.clear_building_footprint_authoritative(view))
    }

    /// Runtime removal `BotSpot`, связанного со Spot-зданием.
    pub fn remove_botspot_runtime(&self, owner_id: PlayerId) -> Option<Entity> {
        let (_, entity) = self.botspot_index.remove(&owner_id)?;
        self.chunk_botspots
            .iter_mut()
            .for_each(|mut e| e.value_mut().retain(|&ent| ent != entity));
        self.bots_render_botspots.iter_mut().for_each(|mut spots| {
            spots
                .value_mut()
                .retain(|spot| spot.bot_id != -i32::from(owner_id));
        });
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
        self.botspot_index.insert(owner_id, entity);
        let (cx, cy) = World::chunk_pos(x, y);
        self.chunk_botspots
            .entry((cx, cy).into())
            .or_default()
            .push(entity);
        self.bots_render_botspots
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
        self.chunk_players
            .get(&(cx, cy).into())
            .map(|players| players.clone())
            .unwrap_or_default()
    }

    pub fn register_player_chunk(&self, pid: PlayerId, cx: u32, cy: u32) {
        let mut players = self.chunk_players.entry((cx, cy).into()).or_default();
        if !players.contains(&pid) {
            players.push(pid);
        }
    }

    pub fn unregister_player_from_chunk(&self, pid: PlayerId, cx: u32, cy: u32) {
        if let Some(mut players) = self.chunk_players.get_mut(&(cx, cy).into()) {
            players.retain(|&id| id != pid);
        }
    }

    #[allow(dead_code)]
    pub fn unregister_player_from_all_chunks(&self, pid: PlayerId) {
        self.chunk_players
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
            if let Some(players) = self.chunk_players.get(&(ncx, ncy).into()) {
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
