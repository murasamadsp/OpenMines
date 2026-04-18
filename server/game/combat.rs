use crate::game::GameStateResource;
use crate::game::buildings::PackType;
use crate::world::World;
use bevy_ecs::prelude::*;

#[allow(clippy::needless_pass_by_value)]
pub fn gun_firing_system(state_res: Res<GameStateResource>) {
    let state = &state_res.0;
    let now = std::time::Instant::now();

    // Поиск всех пушек (Gun)
    for mut entry in state.packs.iter_mut() {
        let (_, pack) = entry.pair_mut();
        if pack.pack_type != PackType::Gun || pack.charge < 1.0 {
            continue;
        }

        // Поиск целей в радиусе 10 клеток
        let mut target_pid = None;
        for player_entry in &state.active_players {
            let p = player_entry.value();

            // Не стреляем по своим (тот же клан или владелец)
            if p.data.id == pack.owner_id {
                continue;
            }
            if pack.clan_id != 0 && p.data.clan_id == Some(pack.clan_id) {
                continue;
            }

            // Проверка защиты (Protector item)
            if p.protection_until.is_some_and(|until| now < until) {
                continue;
            }

            let dx = p.data.x - pack.x;
            let dy = p.data.y - pack.y;
            let dist_sq = dx * dx + dy * dy;

            if dist_sq <= 100 {
                // Радиус 10
                target_pid = Some(p.data.id);
                break;
            }
        }

        if let Some(pid) = target_pid {
            pack.charge -= 1.0;

            if let Some(mut p_mut) = state.active_players.get_mut(&pid) {
                p_mut.data.health = (p_mut.data.health - 5).max(0);
                p_mut.dirty = true;

                // Sync health to player
                crate::net::session::wire::send_u_packet(
                    &p_mut.tx,
                    "@L",
                    &crate::protocol::packets::health(p_mut.data.health, p_mut.data.max_health).1,
                );

                // FX выстрела
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let fx = crate::protocol::packets::hb_fx(pack.x as u16, pack.y as u16, 1); // 1 = shot fx
                let hb_data = crate::net::session::wire::encode_hb_bundle(
                    &crate::protocol::packets::hb_bundle(&[fx]).1,
                );
                let (cx, cy) = World::chunk_pos(pack.x, pack.y);
                state.broadcast_to_nearby(cx, cy, &hb_data, None);
            }
        }
    }
}
