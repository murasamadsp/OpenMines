use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::sync::OnceLock;

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
    Gate,     // 'N' (клановые ворота — блокируют чужих, без GUI)
}

impl PackType {
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

    /// Cell offsets that this building occupies and the cell type to place.
    /// Returns Vec<(dx, dy, `cell_type`)>. First entry is the anchor.
    pub fn building_cells(self) -> Vec<(i32, i32, u8)> {
        get_building_config(self).map_or_else(Vec::new, |cfg| {
            cfg.cells
                .iter()
                .map(|c| (c.dx, c.dy, c.cell_type))
                .collect()
        })
    }
}

#[derive(Clone)]
pub struct PackData {
    pub id: i32,
    #[allow(dead_code)]
    pub ecs_entity: bevy_ecs::entity::Entity,
    pub pack_type: PackType,
    pub x: i32,
    pub y: i32,
    pub owner_id: i32,
    pub clan_id: i32,
    pub charge: f32,
    pub max_charge: f32,
    pub cost: i32,
    pub hp: i32,
    pub max_hp: i32,
    pub money_inside: i64,
    pub crystals_inside: [i64; 6],
    pub items_inside: std::collections::HashMap<i32, i32>,
    pub craft_recipe_id: Option<i32>,
    pub craft_num: i32,
    pub craft_end_ts: i64,
}

#[allow(clippy::missing_fields_in_debug)]
impl fmt::Debug for PackData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PackData")
            .field("id", &self.id)
            .field("pack_type", &self.pack_type)
            .field("x", &self.x)
            .field("y", &self.y)
            .field("owner_id", &self.owner_id)
            .field("clan_id", &self.clan_id)
            .field("charge", &self.charge)
            .field("max_charge", &self.max_charge)
            .field("cost", &self.cost)
            .field("hp", &self.hp)
            .field("max_hp", &self.max_hp)
            .field("money_inside", &self.money_inside)
            .finish()
    }
}

impl PackData {
    pub fn off(&self) -> u8 {
        u8::from(self.charge > 0.0)
    }
}

#[derive(bevy_ecs::prelude::Component)]
pub struct Position {
    #[allow(dead_code)]
    pub x: i32,
    #[allow(dead_code)]
    pub y: i32,
}

#[derive(bevy_ecs::prelude::Component)]
pub struct Building {
    #[allow(dead_code)]
    pub id: i32,
    #[allow(dead_code)]
    pub type_code: u8,
}

#[derive(bevy_ecs::prelude::Component)]
pub struct Owner {
    #[allow(dead_code)]
    pub pid: i32,
    #[allow(dead_code)]
    pub clan_id: i32,
}

#[derive(bevy_ecs::prelude::Component)]
pub struct Health {
    #[allow(dead_code)]
    pub state: i32,
    #[allow(dead_code)]
    pub max_state: i32,
}

#[derive(bevy_ecs::prelude::Component)]
#[allow(dead_code)]
pub struct Sand;
