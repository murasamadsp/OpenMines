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
    pub last_move: Instant,
    pub last_dig: Instant,
    /// Как `Player.TryAct(..., 200)` для `Xgeo` в референсе.
    pub last_geo: Instant,
    pub protection_until: Option<Instant>,
    pub last_shot: Option<Instant>,
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
        skills: skills.states.clone(),
        role: stats.role,
        clan_rank: stats.clan_rank,
    })
}
