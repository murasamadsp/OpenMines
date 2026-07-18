use bevy_ecs::prelude::{Entity, Resource};
use std::sync::Arc;
use std::time::Instant;

use crate::config::{CombatConfig, ProgrammatorConfig, ScheduleConfig};
use crate::db::buildings::BuildingExtra;

use super::super::logic::crafting;
use super::super::structures::buildings::{BuildingCrafting, PackType};
use super::super::{PlayerId, SessionId, WorldPos};

// ─── ECS Resources ──────────────────────────────────────────────────────────

/// Мир (карта/клетки) — выделен из `GameState`, чтобы ECS-системы не зависели
/// от всего `GameState`. Каждая система берёт только то, что реально использует.
#[derive(Resource)]
pub struct WorldResource(pub Arc<crate::world::World>);

#[derive(Resource, Clone, Copy)]
pub struct ProgrammatorConfigResource(pub crate::config::ProgrammatorConfig);

#[derive(Resource, Clone, Copy)]
pub struct CombatConfigResource(pub crate::config::CombatConfig);

#[derive(Resource, Clone, Copy)]
pub struct ScheduleConfigResource(pub crate::config::ScheduleConfig);

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

#[derive(Debug, Clone)]
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

pub fn pack_overlay_off(
    pack_type: PackType,
    charge: i32,
    craft: Option<&BuildingCrafting>,
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
