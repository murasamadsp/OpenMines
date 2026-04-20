//! BotSpot — stationary programmator bot entity (1:1 with C# `BotSpot : PEntity`).
//!
//! A BotSpot is spawned when a Spot building (PackType::Spot) is placed.
//! It runs programs on behalf of its owner but has its own crystal basket.
//! Key properties from C# reference:
//! - `id = -owner.id` (negative of owner player ID)
//! - `skin = 3`, `tail = 1`
//! - Cannot move (stationary at Spot position)
//! - Digs using owner's skills, deposits crystals into own basket
//! - Runs programmator via `ProgrammatorData.Step()`

use crate::game::player::PlayerId;
use bevy_ecs::prelude::{Component, Entity};

/// Marker component for BotSpot entities in ECS.
#[derive(Component, Debug)]
pub struct BotSpotMarker;

/// Core data for a BotSpot entity.
#[allow(dead_code)]
#[derive(Component, Debug)]
pub struct BotSpotData {
    /// BotSpot ID on the wire (= -owner_id). Stored as i32 for encoding.
    pub bot_id: i32,
    /// Owner player ID.
    pub owner_id: PlayerId,
    /// Owner's clan ID (cached, updated when owner's clan changes).
    pub clan_id: i32,
    /// X position in world (same as Spot building position).
    pub x: i32,
    /// Y position in world (same as Spot building position).
    pub y: i32,
    /// Current facing direction (0=down, 1=left, 2=up, 3=right).
    pub dir: i32,
    /// Associated Spot building entity in ECS.
    pub building_entity: Entity,
}

/// BotSpot's own crystal basket (separate from owner's).
/// 1:1 with C# `BotSpot.crys = new Basket(true)`.
#[allow(dead_code)]
#[derive(Component, Debug, Default)]
pub struct BotSpotBasket {
    /// [Green, Blue, Red, Violet, White, Cyan]
    pub crystals: [i64; 6],
    /// Fractional crystal accumulator (C# `private float cb`).
    pub cb: f32,
}

/// Constants matching C# `BotSpot` class.
#[allow(dead_code)]
impl BotSpotData {
    /// C# `public int tail => 1;`
    pub const TAIL: u8 = 1;
    /// C# `public int skin => 3;`
    pub const SKIN: u8 = 3;
}
