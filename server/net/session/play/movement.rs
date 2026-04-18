//! Движение робота по миру и рассылка HB соседям.
//!
//! Следует методологии server-authoritative из `server/AGENTS.md`:
//! - Клиент предсказывает локально, сервер валидирует.
//! - Rate-limit нарушается → тихий drop (без `@T`).
//! - Любая другая ошибка → `@T old` + `warn!`.
//! - Broadcast соседям исключает отправителя (он уже предсказал).

use crate::net::session::prelude::*;

use super::chunks::check_chunk_changed;
use super::dig_build::handle_dig;
use super::packs::check_pack_at_position;

pub fn handle_move(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    x: i32,
    y: i32,
    dir: i32,
) {
    let raw_dir = dir;
    let (old_x, old_y, old_auto_dig, in_window) = {
        let Some(mut p) = state.active_players.get_mut(&pid) else {
            tracing::warn!("handle_move: pid={pid} not found in active players");
            return;
        };
        if (0..=3).contains(&dir) {
            p.data.dir = dir;
        }
        (p.data.x, p.data.y, p.auto_dig, p.current_window.is_some())
    };

    if in_window {
        // Движение в GUI-окне блокируем (как в оригинале), но не "навсегда":
        // попытка движения считается намерением выйти — закрываем окно и откатываем на текущую позицию.
        if let Some(mut p) = state.active_players.get_mut(&pid) {
            p.current_window = None;
        }
        send_u_packet(tx, "Gu", &[]);
        send_u_packet(tx, "@T", &tp(old_x, old_y).1);
        return;
    }

    // Поворот на месте: клиент шлёт Xmov с dx=0, dy=0 при Shift+стрелка
    // (фича «осмотреться без движения»). Обновляем только dir и рассылаем HB соседям.
    if x == old_x && y == old_y {
        if let Some(mut p) = state.active_players.get_mut(&pid) {
            p.dirty = true;
            if (0..=3).contains(&dir) {
                p.data.dir = dir;
            }
        }
        send_player_move_update(state, pid);
        return;
    }

    // Rate-limiting movement
    {
        let Some(p) = state.active_players.get(&pid) else {
            return;
        };
        let delay_ms = crate::game::skills::get_player_skill_effect(
            &p.data.skills,
            crate::game::skills::SkillType::Movement,
        ) as u64;
        let elapsed = p.last_move_ts.elapsed().as_millis() as u64;
        if elapsed < delay_ms {
            return;
        }
    }

    let old_chunk = World::chunk_pos(old_x, old_y);
    let dir = if dir > 9 { dir - 10 } else { dir };
    let dir = if dir == -1 || old_x != x || old_y != y {
        if old_x > x {
            1
        } else if old_x < x {
            3
        } else if old_y > y {
            2
        } else {
            0
        }
    } else {
        dir
    };
    tracing::debug!(
        "handle_move pid={pid} raw_dir={raw_dir} norm_dir={dir} from=({old_x},{old_y}) to=({x},{y})"
    );
    let (old_chunk_x, old_chunk_y) = old_chunk;
    let (new_chunk_x, new_chunk_y) = World::chunk_pos(x, y);
    tracing::debug!(
        "handle_move state: old_valid={} new_valid={} same_chunk? old=({},{}) new=({},{})",
        state.world.valid_coord(old_x, old_y),
        state.world.valid_coord(x, y),
        old_chunk_x,
        old_chunk_y,
        new_chunk_x,
        new_chunk_y
    );

    if !state.world.valid_coord(x, y) {
        send_u_packet(tx, "@T", &tp(old_x, old_y).1);
        tracing::warn!(
            "handle_move: pid={pid} rejected invalid coord target=({x},{y}) -> teleport back to ({old_x},{old_y})"
        );
        return;
    }

    // Клан-ворота (Gate): блокируют чужих, своих пропускают сквозь непустую клетку.
    let gate_pass = if let Some((gx, gy)) = state.find_pack_covering(x, y) {
        state.get_pack_at(gx, gy).and_then(|pack| {
            if pack.pack_type == PackType::Gate {
                let player_clan = state
                    .active_players
                    .get(&pid)
                    .map_or(0, |p| p.data.clan_id.unwrap_or(0));
                Some(pack.clan_id == 0 || pack.clan_id == player_clan)
            } else {
                None
            }
        })
    } else {
        None
    };
    if gate_pass == Some(false) {
        send_u_packet(tx, "@T", &tp(old_x, old_y).1);
        tracing::warn!("handle_move: pid={pid} rejected: gate blocked at ({x},{y})");
        return;
    }

    if gate_pass != Some(true) && !state.world.is_empty(x, y) {
        send_u_packet(tx, "@T", &tp(old_x, old_y).1);
        tracing::warn!(
            "handle_move: pid={pid} rejected: target blocked at ({x},{y}), dir={dir}, auto_dig={old_auto_dig}"
        );
        if old_auto_dig {
            handle_dig(state, tx, pid, dir);
        }
        return;
    }

    // Check distance
    let (cur_old_x, cur_old_y) = {
        let Some(p) = state.active_players.get(&pid) else {
            tracing::warn!("handle_move: pid={pid} disappeared before distance check");
            return;
        };
        (p.data.x, p.data.y)
    };

    let dx = (x - cur_old_x).abs();
    let dy = (y - cur_old_y).abs();
    if dx + dy != 1 {
        send_u_packet(tx, "@T", &tp(cur_old_x, cur_old_y).1);
        tracing::warn!(
            "handle_move: pid={pid} rejected: non-adjacent move server=({cur_old_x},{cur_old_y}) client_dest=({x},{y}) dx={dx} dy={dy}"
        );
        return;
    }

    let (cx, cy) = World::chunk_pos(x, y);
    tracing::debug!(
        "handle_move: pid={pid} move_ok old=({cur_old_x},{cur_old_y})->({x},{y}) dx={dx} dy={dy} chunk=({cx},{cy})"
    );

    // Update position + rate-limit timestamp
    {
        let Some(mut p) = state.active_players.get_mut(&pid) else {
            return;
        };
        p.data.x = x;
        p.data.y = y;
        p.last_move_ts = std::time::Instant::now();
        if (0..=3).contains(&dir) {
            p.data.dir = dir;
        }
    }

    // Gain Movement skill exp
    {
        let skill_data = {
            let Some(mut p) = state.active_players.get_mut(&pid) else {
                check_chunk_changed(state, tx, pid);
                return;
            };
            if add_skill_exp(&mut p.data.skills, "M", 1.0) {
                Some(skill_progress_payload(&p.data.skills))
            } else {
                None
            }
        };
        if let Some(sd) = skill_data {
            send_u_packet(tx, "SK", &skills_packet(&sd).1);
        }
    }
    if let Some(mut p) = state.active_players.get_mut(&pid) {
        p.dirty = true;
    }

    let has_visible_chunk = {
        state
            .active_players
            .get(&pid)
            .and_then(|p| p.last_chunk)
            .is_some_and(|chunk| chunk == World::chunk_pos(x, y))
    };

    check_pack_at_position(state, tx, pid, x, y);
    let new_chunk = World::chunk_pos(x, y);
    if new_chunk == old_chunk && has_visible_chunk {
        send_player_move_update(state, pid);
    }
    check_chunk_changed(state, tx, pid);
}

pub fn send_player_move_update(state: &Arc<GameState>, pid: PlayerId) {
    let Some(player) = state.active_players.get(&pid) else {
        return;
    };
    let cx = player.chunk_x();
    let cy = player.chunk_y();
    let bot = hb_bot(
        net_u16_nonneg(player.data.id),
        net_u16_nonneg(player.data.x),
        net_u16_nonneg(player.data.y),
        net_u8_clamped(player.data.dir, 3),
        net_u8_clamped(player.data.skin, i32::from(u8::MAX)),
        net_u16_nonneg(player.data.clan_id.unwrap_or(0)),
        0,
    );
    let hb_data = encode_hb_bundle(&hb_bundle(&[bot]).1);
    // Отправителя исключаем: он уже предсказал движение локально.
    // Echo собственной позиции от сервера мог бы вызвать визуальный откат при лаге.
    state.broadcast_to_nearby(cx, cy, &hb_data, Some(pid));
}
