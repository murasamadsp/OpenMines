//! `IDamagable` building ticks: hourly hp decay + `NeedEffect` FX broadcast.
//! 1:1 с C# `World.cs` (hourly `Damage(2)` + 0.5s `NeedEffect`/`SendBrokenEffect`).
use crate::game::buildings::{
    BuildingCrafting, BuildingFlags, BuildingMetadata, BuildingOwnership, BuildingStats,
    GridPosition, PackType, damage_building, is_damagable, need_effect,
};
use crate::game::{BroadcastEffect, BroadcastQueue, PackResendQueue};
use bevy_ecs::prelude::*;

/// Hourly `IDamagable.Damage(2)` tick + `NeedEffect`.
/// C# `World.cs:472-486`: each pack that is `IDamagable` → `Damage(2)`; if `NeedEffect` → `SendBrokenEffect`.
#[allow(clippy::needless_pass_by_value)]
pub fn building_hourly_damage_system(
    mut query: Query<(
        &BuildingMetadata,
        &GridPosition,
        &BuildingOwnership,
        &mut BuildingStats,
    )>,
    mut bcast_q: ResMut<BroadcastQueue>,
) {
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
    query: Query<(
        &BuildingMetadata,
        &GridPosition,
        &BuildingOwnership,
        &BuildingStats,
    )>,
    mut bcast_q: ResMut<BroadcastQueue>,
) {
    for (meta, bpos, ownership, stats) in &query {
        if !is_damagable(meta.pack_type) || ownership.owner_id == 0 {
            continue;
        }
        if need_effect(stats) {
            send_broken_effect_to_queue(&mut bcast_q, bpos.x, bpos.y);
        }
    }
}

/// C# `Crafter.Update()`: when craft progress reaches 100% for the first time,
/// call `base.Update()` -> `Chunk.ResendPack(this)` and mark `ready=true`.
#[allow(clippy::needless_pass_by_value)]
pub fn crafter_completion_resend_system(
    mut query: Query<(
        &BuildingMetadata,
        &GridPosition,
        &mut BuildingCrafting,
        &mut BuildingFlags,
    )>,
    mut pack_resend_q: ResMut<PackResendQueue>,
) {
    let now = crate::time::now_unix();
    for (meta, bpos, mut craft, mut flags) in &mut query {
        if meta.pack_type != PackType::Craft
            || craft.ready
            || craft.recipe_id.is_none()
            || craft.end_ts <= 0
            || now < craft.end_ts
        {
            continue;
        }
        craft.ready = true;
        flags.dirty = true;
        pack_resend_q.0.push((bpos.x, bpos.y));
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

#[cfg(test)]
mod tests {
    use super::crafter_completion_resend_system;
    use crate::game::PackResendQueue;
    use crate::game::buildings::{
        BuildingCrafting, BuildingFlags, BuildingMetadata, GridPosition, PackType,
    };
    use bevy_ecs::prelude::{Schedule, World};

    #[test]
    fn crafter_completion_resend_marks_ready_and_queues_pack_once() {
        let mut world = World::new();
        world.insert_resource(PackResendQueue::default());
        let entity = world
            .spawn((
                BuildingMetadata {
                    id: 1,
                    pack_type: PackType::Craft,
                },
                GridPosition { x: 10, y: 20 },
                BuildingCrafting {
                    recipe_id: Some(0),
                    num: 1,
                    end_ts: crate::time::now_unix() - 1,
                    ready: false,
                },
                BuildingFlags { dirty: false },
            ))
            .id();

        let mut schedule = Schedule::default();
        schedule.add_systems(crafter_completion_resend_system);
        schedule.run(&mut world);

        let craft = world.get::<BuildingCrafting>(entity).unwrap();
        let flags = world.get::<BuildingFlags>(entity).unwrap();
        let queue = world.resource::<PackResendQueue>();
        assert!(craft.ready);
        assert!(flags.dirty);
        assert_eq!(queue.0, vec![(10, 20)]);

        schedule.run(&mut world);
        let queue = world.resource::<PackResendQueue>();
        assert_eq!(queue.0, vec![(10, 20)]);
    }

    #[test]
    fn crafter_completion_resend_ignores_unfinished_craft() {
        let mut world = World::new();
        world.insert_resource(PackResendQueue::default());
        world.spawn((
            BuildingMetadata {
                id: 1,
                pack_type: PackType::Craft,
            },
            GridPosition { x: 10, y: 20 },
            BuildingCrafting {
                recipe_id: Some(0),
                num: 1,
                end_ts: crate::time::now_unix() + 60,
                ready: false,
            },
            BuildingFlags { dirty: false },
        ));

        let mut schedule = Schedule::default();
        schedule.add_systems(crafter_completion_resend_system);
        schedule.run(&mut world);

        assert!(world.resource::<PackResendQueue>().0.is_empty());
    }
}
