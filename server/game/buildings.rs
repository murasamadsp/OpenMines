use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::sync::OnceLock;
use bevy_ecs::prelude::{Component, Entity};
use crate::game::player::PlayerId;
use crate::db::buildings::BuildingRow;

#[derive(Debug, Clone, Deserialize)]
pub struct BuildingCellConfig {
    pub dx: i32,
    pub dy: i32,
    #[serde(rename = "type")]
    pub cell_type: u8,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BuildingConfig {
    #[allow(dead_code)]
    pub code: String,
    pub cost: i64,
    pub charge: f32,
    pub max_charge: f32,
    pub hp: i32,
    pub max_hp: i32,
    pub cells: Vec<BuildingCellConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BuildingsConfig {
    pub buildings: HashMap<String, BuildingConfig>,
}

static BUILDINGS_CONFIG: OnceLock<BuildingsConfig> = OnceLock::new();

pub fn load_buildings_config(path: &str) -> anyhow::Result<()> {
    let data = fs::read_to_string(path)?;
    let config: BuildingsConfig = serde_json::from_str(&data)?;
    BUILDINGS_CONFIG
        .set(config)
        .map_err(|_| anyhow::anyhow!("Buildings config already loaded"))?;
    Ok(())
}

pub fn get_building_config(pack_type: PackType) -> Option<&'static BuildingConfig> {
    let cfg = BUILDINGS_CONFIG.get()?;
    let key = match pack_type {
        PackType::Teleport => "Teleport",
        PackType::Resp => "Resp",
        PackType::Gun => "Gun",
        PackType::Market => "Market",
        PackType::Up => "Up",
        PackType::Storage => "Storage",
        PackType::Craft => "Craft",
        PackType::Spot => "Spot",
        PackType::Gate => "Gate",
    };
    cfg.buildings.get(key)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PackType {
    Teleport, // 'T'
    Resp,     // 'R'
    Gun,      // 'G'
    Market,   // 'M'
    Up,       // 'U'
    Storage,  // 'L'
    Craft,    // 'F'
    Spot,     // 'O'
    Gate,     // 'N'
}

impl PackType {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Teleport => "Teleport",
            Self::Resp => "Resp",
            Self::Gun => "Gun",
            Self::Market => "Market",
            Self::Up => "Up",
            Self::Storage => "Storage",
            Self::Craft => "Craft",
            Self::Spot => "Spot",
            Self::Gate => "Gate",
        }
    }

    pub const fn code(self) -> u8 {
        match self {
            Self::Teleport => b'T',
            Self::Resp => b'R',
            Self::Gun => b'G',
            Self::Market => b'M',
            Self::Up => b'U',
            Self::Storage => b'L',
            Self::Craft => b'F',
            Self::Spot => b'O',
            Self::Gate => b'N',
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "T" | "Teleport" => Some(Self::Teleport),
            "R" | "Resp" => Some(Self::Resp),
            "G" | "Gun" => Some(Self::Gun),
            "M" | "Market" => Some(Self::Market),
            "U" | "Up" => Some(Self::Up),
            "L" | "Storage" => Some(Self::Storage),
            "F" | "Craft" => Some(Self::Craft),
            "O" | "Spot" => Some(Self::Spot),
            "N" | "Gate" => Some(Self::Gate),
            _ => None,
        }
    }

    pub fn building_cells(self) -> Vec<(i32, i32, u8)> {
        get_building_config(self).map_or_else(Vec::new, |cfg| {
            cfg.cells
                .iter()
                .map(|c| (c.dx, c.dy, c.cell_type))
                .collect()
        })
    }
}

// ─── ECS Components ───────────────────────────────────────────────────

#[derive(Component)]
pub struct BuildingMetadata {
    pub id: i32,
    pub pack_type: PackType,
}

#[derive(Component)]
pub struct BuildingStats {
    pub charge: f32,
    pub max_charge: f32,
    pub cost: i32,
    pub hp: i32,
    pub max_hp: i32,
}

#[derive(Component)]
pub struct BuildingStorage {
    pub money: i64,
    pub crystals: [i64; 6],
    pub items: HashMap<i32, i32>,
}

#[derive(Component)]
pub struct BuildingCrafting {
    pub recipe_id: Option<i32>,
    pub num: i32,
    pub end_ts: i64,
}

#[derive(Component)]
pub struct BuildingOwnership {
    pub owner_id: PlayerId,
    pub clan_id: i32,
}

#[derive(Component)]
pub struct GridPosition {
    pub x: i32,
    pub y: i32,
}

#[derive(Component)]
pub struct BuildingFlags {
    pub dirty: bool,
}

pub fn extract_building_row(ecs: &bevy_ecs::prelude::World, entity: Entity) -> Option<BuildingRow> {
    let meta = ecs.get::<BuildingMetadata>(entity)?;
    let pos = ecs.get::<GridPosition>(entity)?;
    let stats = ecs.get::<BuildingStats>(entity)?;
    let storage = ecs.get::<BuildingStorage>(entity)?;
    let ownership = ecs.get::<BuildingOwnership>(entity)?;
    let craft = ecs.get::<BuildingCrafting>(entity)?;

    Some(BuildingRow {
        id: meta.id,
        type_code: (meta.pack_type.code() as char).to_string(),
        x: pos.x,
        y: pos.y,
        owner_id: ownership.owner_id,
        clan_id: ownership.clan_id,
        charge: stats.charge,
        max_charge: stats.max_charge,
        cost: stats.cost,
        hp: stats.hp,
        max_hp: stats.max_hp,
        money_inside: storage.money,
        crystals_inside: storage.crystals,
        items_inside: storage.items.clone(),
        craft_recipe_id: craft.recipe_id,
        craft_num: craft.num,
        craft_end_ts: craft.end_ts,
    })
}

/// Helper structure for network sync (temporary "view")
#[derive(Debug, Clone)]
pub struct PackView {
    pub id: i32,
    pub pack_type: PackType,
    pub x: i32,
    pub y: i32,
    pub owner_id: PlayerId,
    pub clan_id: i32,
    pub charge: f32,
    pub max_charge: f32,
    pub hp: i32,
    pub max_hp: i32,
}

impl PackView {
    pub fn off(&self) -> u8 {
        u8::from(self.charge > 0.0)
    }
}
