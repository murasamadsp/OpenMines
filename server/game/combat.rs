use crate::game::GameStateResource;
use crate::game::buildings::{PackType, BuildingMetadata, BuildingStats, BuildingOwnership, GridPosition};
use crate::game::player::{PlayerId, PlayerPosition, PlayerStats, PlayerCooldowns, PlayerConnection, PlayerFlags, PlayerMetadata};
use crate::world::WorldProvider;
use bevy_ecs::prelude::*;

/// Очередь смерти после `gun_firing_system`: нельзя вызывать `handle_death` изнутри `schedule.run` (вложенный `ecs.write()`).
#[derive(Resource, Default)]
pub struct DeathQueue(pub Vec<PlayerId>);

#[allow(clippy::needless_pass_by_value)]
pub fn gun_firing_system(
    state_res: Res<GameStateResource>,
    mut death_q: ResMut<DeathQueue>,
    mut guns_query: Query<(&BuildingMetadata, &mut BuildingStats, &BuildingOwnership, &GridPosition)>,
    mut players_query: Query<(
        Entity,
        &PlayerMetadata,
        &PlayerPosition,
        &mut PlayerStats,
        &PlayerCooldowns,
        &PlayerConnection,
        &mut PlayerFlags,
    )>,
) {
    let state = &state_res.0;
    let now = std::time::Instant::now();

    for (meta, mut b_stats, b_ownership, b_pos) in &mut guns_query {
        if meta.pack_type != PackType::Gun || b_stats.charge < 1.0 { continue; }

        let mut target_entity = None;
        for (p_entity, p_meta, p_pos, p_stats, p_cd, _, _) in &players_query {
            if p_meta.id == b_ownership.owner_id { continue; }
            if b_ownership.clan_id != 0 && p_stats.clan_id == Some(b_ownership.clan_id) { continue; }
            if p_cd.protection_until.is_some_and(|u| now < u) { continue; }

            let dx = p_pos.x - b_pos.x;
            let dy = p_pos.y - b_pos.y;
            if dx*dx + dy*dy <= 100 {
                target_entity = Some(p_entity);
                break;
            }
        }

        if let Some(entity) = target_entity {
            b_stats.charge -= 1.0;
            if let Ok((_ent, p_meta, _pos, mut stats, _cd, conn, mut flags)) = players_query.get_mut(entity) {
                // Как `Player.Hurt` с `DamageType.Gun`: при `Health - num <= 0` → `Death()` (референс `Player.cs`).
                let dmg = 5;
                if stats.health > dmg {
                    stats.health -= dmg;
                    flags.dirty = true;
                } else {
                    stats.health = 0;
                    flags.dirty = true;
                    death_q.0.push(p_meta.id);
                }
                let _ = conn.tx.send(crate::net::session::wire::make_u_packet_bytes("@L", &crate::protocol::packets::health(stats.health, stats.max_health).1));

                let fx = crate::protocol::packets::hb_fx(b_pos.x as u16, b_pos.y as u16, 1);
                let data = crate::net::session::wire::encode_hb_bundle(&crate::protocol::packets::hb_bundle(&[fx]).1);
                let (cx, cy) = crate::world::World::chunk_pos(b_pos.x, b_pos.y);
                state.broadcast_to_nearby(cx, cy, &data, None);
            }
        }
    }
}
