//! Движение робота по миру и рассылка HB соседям.
use crate::net::session::prelude::*;
use crate::game::player::{PlayerPosition, PlayerStats, PlayerCooldowns, PlayerFlags};

pub fn handle_move(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    dir: i32,
) {
    let result = state.modify_player(pid, |ecs, entity| {
        let (px, py, skin, clan) = {
            let pos = ecs.get::<PlayerPosition>(entity)?;
            let stats = ecs.get::<PlayerStats>(entity)?;
            let cd = ecs.get::<PlayerCooldowns>(entity)?;
            if cd.last_move.elapsed().as_millis() < 50 { return None; }
            (pos.x, pos.y, stats.skin, stats.clan_id.unwrap_or(0))
        };

        let (nx, ny) = {
            let (dx, dy) = dir_offset(dir);
            (px + dx, py + dy)
        };

        if !state.world.valid_coord(nx, ny) || !state.world.is_empty(nx, ny) {
            send_u_packet(tx, "@T", &tp(px, py).1);
            return None;
        }

        {
            let mut pos_mut = ecs.get_mut::<PlayerPosition>(entity)?;
            pos_mut.x = nx;
            pos_mut.y = ny;
            pos_mut.dir = dir;
        }
        {
            let mut cd_mut = ecs.get_mut::<PlayerCooldowns>(entity)?;
            cd_mut.last_move = std::time::Instant::now();
        }
        {
            let mut flags_mut = ecs.get_mut::<PlayerFlags>(entity)?;
            flags_mut.dirty = true;
        }

        Some((nx, ny, dir, skin, clan))
    }).flatten();

    if let Some((nx, ny, ndir, skin, clan)) = result {
        let (cx, cy) = World::chunk_pos(nx, ny);
        let bot = hb_bot(
            net_u16_nonneg(pid),
            net_u16_nonneg(nx),
            net_u16_nonneg(ny),
            net_u8_clamped(ndir, 3),
            net_u8_clamped(skin, 255),
            net_u16_nonneg(clan),
            0,
        );
        let hb_data = encode_hb_bundle(&hb_bundle(&[bot]).1);
        state.broadcast_to_nearby(cx, cy, &hb_data, Some(pid));
        crate::net::session::play::chunks::check_chunk_changed(state, tx, pid);
    }
}
