use crate::game::{GameStateResource, BroadcastQueue, BroadcastEffect};
use crate::game::buildings::{
    BuildingMetadata, BuildingOwnership, BuildingStats, GridPosition, PackType,
};
use crate::game::player::{
    PlayerConnection, PlayerCooldowns, PlayerFlags, PlayerId, PlayerMetadata, PlayerPosition, PlayerStats,
    PlayerSkills as PlayerSkillsCom,
};
use crate::game::skills::{OnHurt, PlayerSkills as SkillHurt};
use crate::world::WorldProvider;
use crate::world::cells::cell_type;
use bevy_ecs::prelude::*;
use crate::db::BoxRow;

/// Очередь смерти после `gun_firing_system`: нельзя вызывать `handle_death` изнутри `schedule.run` (вложенный `ecs.write()`).
#[derive(Resource, Default)]
pub struct DeathQueue(pub Vec<PlayerId>);

/// `Player.Update`: стоя на непустой клетке — `Hurt(fall_damage)`; далее ящик 90 / `is_destructible` (как `Player.cs` при `!isEmpty`).
#[allow(clippy::needless_pass_by_value)]
pub fn standing_cell_hazard_system(
    state_res: Res<GameStateResource>,
    mut death_q: ResMut<DeathQueue>,
    mut bcast_q: ResMut<BroadcastQueue>,
    mut q: Query<(
        &PlayerMetadata,
        &PlayerPosition,
        &mut PlayerStats,
        &PlayerConnection,
        &mut PlayerFlags,
    )>,
) {
    let state = &state_res.0;

    for (p_meta, pos, mut stats, conn, mut flags) in &mut q {
        let (px, py) = (pos.x, pos.y);
        if !state.world.valid_coord(px, py) {
            continue;
        }
        let cell = state.world.get_cell(px, py);
        let pdef = {
            let defs = state.world.cell_defs();
            defs.get(cell).clone()
        };
        if pdef.cell_is_empty() {
            continue;
        }
        if pdef.fall_damage > 0 {
            let fd = pdef.fall_damage;
            if stats.health > fd {
                stats.health -= fd;
            } else {
                stats.health = 0;
                death_q.0.push(p_meta.id);
            }
            flags.dirty = true;
            let _ = conn.tx.send(crate::net::session::wire::make_u_packet_bytes(
                "@L",
                &crate::protocol::packets::health(stats.health, stats.max_health).1,
            ));
        }

        if cell == cell_type::BOX {
            if let Ok(Some(BoxRow { crystals: crys, .. })) = state.db.get_box_at(px, py) {
                // ref `PEntity.GetBox`: всегда удаляет запись; кристаллы могут быть нулём.
                for i in 0..6 {
                    stats.crystals[i] = stats.crystals[i].saturating_add(crys[i]);
                }
                let _ = state.db.delete_box_at(px, py);
                flags.dirty = true;
                let _ = conn.tx.send(crate::net::session::wire::make_u_packet_bytes(
                    "@B",
                    &crate::protocol::packets::basket(&stats.crystals, 1000).1,
                ));
            }
            let _ = state.world.damage_cell(px, py, 1.0);
            bcast_q.0.push(BroadcastEffect::CellUpdate(px, py));
        } else if pdef.physical.is_destructible {
            state.world.destroy(px, py);
            bcast_q.0.push(BroadcastEffect::CellUpdate(px, py));
        }
    }
}

/// Радиус 20 (см. `Vector2.Distance(…) <= 20` в `Gun.cs`), 60 HP, `DamageType.Gun` → AntiGun.
#[allow(clippy::needless_pass_by_value)]
pub fn gun_firing_system(
    state_res: Res<GameStateResource>,
    mut death_q: ResMut<DeathQueue>,
    mut bcast_q: ResMut<BroadcastQueue>,
    mut guns_query: Query<(&BuildingMetadata, &mut BuildingStats, &BuildingOwnership, &GridPosition)>,
    mut players_query: Query<(
        Entity,
        &PlayerMetadata,
        &PlayerPosition,
        &PlayerSkillsCom,
        &mut PlayerStats,
        &PlayerCooldowns,
        &PlayerConnection,
        &mut PlayerFlags,
    )>,
) {
    let state = &state_res.0;
    let now = std::time::Instant::now();

    for (meta, mut b_stats, b_ownership, b_pos) in &mut guns_query {
        if meta.pack_type != PackType::Gun || b_stats.charge < 1.0 {
            continue;
        }

        let mut target_entity = None;
        for (p_entity, p_meta, p_pos, _p_sk, p_stats, p_cd, _, _) in &players_query {
            if p_meta.id == b_ownership.owner_id {
                continue;
            }
            if b_ownership.clan_id != 0 && p_stats.clan_id == Some(b_ownership.clan_id) {
                continue;
            }
            if p_cd.protection_until.is_some_and(|u| now < u) {
                continue;
            }

            let dx = p_pos.x - b_pos.x;
            let dy = p_pos.y - b_pos.y;
            if dx * dx + dy * dy <= 400 {
                target_entity = Some(p_entity);
                break;
            }
        }

        if let Some(entity) = target_entity {
            b_stats.charge -= 1.0;
            if let Ok((_ent, p_meta, _pos, p_sk, mut stats, _cd, conn, mut flags)) = players_query.get_mut(entity) {
                let sk = SkillHurt {
                    skills: &p_sk.states,
                };
                let dmg = (sk.on_hurt(60.0).round() as i32).max(0);
                if dmg > 0 {
                    if stats.health > dmg {
                        stats.health -= dmg;
                        flags.dirty = true;
                    } else {
                        stats.health = 0;
                        flags.dirty = true;
                        death_q.0.push(p_meta.id);
                    }
                }
                let _ = conn.tx.send(crate::net::session::wire::make_u_packet_bytes(
                    "@L",
                    &crate::protocol::packets::health(stats.health, stats.max_health).1,
                ));

                let fx = crate::protocol::packets::hb_fx(b_pos.x as u16, b_pos.y as u16, 1);
                let data = crate::net::session::wire::encode_hb_bundle(
                    &crate::protocol::packets::hb_bundle(&[fx]).1,
                );
                let (cx, cy) = crate::world::World::chunk_pos(b_pos.x, b_pos.y);
                bcast_q.0.push(BroadcastEffect::Nearby { cx, cy, data, exclude: None });
            }
        }
    }
}
