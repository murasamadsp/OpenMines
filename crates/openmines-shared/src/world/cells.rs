use anyhow::Result;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashSet;
use std::fs;

const CELL_TYPE_COUNT: u8 = 126;

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
    /// Невидимый неразрушаемый блок — каркас/стена футпринта здания
    /// (см. `docs/ITEMS_BUILDINGS_STATUS.md`).
    pub const INVISIBLE_BLOCK: u8 = 106;
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
    /// Legacy JS programmator `is_slime` includes 82. In current `cells.json` this is `ВБ песок`.
    pub const V_B_SAND: u8 = 82;
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
    // Note: ACID_ROCK = 118 is defined below with other acid constants.
    pub const HYPNO_ROCK: u8 = 119;
    pub const GOLDEN_ROCK: u8 = 120;
    pub const DEEP_ROCK: u8 = 121;
    /// Как в `cells.json` (в C# enum `GRock` ошибочно = 112).
    pub const G_ROCK: u8 = 122;
    pub const GRAY_ACID: u8 = 66;
    pub const PURPLE_ACID: u8 = 67;
    pub const PEARL: u8 = 68;
    pub const PASSIVE_ACID: u8 = 86;
    pub const SKULL: u8 = 88;
    pub const LIVING_ACTIVE_ACID: u8 = 95;
    pub const CORROSIVE_ACTIVE_ACID: u8 = 96;
    pub const DARK_WHITE_SAND: u8 = 61;
    pub const RUSTY_SAND: u8 = 62;
    pub const DARK_RUSTY_SAND: u8 = 63;
    pub const ACID_ROCK: u8 = 118;
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

/// Референс `World.isRoad` (World.cs:394): `Road` | `GoldenRoad` | `PolymerRoad` | `BuildingDoor`.
pub const fn is_road(cell: u8) -> bool {
    matches!(
        cell,
        cell_type::ROAD
            | cell_type::GOLDEN_ROAD
            | cell_type::POLYMER_ROAD
            | cell_type::BUILDING_DOOR
    )
}

/// Check if cell is a boulder (can be pushed)
pub const fn is_boulder(cell: u8) -> bool {
    matches!(
        cell,
        cell_type::BOULDER1 | cell_type::BOULDER2 | cell_type::BOULDER3
    )
}

/// Programmator acid check: C#/JS slime-acid family used by legacy programs.
pub const fn is_acid(cell: u8) -> bool {
    matches!(
        cell,
        cell_type::GRAY_ACID
            | cell_type::PURPLE_ACID
            | cell_type::PASSIVE_ACID
            | cell_type::LIVING_ACTIVE_ACID
            | cell_type::CORROSIVE_ACTIVE_ACID
            | cell_type::ACID_ROCK
    )
}

/// Programmator slime check: legacy JS set, including byte 82 for compatibility.
pub const fn is_slime(cell: u8) -> bool {
    matches!(
        cell,
        cell_type::GRAY_ACID
            | cell_type::PURPLE_ACID
            | cell_type::PEARL
            | cell_type::V_B_SAND
            | cell_type::PASSIVE_ACID
            | cell_type::LIVING_ACTIVE_ACID
            | cell_type::CORROSIVE_ACTIVE_ACID
            | cell_type::ACID_ROCK
    )
}

pub const fn is_living_crystal(cell: u8) -> bool {
    matches!(
        cell,
        cell_type::ALIVE_CYAN
            | cell_type::ALIVE_RED
            | cell_type::ALIVE_VIOL
            | cell_type::ALIVE_BLACK
            | cell_type::ALIVE_WHITE
            | cell_type::ALIVE_RAINBOW
            | cell_type::ALIVE_BLUE
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
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let path_ref = path.as_ref();
        let data = fs::read_to_string(path_ref)?;
        let parsed: Vec<CellDef> = serde_json::from_str(&data)?;

        // 1:1 reference contract: `cells.json` is an array of 126 entries indexed by type (0..125).
        // We normalize the loaded file into a dense 126-slot table keyed by `type` to preserve
        // client expectations even if the JSON order is shuffled or has holes.
        let mut cells: Vec<CellDef> = (0..CELL_TYPE_COUNT)
            .map(|i| CellDef {
                cell_type: i,
                ..Default::default()
            })
            .collect();
        // Валидация: все типы должны быть 0..125, дубликаты — ошибка.
        let mut seen = HashSet::new();
        for def in &parsed {
            if def.cell_type >= CELL_TYPE_COUNT {
                anyhow::bail!(
                    "Cell type {} out of range [0..125] in cells.json",
                    def.cell_type
                );
            }
            if !seen.insert(def.cell_type) {
                anyhow::bail!("Duplicate cell type {} in cells.json", def.cell_type);
            }
        }
        if seen.len() != usize::from(CELL_TYPE_COUNT) {
            let missing = (0..CELL_TYPE_COUNT)
                .find(|cell_type| !seen.contains(cell_type))
                .expect("cell_defs missing count did not identify a missing type");
            anyhow::bail!("Missing cell type {missing} in cells.json");
        }

        for def in parsed {
            let idx = def.cell_type as usize;
            cells[idx] = def;
        }
        Ok(Self { cells })
    }

    #[inline]
    pub fn get(&self, cell_type: u8) -> &CellDef {
        self.cells
            .get(cell_type as usize)
            .expect("unknown cell type id")
    }

    #[inline]
    pub fn get_typed(&self, cell_type: CellType) -> &CellDef {
        self.get(cell_type.0)
    }
}

/// A strongly-typed wrapper around raw `u8` cell identifiers.
/// Provides compiler-enforced safety to avoid mixing up raw byte indexes and cell type constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct CellType(pub u8);

impl CellType {
    #[inline]
    #[must_use]
    pub const fn new(val: u8) -> Self {
        Self(val)
    }

    #[inline]
    #[must_use]
    pub const fn is(self, val: u8) -> bool {
        self.0 == val
    }

    #[inline]
    #[must_use]
    pub const fn is_crystal(self) -> bool {
        is_crystal(self.0)
    }

    #[inline]
    #[must_use]
    pub const fn crystal_type(self) -> Option<usize> {
        crystal_type(self.0)
    }

    #[inline]
    #[must_use]
    pub const fn crystal_multiplier(self) -> i64 {
        crystal_multiplier(self.0)
    }

    #[inline]
    #[must_use]
    pub const fn is_road(self) -> bool {
        is_road(self.0)
    }

    #[inline]
    #[must_use]
    pub const fn is_boulder(self) -> bool {
        is_boulder(self.0)
    }

    #[inline]
    #[must_use]
    pub const fn is_acid(self) -> bool {
        is_acid(self.0)
    }

    #[inline]
    #[must_use]
    pub const fn is_slime(self) -> bool {
        is_slime(self.0)
    }

    #[inline]
    #[must_use]
    pub const fn is_living_crystal(self) -> bool {
        is_living_crystal(self.0)
    }

    #[inline]
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == cell_type::EMPTY
    }

    #[inline]
    #[must_use]
    pub const fn is_sand(self) -> bool {
        matches!(
            self.0,
            cell_type::WHITE_SAND
                | cell_type::BLUE_SAND
                | cell_type::DARK_BLUE_SAND
                | cell_type::YELLOW_SAND
                | cell_type::DARK_YELLOW_SAND
                | cell_type::DARK_WHITE_SAND
                | cell_type::RUSTY_SAND
                | cell_type::DARK_RUSTY_SAND
        )
    }
}

impl From<u8> for CellType {
    #[inline]
    fn from(val: u8) -> Self {
        Self(val)
    }
}

impl From<CellType> for u8 {
    #[inline]
    fn from(cell: CellType) -> Self {
        cell.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_road_matches_csharp_set() {
        // C# World.isRoad (World.cs:394): Road | GoldenRoad | PolymerRoad | BuildingDoor.
        assert!(is_road(cell_type::ROAD));
        assert!(is_road(cell_type::GOLDEN_ROAD));
        assert!(is_road(cell_type::POLYMER_ROAD));
        assert!(is_road(cell_type::BUILDING_DOOR)); // ранее пропускалось — паритет-фикс
        assert!(!is_road(cell_type::NOTHING));
        assert!(!is_road(cell_type::EMPTY));
    }

    #[test]
    fn crystal_classification_round_trips() {
        // Каждый кристалл-тип → валидный индекс 0..5; не-кристалл → None.
        for c in [
            cell_type::X_GREEN,
            cell_type::GREEN,
            cell_type::X_CYAN,
            cell_type::CYAN,
            cell_type::WHITE,
        ] {
            assert!(is_crystal(c), "cell {c} — кристалл");
            assert!(
                crystal_type(c).is_some_and(|i| i < 6),
                "индекс кристалла 0..5"
            );
        }
        assert!(!is_crystal(cell_type::EMPTY));
        assert!(crystal_type(cell_type::ROCK).is_none());
    }

    #[test]
    fn cell_type_helpers() {
        let road = CellType::new(cell_type::ROAD);
        assert!(road.is_road());
        assert!(!road.is_crystal());
        assert!(road.is(cell_type::ROAD));

        let green = CellType::new(cell_type::GREEN);
        assert!(green.is_crystal());
        assert_eq!(green.crystal_type(), Some(0));
    }

    #[test]
    fn programmator_cell_predicates_are_typed() {
        assert!(CellType::new(cell_type::GRAY_ACID).is_acid());
        assert!(CellType::new(cell_type::ACID_ROCK).is_acid());
        assert!(!CellType::new(cell_type::PEARL).is_acid());

        assert!(CellType::new(cell_type::PEARL).is_slime());
        assert!(CellType::new(cell_type::V_B_SAND).is_slime());
        assert!(!CellType::new(cell_type::GREEN).is_slime());

        assert!(CellType::new(cell_type::ALIVE_CYAN).is_living_crystal());
        assert!(CellType::new(cell_type::ALIVE_BLUE).is_living_crystal());
        assert!(!CellType::new(cell_type::CYAN).is_living_crystal());
    }

    #[test]
    fn cell_defs_get_rejects_unknown_cell_id_instead_of_falling_back_to_zero() {
        let defs = CellDefs {
            cells: (0..CELL_TYPE_COUNT)
                .map(|cell_type| CellDef {
                    cell_type,
                    ..Default::default()
                })
                .collect(),
        };

        assert_eq!(defs.get(0).cell_type, 0);
        assert!(
            std::panic::catch_unwind(|| defs.get(CELL_TYPE_COUNT)).is_err(),
            "unknown cell id must fail fast, not read cell 0"
        );
    }

    #[test]
    fn load_rejects_missing_cell_type_instead_of_filling_default() {
        let path = std::env::temp_dir().join(format!(
            "openmines_cells_missing_{}_{}.json",
            std::process::id(),
            CELL_TYPE_COUNT
        ));
        let defs: Vec<_> = (0..CELL_TYPE_COUNT - 1)
            .map(|cell_type| serde_json::json!({ "type": cell_type }))
            .collect();
        std::fs::write(&path, serde_json::to_vec(&defs).unwrap()).unwrap();

        let Err(err) = CellDefs::load(&path) else {
            panic!("missing cell type must be rejected");
        };
        assert!(
            err.to_string().contains("Missing cell type 125"),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_file(path);
    }
}
