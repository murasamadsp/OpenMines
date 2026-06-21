//! `IDamagable` building ticks: hourly hp decay + `NeedEffect` FX broadcast.
//! 1:1 с C# `World.cs` (hourly `Damage(2)` + 0.5s `NeedEffect`/`SendBrokenEffect`).
use crate::game::buildings::{
    BuildingMetadata, BuildingOwnership, BuildingStats, GridPosition, damage_building,
    is_damagable, need_effect,
};
use crate::game::{BroadcastEffect, BroadcastQueue};
use bevy_ecs::prelude::*;
use std::time::{Duration, Instant};

const HOURLY_INTERVAL: Duration = Duration::from_secs(3600);
/// 0.5s effect tick → упрощено до 1s (game tick granularity, visual-only diff).
const EFFECT_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Resource)]
pub struct BuildingDamageTimer {
    pub last_hourly: Instant,
    pub last_effect: Instant,
}

impl Default for BuildingDamageTimer {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            last_hourly: now,
            last_effect: now,
        }
    }
}

/// Hourly `IDamagable.Damage(2)` tick + `NeedEffect`.
/// C# `World.cs:472-486`: each pack that is `IDamagable` → `Damage(2)`; if `NeedEffect` → `SendBrokenEffect`.
#[allow(clippy::needless_pass_by_value)]
pub fn building_hourly_damage_system(
    mut timer: ResMut<BuildingDamageTimer>,
    mut query: Query<(
        &BuildingMetadata,
        &GridPosition,
        &BuildingOwnership,
        &mut BuildingStats,
    )>,
    mut bcast_q: ResMut<BroadcastQueue>,
) {
    if timer.last_hourly.elapsed() < HOURLY_INTERVAL {
        return;
    }
    timer.last_hourly = Instant::now();

    for (meta, bpos, ownership, mut stats) in &mut query {
        if !is_damagable(meta.pack_type) || ownership.owner_id == 0 {
            continue;
        }
        damage_building(&mut stats, 2);
        if need_effect(&stats) {
            send_broken_effect_to_queue(&mut bcast_q, bpos.x, bpos.y);
        }
    }
}

/// Per-second `NeedEffect` FX tick for broken buildings.
/// C# `World.cs:489-504`: every 0.5s, `IDamagable` → if `NeedEffect` → `SendBrokenEffect`.
#[allow(clippy::needless_pass_by_value)]
pub fn building_effect_tick_system(
    mut timer: ResMut<BuildingDamageTimer>,
    query: Query<(
        &BuildingMetadata,
        &GridPosition,
        &BuildingOwnership,
        &BuildingStats,
    )>,
    mut bcast_q: ResMut<BroadcastQueue>,
) {
    if timer.last_effect.elapsed() < EFFECT_INTERVAL {
        return;
    }
    timer.last_effect = Instant::now();

    for (meta, bpos, ownership, stats) in &query {
        if !is_damagable(meta.pack_type) || ownership.owner_id == 0 {
            continue;
        }
        if need_effect(stats) {
            send_broken_effect_to_queue(&mut bcast_q, bpos.x, bpos.y);
        }
    }
}

/// `IDamagable.SendBrokenEffect()` → FX 12 broadcast via `BroadcastQueue`.
fn send_broken_effect_to_queue(bcast_q: &mut BroadcastQueue, x: i32, y: i32) {
    let fx = crate::protocol::packets::hb_fx(
        u16::try_from(x.rem_euclid(65536)).unwrap_or(0),
        u16::try_from(y.rem_euclid(65536)).unwrap_or(0),
        12,
    );
    let data =
        crate::net::session::wire::encode_hb_bundle(&crate::protocol::packets::hb_bundle(&[fx]).1);
    let (cx, cy) = crate::world::World::chunk_pos(x, y);
    bcast_q.0.push(BroadcastEffect::Nearby {
        cx,
        cy,
        data,
        exclude: None,
    });
}
