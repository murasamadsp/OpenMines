use bevy_ecs::prelude::Resource;

use super::super::{PlayerId, WorldPos};

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

#[derive(Resource, Default)]
pub struct PendingCellConversions(pub Vec<PendingConversion>);
