use bevy_ecs::prelude::Resource;
use std::sync::Arc;

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
