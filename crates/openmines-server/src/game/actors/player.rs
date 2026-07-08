use crate::db::{PlayerRow, SkillSlots};
use bevy_ecs::prelude::Component;
use std::collections::HashMap;
use std::fmt;
use std::time::Instant;
use tokio::sync::mpsc;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct PlayerId(pub i32);

impl From<i32> for PlayerId {
    fn from(value: i32) -> Self {
        Self(value)
    }
}

impl From<PlayerId> for i32 {
    fn from(value: PlayerId) -> Self {
        value.0
    }
}

impl std::borrow::Borrow<i32> for PlayerId {
    fn borrow(&self) -> &i32 {
        &self.0
    }
}

impl PlayerId {
    #[must_use]
    pub const fn as_i32(self) -> i32 {
        self.0
    }
}

impl fmt::Display for PlayerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::str::FromStr for PlayerId {
    type Err = <i32 as std::str::FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(Self)
    }
}

impl std::ops::Neg for PlayerId {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self(-self.0)
    }
}

impl PartialEq<i32> for PlayerId {
    fn eq(&self, other: &i32) -> bool {
        self.0 == *other
    }
}

impl PartialEq<PlayerId> for i32 {
    fn eq(&self, other: &PlayerId) -> bool {
        *self == other.0
    }
}

#[derive(Component)]
pub struct PlayerPosition {
    pub x: i32,
    pub y: i32,
    pub dir: i32,
}

#[derive(Component)]
pub struct PlayerStats {
    pub health: i32,
    pub max_health: i32,
    pub money: i64,
    pub creds: i64,
    pub crystals: [i64; 6],
    pub role: i32,
    pub skin: i32,
    pub clan_id: Option<i32>,
    pub clan_rank: i32,
    /// Время последнего клейма ежедневного бонуса (`GDon`), unix-секунды; 0 = ни разу.
    pub last_bonus_at: i64,
}

#[derive(Component, Clone)]
pub struct PlayerInventory {
    pub items: HashMap<i32, i32>,
    pub selected: i32,
    /// Как `Inventory.minv`: `true` — сетка из `miniq` (до 4 слотов), `false` — полный инвентарь.
    pub minv: bool,
    /// Очередь выбранных слотов для мини-режима (`Inventory.miniq` в C#).
    pub miniq: Vec<i32>,
}

#[derive(Component, Clone)]
pub struct PlayerSkillsComp {
    /// Слотовая модель скиллов (1:1 C# `PlayerSkills`): slot→skill + slots + selectedslot.
    pub states: SkillSlots,
}

#[derive(Component)]
pub struct PlayerView {
    pub last_chunk: Option<(u32, u32)>,
    pub visible_chunks: Vec<(u32, u32)>,
}

#[derive(Component)]
pub struct PlayerUI {
    pub current_window: Option<String>,
    pub current_chat: String,
}

#[derive(Component)]
pub struct PlayerCooldowns {
    pub last_dig: Instant,
    /// Как `Player.TryAct(..., 200)` для `Xbld` в референсе.
    pub last_build: Instant,
    /// Как `Player.TryAct(..., 200)` для `Xgeo` в референсе.
    pub last_geo: Instant,
    /// C# `Inventory.time` — гейт 400ms между использованиями предметов (INUS).
    pub last_inventory_use: Instant,
    pub protection_until: Option<Instant>,
    /// C190 stacking damage: each hit within a short window increments stacks.
    pub c190_stacks: i32,
    pub last_c190_hit: Option<Instant>,
}

/// Клетки, поднятые гео-киркой (`PEntity.geo` в C#). Верх стека — последний элемент.
#[derive(Component, Default)]
pub struct PlayerGeoStack(pub Vec<u8>);

#[derive(Component)]
pub struct PlayerMetadata {
    pub id: PlayerId,
    pub name: String,
    pub passwd: String,
    pub hash: String,
    pub resp_x: Option<i32>,
    pub resp_y: Option<i32>,
}

// 1:1 ref Settings.cs: поля настроек мапятся 1:1 на клиентский протокол
// (cc/snd/mus/isca/tsca/mous/pot/frc/ctrl/mof). Битпакинг/группировка
// сломали бы это соответствие — каждое поле адресуется по имени в
// settings GUI и sync. Та же конвенция точечного allow, что skills.rs.
#[allow(clippy::struct_excessive_bools)]
#[derive(Component, Clone, Copy)]
pub struct PlayerSettings {
    pub auto_dig: bool,
    pub aggression: bool,
    // C# ref Settings.cs fields:
    pub cc: i32,
    pub snd: bool,
    pub mus: bool,
    pub isca: i32,
    pub tsca: i32,
    pub mous: bool,
    pub pot: bool,
    pub frc: bool,
    pub ctrl: bool,
    pub mof: bool,
}

impl Default for PlayerSettings {
    fn default() -> Self {
        Self {
            auto_dig: false,
            aggression: false,
            cc: 10,
            snd: false,
            mus: false,
            isca: 0,
            tsca: 0,
            mous: true,
            pot: false,
            frc: true,
            ctrl: true,
            mof: true,
        }
    }
}

#[derive(Component)]
pub struct PlayerFlags {
    pub dirty: bool,
}

#[derive(Component)]
pub struct PlayerConnection {
    pub tx: mpsc::UnboundedSender<Vec<u8>>,
}

impl PlayerConnection {
    pub fn send_or_log(&self, data: Vec<u8>) {
        if let Err(err) = self.tx.send(data) {
            tracing::debug!("Player connection channel closed during send: {:?}", err);
        }
    }
}

pub struct ActivePlayer {
    pub ecs_entity: bevy_ecs::entity::Entity,
    /// Токен сеанса — идентифицирует конкретное подключение. Guard от
    /// reconnect-гонки: отложенный `Disconnect` старого сеанса сносит entity
    /// только если токен в `active_players` всё ещё его (иначе уже переподключился).
    pub session_token: u64,
    /// Время последнего периодического `BotsRender` (1:1 C# `lBotsUpdate`).
    /// Каждые 4с в game-tick игроку заново шлются `X` всех видимых ботов —
    /// иначе клиентский `RobotsGarbageCollector` (6с без пинга) удаляет
    /// простаивающих ботов (мигают при ходьбе, исчезают в покое).
    pub last_bots_render: std::time::Instant,
}

impl PlayerPosition {
    pub fn chunk_x(&self) -> u32 {
        self.x.max(0).cast_unsigned() / 32
    }

    pub fn chunk_y(&self) -> u32 {
        self.y.max(0).cast_unsigned() / 32
    }
}

pub fn extract_player_row(
    ecs: &bevy_ecs::prelude::World,
    entity: bevy_ecs::entity::Entity,
) -> Option<PlayerRow> {
    let pos = ecs.get::<PlayerPosition>(entity)?;
    let stats = ecs.get::<PlayerStats>(entity)?;
    let meta = ecs.get::<PlayerMetadata>(entity)?;
    let inv = ecs.get::<PlayerInventory>(entity)?;
    let skills = ecs.get::<PlayerSkillsComp>(entity)?;
    let settings = ecs.get::<PlayerSettings>(entity)?;
    let prog = ecs.get::<crate::game::programmator::ProgrammatorState>(entity);
    let programmator_running = prog.is_some_and(|p| p.running);
    let programmator_snapshot = prog.and_then(|p| serde_json::to_string(&p.snapshot()).ok());

    Some(PlayerRow {
        id: meta.id.into(),
        name: meta.name.clone(),
        passwd: meta.passwd.clone(),
        hash: meta.hash.clone(),
        x: pos.x,
        y: pos.y,
        dir: pos.dir,
        health: stats.health,
        max_health: stats.max_health,
        money: stats.money,
        creds: stats.creds,
        skin: stats.skin,
        auto_dig: settings.auto_dig,
        aggression: settings.aggression,
        crystals: stats.crystals,
        clan_id: stats.clan_id,
        resp_x: meta.resp_x,
        resp_y: meta.resp_y,
        inventory: inv.items.clone(),
        // Слотовая модель сериализуется целиком (skills + total_slots); __slots-хак удалён.
        skills: skills.states.clone(),
        role: stats.role,
        selected_program_id: prog.and_then(|p| p.selected_id),
        selected_program: None,
        programmator_running,
        programmator_snapshot,
        clan_rank: stats.clan_rank,
        last_bonus_at: stats.last_bonus_at,
    })
}
