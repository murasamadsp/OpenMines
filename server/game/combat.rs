use crate::db::BoxRow;
use crate::game::buildings::{
    BuildingMetadata, BuildingOwnership, BuildingStats, GridPosition, PackType,
};
use crate::game::player::{
    PlayerConnection, PlayerCooldowns, PlayerFlags, PlayerId, PlayerMetadata, PlayerPosition,
    PlayerSkills as PlayerSkillsCom, PlayerStats,
};
use crate::game::skills::{OnHurt, PlayerSkills as SkillHurt};
use crate::game::{BroadcastEffect, BroadcastQueue, GameStateResource};
use crate::world::WorldProvider;
use crate::world::cells::cell_type;
use bevy_ecs::prelude::*;

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
        &mut PlayerSkillsCom,
        &mut PlayerCooldowns,
    )>,
) {
    let state = &state_res.0;

    for (p_meta, pos, mut stats, conn, mut flags, mut skills, mut cooldowns) in &mut q {
        // C# ref Player.Update: reset c190stacks to 1 after 1 minute
        if cooldowns.last_c190_hit.is_some_and(|t| t.elapsed() >= std::time::Duration::from_secs(60)) {
            cooldowns.c190_stacks = 1;
            cooldowns.last_c190_hit = Some(std::time::Instant::now());
        }
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
            // C# ref: Player.Hurt → Health.AddExp (on every hurt)
            crate::game::skills::add_skill_exp(&mut skills.states, "l", 1.0);
            let sk = crate::protocol::packets::skills_packet(
                &crate::game::skills::skill_progress_payload(&skills.states),
            );
            let _ = conn.tx.send(crate::net::session::wire::make_u_packet_bytes(sk.0, &sk.1));

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
            // C# ref: Player.Hurt → SendDFToBots(6, 0, 0, id, 0) when not lethal
            if stats.health > 0 {
                let fx = crate::protocol::packets::hb_directed_fx(
                    crate::net::session::util::net_u16_nonneg(p_meta.id),
                    0, 0, 6, 0, 0,
                );
                let (cx, cy) = crate::world::World::chunk_pos(px, py);
                bcast_q.0.push(BroadcastEffect::Nearby {
                    cx,
                    cy,
                    data: crate::net::session::wire::encode_hb_bundle(
                        &crate::protocol::packets::hb_bundle(&[fx]).1,
                    ),
                    exclude: Some(p_meta.id),
                });
            }
        }

        if cell == cell_type::BOX {
            if let Ok(Some(BoxRow { crystals: crys, .. })) = state.db.get_box_at(px, py) {
                // ref `PEntity.GetBox`: всегда удаляет запись; кристаллы могут быть нулём.
                for (i, &c) in crys.iter().enumerate() {
                    stats.crystals[i] = stats.crystals[i].saturating_add(c);
                }
                let _ = state.db.delete_box_at(px, py);
                flags.dirty = true;
                let _ = conn.tx.send(crate::net::session::wire::make_u_packet_bytes(
                    "@B",
                    &crate::protocol::packets::basket(&stats.crystals, 1).1,
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
#[allow(clippy::needless_pass_by_value, clippy::type_complexity)]
pub fn gun_firing_system(
    state_res: Res<GameStateResource>,
    mut death_q: ResMut<DeathQueue>,
    mut bcast_q: ResMut<BroadcastQueue>,
    mut guns_query: Query<(
        &BuildingMetadata,
        &mut BuildingStats,
        &BuildingOwnership,
        &GridPosition,
    )>,
    mut players_query: Query<(
        Entity,
        &PlayerMetadata,
        &PlayerPosition,
        &mut PlayerSkillsCom,
        &mut PlayerStats,
        &PlayerCooldowns,
        &PlayerConnection,
        &mut PlayerFlags,
    )>,
) {
    let _state = &state_res.0;
    let now = std::time::Instant::now();

    for (meta, mut b_stats, b_ownership, b_pos) in &mut guns_query {
        if meta.pack_type != PackType::Gun || b_stats.charge <= 0.0 {
            continue;
        }

        let mut target_entity = None;
        for (p_entity, _p_meta, p_pos, _p_sk, p_stats, p_cd, _, _) in players_query.iter() {
            if p_stats.clan_id == Some(b_ownership.clan_id) {
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

        if let Some(entity) = target_entity
            && let Ok((_ent, p_meta, _pos, mut p_sk, mut stats, _cd, conn, mut flags)) =
                players_query.get_mut(entity)
        {
            // C# Player.Hurt(60, DamageType.Gun): skill exp before damage
            crate::game::skills::add_skill_exp(&mut p_sk.states, "l", 1.0); // Health
            crate::game::skills::add_skill_exp(&mut p_sk.states, "*I", 1.0); // Induction
            crate::game::skills::add_skill_exp(&mut p_sk.states, "u", 1.0); // AntiGun
            // Always send @S after skill exp (C# Skill.AddExp always sends)
            let sk_pkt = crate::protocol::packets::skills_packet(
                &crate::game::skills::skill_progress_payload(&p_sk.states),
            );
            let _ = conn.tx.send(crate::net::session::wire::make_u_packet_bytes(
                sk_pkt.0, &sk_pkt.1,
            ));
            // C# Gun.Update order: damage first, then deduct charge
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

            // Charge cost: 0.5 * (Induction_Effect / 100)
            let induction_effect = crate::game::skills::get_player_skill_effect(
                &p_sk.states,
                crate::game::skills::SkillType::Induction,
            );
            let charge_cost = 0.5 * (induction_effect / 100.0);
            if b_stats.charge - charge_cost > 0.0 {
                b_stats.charge -= charge_cost;
            } else {
                b_stats.charge = 0.0;
            }

            let fx = crate::protocol::packets::hb_fx(b_pos.x as u16, b_pos.y as u16, 1);
            let data = crate::net::session::wire::encode_hb_bundle(
                &crate::protocol::packets::hb_bundle(&[fx]).1,
            );
            let (cx, cy) = crate::world::World::chunk_pos(b_pos.x, b_pos.y);
            bcast_q.0.push(BroadcastEffect::Nearby {
                cx,
                cy,
                data,
                exclude: None,
            });
        }
    }
}
