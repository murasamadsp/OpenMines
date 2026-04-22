use crate::db::{PlayerRow, SkillState};
use bevy_ecs::prelude::Component;
use std::collections::HashMap;
use std::time::Instant;
use tokio::sync::mpsc;

pub type PlayerId = i32;

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
    /// Дробный аккумулятор кристаллов (как `cb` в C# `Player.Mine()`).
    pub crystal_carry: f32,
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
pub struct PlayerSkills {
    pub states: HashMap<String, SkillState>,
    /// Number of skill slots the player has (C# `PlayerSkills.slots`).
    /// Default is 20, max is 34. Purchased via creds in Up building.
    #[allow(dead_code)]
    pub total_slots: i32,
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
    // TODO: last_move will be used when movement rate-limiting is fully wired
    #[allow(dead_code)]
    pub last_move: Instant,
    pub last_dig: Instant,
    /// Как `Player.TryAct(..., 200)` для `Xbld` в референсе.
    pub last_build: Instant,
    /// Как `Player.TryAct(..., 200)` для `Xgeo` в референсе.
    pub last_geo: Instant,
    pub protection_until: Option<Instant>,
    // TODO: last_shot will be used when gun cooldown tracking is fully wired
    #[allow(dead_code)]
    pub last_shot: Option<Instant>,
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

#[derive(Component)]
pub struct PlayerSettings {
    pub auto_dig: bool,
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

impl Clone for PlayerSettings {
    fn clone(&self) -> Self {
        *self
    }
}

impl Copy for PlayerSettings {}

impl Default for PlayerSettings {
    fn default() -> Self {
        Self {
            auto_dig: false,
            cc: 10, snd: false, mus: false, isca: 0, tsca: 0,
            mous: true, pot: false, frc: true, ctrl: true, mof: true,
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

pub struct ActivePlayer {
    pub ecs_entity: bevy_ecs::entity::Entity,
}

impl PlayerPosition {
    pub fn chunk_x(&self) -> u32 {
        self.x.max(0) as u32 / 32
    }

    pub fn chunk_y(&self) -> u32 {
        self.y.max(0) as u32 / 32
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
    let skills = ecs.get::<PlayerSkills>(entity)?;
    let settings = ecs.get::<PlayerSettings>(entity)?;

    Some(PlayerRow {
        id: meta.id,
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
        crystals: stats.crystals,
        clan_id: stats.clan_id,
        resp_x: meta.resp_x,
        resp_y: meta.resp_y,
        inventory: inv.items.clone(),
        skills: {
            let mut s = skills.states.clone();
            // Persist total_slots in JSON under special key for DB storage
            if skills.total_slots != 20 {
                s.insert(
                    "__slots".to_string(),
                    crate::db::SkillState {
                        level: skills.total_slots,
                        exp: 0.0,
                    },
                );
            }
            s
        },
        role: stats.role,
        clan_rank: stats.clan_rank,
    })
}
