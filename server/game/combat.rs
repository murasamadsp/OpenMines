use crate::game::GameStateResource;
use crate::game::buildings::PackType;
use crate::game::player::{PlayerPosition, PlayerStats, PlayerCooldowns, PlayerConnection, PlayerFlags, PlayerMetadata};
use crate::world::WorldProvider;
use bevy_ecs::prelude::*;

#[allow(clippy::needless_pass_by_value)]
pub fn gun_firing_system(
    state_res: Res<GameStateResource>,
    mut query: Query<(
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

    for mut entry in state.packs.iter_mut() {
        let pack = entry.value_mut();
        if pack.pack_type != PackType::Gun || pack.charge < 1.0 { continue; }

        let mut target_entity = None;
        for (meta, pos, stats, cooldowns, _, _) in &query {
            if meta.id == pack.owner_id { continue; }
            if pack.clan_id != 0 && stats.clan_id == Some(pack.clan_id) { continue; }
            if cooldowns.protection_until.is_some_and(|u| now < u) { continue; }

            let dx = pos.x - pack.x;
            let dy = pos.y - pack.y;
            if dx*dx + dy*dy <= 100 {
                target_entity = state.get_player_entity(meta.id);
                break;
            }
        }

        if let Some(entity) = target_entity {
            pack.charge -= 1.0;
            if let Ok((_meta, _pos, mut stats, _cd, conn, mut flags)) = query.get_mut(entity) {
                stats.health = (stats.health - 5).max(0);
                flags.dirty = true;
                let _ = conn.tx.send(crate::net::session::wire::make_u_packet_bytes("@L", &crate::protocol::packets::health(stats.health, stats.max_health).1));
                
                let fx = crate::protocol::packets::hb_fx(pack.x as u16, pack.y as u16, 1);
                let data = crate::net::session::wire::encode_hb_bundle(&crate::protocol::packets::hb_bundle(&[fx]).1);
                let (cx, cy) = crate::world::World::chunk_pos(pack.x, pack.y);
                state.broadcast_to_nearby(cx, cy, &data, None);
            }
        }
    }
}
