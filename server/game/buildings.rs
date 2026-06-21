use crate::db::buildings::BuildingRow;
use crate::game::player::PlayerId;
use bevy_ecs::prelude::{Component, Entity};
use rand::Rng as _;
use serde::Deserialize;
use std::collections::HashMap;
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
    let key = pack_type.config_json_key()?;
    cfg.buildings.get(key)
}

/// Как `MinesServer.GameShit.Buildings.PackType` в `server_reference` (`PackType.cs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PackType {
    None,     // ' ' — в т.ч. класс Gate в C# объявлен как `PackType.None`
    Teleport, // 'T'
    Resp,     // 'R'
    Gun,      // 'G'
    Market,   // 'M'
    Up,       // 'U'
    Storage,  // 'L'
    Craft,    // 'F'
    Vulkan,   // 'Q'
    Spot,     // 'O'
    Levi,     // 'W'
    Jobs,     // 'J'
    Zalupa,   // 'Y'
    /// В референсе отдельное enum-значение; в Rust для ворот используем `Gate` (логика), на wire — пробел как у `None`.
    #[allow(clippy::upper_case_acronyms)]
    FLAGBLYAT, // 'D'
    /// Клановые ворота: в C# `Gate : Pack` с `type => PackType.None` — тот же символ, что у `None`.
    Gate,
}

impl PackType {
    const fn config_json_key(self) -> Option<&'static str> {
        match self {
            Self::Teleport => Some("Teleport"),
            Self::Resp => Some("Resp"),
            Self::Gun => Some("Gun"),
            Self::Market => Some("Market"),
            Self::Up => Some("Up"),
            Self::Storage => Some("Storage"),
            Self::Craft => Some("Craft"),
            Self::Spot => Some("Spot"),
            Self::Gate => Some("Gate"),
            _ => None,
        }
    }

    /// Участвует в HB «O»-подпакете; в C# `if (p.type != PackType.None)`.
    pub const fn included_in_hb_overlay(self) -> bool {
        self.code() != b' '
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Teleport => "Teleport",
            Self::Resp => "Resp",
            Self::Gun => "Gun",
            Self::Market => "Market",
            Self::Up => "Up",
            Self::Storage => "Storage",
            Self::Craft => "Craft",
            Self::Vulkan => "Vulkan",
            Self::Spot => "Spot",
            Self::Levi => "Levi",
            Self::Jobs => "Jobs",
            Self::Zalupa => "Zalupa",
            Self::FLAGBLYAT => "FLAGBLYAT",
            Self::Gate => "Gate",
        }
    }

    pub const fn code(self) -> u8 {
        match self {
            Self::None | Self::Gate => b' ',
            Self::Teleport => b'T',
            Self::Resp => b'R',
            Self::Gun => b'G',
            Self::Market => b'M',
            Self::Up => b'U',
            Self::Storage => b'L',
            Self::Craft => b'F',
            Self::Vulkan => b'Q',
            Self::Spot => b'O',
            Self::Levi => b'W',
            Self::Jobs => b'J',
            Self::Zalupa => b'Y',
            Self::FLAGBLYAT => b'D',
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        // Ворота: в C# `type => None` (' '), у нас отдельный вариант для логики клана.
        match s {
            "N" | "Gate" | " " => return Some(Self::Gate),
            _ => {}
        }
        match s.trim() {
            "" | "None" => Some(Self::None),
            "T" | "Teleport" => Some(Self::Teleport),
            "R" | "Resp" => Some(Self::Resp),
            "G" | "Gun" => Some(Self::Gun),
            "M" | "Market" => Some(Self::Market),
            "U" | "Up" => Some(Self::Up),
            "L" | "Storage" => Some(Self::Storage),
            "F" | "Craft" => Some(Self::Craft),
            "Q" | "Vulkan" => Some(Self::Vulkan),
            "O" | "Spot" => Some(Self::Spot),
            "W" | "Levi" => Some(Self::Levi),
            "J" | "Jobs" => Some(Self::Jobs),
            "Y" | "Zalupa" => Some(Self::Zalupa),
            "D" | "FLAGBLYAT" => Some(Self::FLAGBLYAT),
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
    /// `IDamagable`: момент когда hp стало 0. None пока здание не сломано.
    pub broken_timer: Option<std::time::Instant>,
}

/// Возвращает true если тип здания реализует `IDamagable` (C# ref: 8 классов).
#[must_use]
pub const fn is_damagable(pack_type: PackType) -> bool {
    matches!(
        pack_type,
        PackType::Gun
            | PackType::Resp
            | PackType::Teleport
            | PackType::Market
            | PackType::Up
            | PackType::Storage
            | PackType::Craft
            | PackType::Spot
    )
}

/// `IDamagable.Damage(i)` — урон зданию.
/// Возвращает true если charge изменился (нужен HB O resend).
pub fn damage_building(stats: &mut BuildingStats, i: i32) -> bool {
    let charge_before = stats.charge;
    if i > 5 {
        stats.charge = (stats.charge - 100.0).max(0.0);
    }
    if stats.hp == 0 {
        return (stats.charge - charge_before).abs() > f32::EPSILON;
    }
    stats.hp = (stats.hp - i).max(0);
    if stats.hp == 0 && stats.broken_timer.is_none() {
        stats.broken_timer = Some(std::time::Instant::now());
    }
    (stats.charge - charge_before).abs() > f32::EPSILON
}

/// `IDamagable.CanDestroy()` — hp==0 И 8 часов истекло.
#[must_use]
pub fn can_destroy(stats: &BuildingStats) -> bool {
    stats.hp == 0
        && stats
            .broken_timer
            .is_some_and(|t| t.elapsed() >= std::time::Duration::from_secs(8 * 3600))
}

/// `IDamagable.NeedEffect()` — вероятность разрушительного FX (вероятнее в начале 8-часового окна).
#[must_use]
pub fn need_effect(stats: &BuildingStats) -> bool {
    if stats.hp != 0 {
        return false;
    }
    let Some(bt) = stats.broken_timer else {
        return false;
    };
    let elapsed = bt.elapsed().as_secs_f64();
    let total = f64::from(8 * 3600u32);
    // C#: value = percent elapsed (0..=100); effect if random(0..=100) > value
    let value = (elapsed / total * 100.0).round();
    let r = f64::from(rand::rng().random_range(0u32..=100));
    r > value
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

pub fn spawn_building_from_row(ecs: &mut bevy_ecs::prelude::World, row: &BuildingRow) -> Entity {
    let pack_type = PackType::from_str(&row.type_code).unwrap_or(PackType::Resp);
    ecs.spawn((
        BuildingMetadata {
            id: row.id,
            pack_type,
        },
        GridPosition { x: row.x, y: row.y },
        BuildingStats {
            charge: row.charge,
            max_charge: row.max_charge,
            cost: row.cost,
            hp: row.hp,
            max_hp: row.max_hp,
            broken_timer: None,
        },
        BuildingStorage {
            money: row.money_inside,
            crystals: row.crystals_inside,
            items: row.items_inside.clone(),
        },
        BuildingOwnership {
            owner_id: row.owner_id,
            clan_id: row.clan_id,
        },
        BuildingCrafting {
            recipe_id: row.craft_recipe_id,
            num: row.craft_num,
            end_ts: row.craft_end_ts,
        },
        BuildingFlags { dirty: false },
    ))
    .id()
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
    // TODO: max_charge/hp/max_hp will be used when building UI/status packets are fully wired
    #[allow(dead_code)]
    pub max_charge: f32,
    #[allow(dead_code)]
    pub hp: i32,
    #[allow(dead_code)]
    pub max_hp: i32,
}

impl PackView {
    // TODO: will be used when pack on/off state is needed for network overlay
    #[allow(dead_code)]
    pub fn off(&self) -> u8 {
        u8::from(self.charge > 0.0)
    }
}
