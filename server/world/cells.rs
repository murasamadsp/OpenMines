use anyhow::Result;
use serde::{Deserialize, Deserializer, Serialize};
use std::fs;

fn deserialize_name_or_null<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Option::unwrap_or_default)
}

// ─── CellType constants (from C# CellType enum) ───────────────────────────────

#[allow(dead_code)]
pub mod cell_type {
    pub const NOTHING: u8 = 0;
    pub const GATE: u8 = 30;
    pub const EMPTY: u8 = 32;
    pub const ROAD: u8 = 35;
    pub const GOLDEN_ROAD: u8 = 36;
    pub const BUILDING_DOOR: u8 = 37;
    pub const BUILDING_BORDER: u8 = 38;
    pub const POLYMER_ROAD: u8 = 39;
    pub const BLACK_BOULDER1: u8 = 40;
    pub const BLACK_BOULDER2: u8 = 41;
    pub const BLACK_BOULDER3: u8 = 42;
    pub const METAL_BOULDER1: u8 = 43;
    pub const METAL_BOULDER2: u8 = 44;
    pub const METAL_BOULDER3: u8 = 45;
    pub const QUAD_BLOCK: u8 = 48;
    pub const SUPPORT: u8 = 49;
    pub const ALIVE_CYAN: u8 = 50;
    pub const ALIVE_RED: u8 = 51;
    pub const ALIVE_VIOL: u8 = 52;
    pub const ALIVE_BLACK: u8 = 53;
    pub const ALIVE_WHITE: u8 = 54;
    pub const ALIVE_RAINBOW: u8 = 55;
    pub const WHITE_SAND: u8 = 60;
    pub const X_GREEN: u8 = 71;
    pub const X_BLUE: u8 = 72;
    pub const X_RED: u8 = 73;
    pub const X_CYAN: u8 = 74;
    pub const X_VIOLET: u8 = 75;
    pub const MILITARY_BLOCK_FRAME: u8 = 80;
    pub const MILITARY_BLOCK: u8 = 81;
    pub const TELEPORT_BLOCK: u8 = 83;
    pub const BOX: u8 = 90;
    pub const LAVA: u8 = 91;
    pub const BOULDER1: u8 = 92;
    pub const BOULDER2: u8 = 93;
    pub const BOULDER3: u8 = 94;
    // Песка — как в MinesServer `CellType` / `cells.json` (не 46/48: там дыры и QuadBlock=48).
    pub const BLUE_SAND: u8 = 97;
    pub const DARK_BLUE_SAND: u8 = 98;
    pub const YELLOW_SAND: u8 = 99;
    pub const DARK_YELLOW_SAND: u8 = 100;
    pub const GREEN_BLOCK: u8 = 101;
    pub const YELLOW_BLOCK: u8 = 102;
    pub const ROCK: u8 = 103;
    pub const BORDER: u8 = 104;
    pub const RED_BLOCK: u8 = 105;
    pub const GREEN: u8 = 107;
    pub const RED: u8 = 108;
    pub const BLUE: u8 = 109;
    pub const VIOLET: u8 = 110;
    pub const WHITE: u8 = 111;
    pub const CYAN: u8 = 112;
    pub const HEAVY_ROCK: u8 = 113;
    pub const BLACK_ROCK: u8 = 114;
    // 115 — gap
    pub const ALIVE_BLUE: u8 = 116;
    pub const RED_ROCK: u8 = 117;
    // 118 — gap
    pub const HYPNO_ROCK: u8 = 119;
    pub const GOLDEN_ROCK: u8 = 120;
    pub const DEEP_ROCK: u8 = 121;
    /// Как в `cells.json` (в C# enum `GRock` ошибочно = 112).
    pub const G_ROCK: u8 = 122;
    pub const GRAY_ACID: u8 = 66;
    pub const PEARL: u8 = 68;
    pub const PASSIVE_ACID: u8 = 86;
    pub const DARK_WHITE_SAND: u8 = 61;
    pub const RUSTY_SAND: u8 = 62;
    pub const DARK_RUSTY_SAND: u8 = 63;
}

/// Check if a cell type is a crystal (matches C# World.isCry)
pub const fn is_crystal(cell: u8) -> bool {
    matches!(
        cell,
        cell_type::X_GREEN
            | cell_type::GREEN
            | cell_type::X_BLUE
            | cell_type::BLUE
            | cell_type::X_RED
            | cell_type::RED
            | cell_type::X_VIOLET
            | cell_type::VIOLET
            | cell_type::WHITE
            | cell_type::X_CYAN
            | cell_type::CYAN
    )
}

/// Map cell to crystal index 0-5 (matches C# Player.ParseCryType)
/// 0=Green, 1=Blue, 2=Red, 3=Violet, 4=White, 5=Cyan
pub const fn crystal_type(cell: u8) -> Option<usize> {
    match cell {
        cell_type::X_GREEN | cell_type::GREEN => Some(0),
        cell_type::X_BLUE | cell_type::BLUE => Some(1),
        cell_type::X_RED | cell_type::RED => Some(2),
        cell_type::X_VIOLET | cell_type::VIOLET => Some(3),
        cell_type::WHITE => Some(4),
        cell_type::X_CYAN | cell_type::CYAN => Some(5),
        _ => None,
    }
}

/// X-type crystals yield more per hit (from C# Player.Mine)
pub const fn crystal_multiplier(cell: u8) -> i64 {
    match cell {
        cell_type::X_GREEN => 4,
        cell_type::X_BLUE => 3,
        cell_type::X_RED | cell_type::X_VIOLET | cell_type::X_CYAN => 2,
        _ => 1,
    }
}

/// Референс `World.isRoad`
// TODO: will be used when road-type cell detection is needed for movement/building logic
#[allow(dead_code)]
pub const fn is_road(cell: u8) -> bool {
    matches!(
        cell,
        cell_type::ROAD | cell_type::GOLDEN_ROAD | cell_type::POLYMER_ROAD
    )
}

/// Check if cell is a boulder (can be pushed)
pub const fn is_boulder(cell: u8) -> bool {
    matches!(
        cell,
        cell_type::BOULDER1 | cell_type::BOULDER2 | cell_type::BOULDER3
    )
}

/// Dig/build interaction flags (≤3 bools per struct for `clippy::struct_excessive_bools`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CellDefPhysical {
    #[serde(default, rename = "can_place_over")]
    pub can_place_over: bool,
    #[serde(default)]
    pub is_diggable: bool,
    #[serde(default)]
    pub is_destructible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CellDefNature {
    #[serde(default, rename = "isPickable")]
    pub is_pickable: bool,
    #[serde(default, rename = "isSand")]
    pub is_sand: bool,
    #[serde(default, rename = "isBoulder")]
    pub is_boulder: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CellDefPresence {
    #[serde(default, rename = "isEmpty")]
    pub is_empty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CellDef {
    /// В референсном `cells.json` часто `"name": null`.
    #[serde(default, deserialize_with = "deserialize_name_or_null")]
    pub name: String,
    #[serde(default, rename = "type")]
    pub cell_type: u8,
    #[serde(default)]
    pub fall_damage: i32,
    #[serde(default)]
    pub durability: f32,
    #[serde(default)]
    pub damage: i32,
    #[serde(flatten)]
    pub physical: CellDefPhysical,
    #[serde(flatten)]
    pub nature: CellDefNature,
    #[serde(flatten)]
    pub presence: CellDefPresence,
}

impl CellDef {
    #[must_use]
    pub const fn cell_is_empty(&self) -> bool {
        self.presence.is_empty
    }

    #[must_use]
    pub const fn is_sand(&self) -> bool {
        self.nature.is_sand
    }

    #[must_use]
    pub const fn is_diggable(&self) -> bool {
        self.physical.is_diggable
    }

    #[must_use]
    pub const fn can_place_over(&self) -> bool {
        self.physical.can_place_over
    }
}

pub struct CellDefs {
    pub cells: Vec<CellDef>,
}

impl CellDefs {
    pub fn load(path: &str) -> Result<Self> {
        if let Ok(data) = fs::read_to_string(path) {
            let parsed: Vec<CellDef> = serde_json::from_str(&data)?;

            // 1:1 reference contract: `cells.json` is an array of 126 entries indexed by type (0..125).
            // We normalize the loaded file into a dense 126-slot table keyed by `type` to preserve
            // client expectations even if the JSON order is shuffled or has holes.
            let mut cells: Vec<CellDef> = (0..126u8)
                .map(|i| CellDef {
                    cell_type: i,
                    ..Default::default()
                })
                .collect();
            for mut def in parsed {
                if def.cell_type < 126 {
                    let idx = def.cell_type as usize;
                    // Ensure the in-memory slot's type matches its index, even if the JSON was inconsistent.
                    def.cell_type = idx as u8;
                    cells[idx] = def;
                }
            }
            Ok(Self { cells })
        } else {
            let cells: Vec<CellDef> = (0..126u8)
                .map(|i| CellDef {
                    cell_type: i,
                    presence: CellDefPresence {
                        is_empty: i == 0 || i == 32 || i == 36 || i == 37 || i == 39,
                    },
                    physical: CellDefPhysical {
                        is_diggable: i != 0 && i != 36 && i != 37,
                        is_destructible: true,
                        can_place_over: i == 0 || i == 32,
                    },
                    durability: match i {
                        0 | 32 | 36 | 37 | 39 => 0.0,
                        _ => 10.0,
                    },
                    ..Default::default()
                })
                .collect();
            fs::write(path, serde_json::to_string_pretty(&cells)?)?;
            Ok(Self { cells })
        }
    }

    #[inline]
    pub fn get(&self, cell_type: u8) -> &CellDef {
        self.cells.get(cell_type as usize).unwrap_or(&self.cells[0])
    }
}
