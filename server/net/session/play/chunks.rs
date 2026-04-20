//! Подгрузка и синхронизация чанков вокруг игрока.
use crate::net::session::prelude::*;

pub fn check_chunk_changed(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let p_data = state
        .query_player(pid, |ecs, entity| {
            let pos = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
            let view = ecs.get::<crate::game::player::PlayerView>(entity)?;
            Some((
                pos.chunk_x(),
                pos.chunk_y(),
                view.last_chunk,
                view.visible_chunks.clone(),
            ))
        })
        .flatten();

    let Some((cx, cy, last_chunk, old_visible)) = p_data else {
        return;
    };
    if Some((cx, cy)) == last_chunk {
        return;
    }

    let new_visible = state.visible_chunks_around(cx, cy);
    let old_visible_set: HashSet<(u32, u32)> = old_visible.iter().copied().collect();
    let mut sub_packets: Vec<Vec<u8>> = Vec::new();
    let mut sub_batch_bytes = 0usize;
    let mut sent_batches = 0usize;

    let flush_sub_packets = |sub_packets: &mut Vec<Vec<u8>>,
                             sent_batches: &mut usize,
                             sub_batch_bytes: &mut usize,
                             tx: &mpsc::UnboundedSender<Vec<u8>>,
                             _pid: PlayerId| {
        if sub_packets.is_empty() {
            return;
        }
        let bundle = hb_bundle(sub_packets.as_slice()).1;
        send_b_packet(tx, "HB", &bundle);
        *sent_batches += 1;
        sub_packets.clear();
        *sub_batch_bytes = 0;
    };

    for (ncx, ncy) in new_visible.iter().copied() {
        if old_visible_set.contains(&(ncx, ncy)) {
            continue;
        }

        let cells = state.world.read_chunk_cells(ncx, ncy);
        let ox = u16::try_from((ncx * 32).min(u16::MAX as u32)).unwrap_or(u16::MAX);
        let oy = u16::try_from((ncy * 32).min(u16::MAX as u32)).unwrap_or(u16::MAX);
        sub_packets.push(hb_map(ox, oy, 32, 32, &cells));
        sub_batch_bytes += sub_packets.last().map_or(0, |p| p.len());

        // Сначала отправляем ботов (игроков), чтобы клиент знал о них до обработки построек
        for entry in &state.active_players {
            let opid = *entry.key();
            let bot_data = state
                .query_player(opid, |ecs, entity| {
                    let p = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
                    let s = ecs.get::<crate::game::player::PlayerStats>(entity)?;
                    if p.chunk_x() == ncx && p.chunk_y() == ncy {
                        Some(hb_bot(
                            net_u16_nonneg(opid),
                            net_u16_nonneg(p.x),
                            net_u16_nonneg(p.y),
                            net_u8_clamped(p.dir, 3),
                            net_u8_clamped(s.skin, 255),
                            net_u16_nonneg(s.clan_id.unwrap_or(0)),
                            0,
                        ))
                    } else {
                        None
                    }
                })
                .flatten();
            if let Some(bot) = bot_data {
                sub_packets.push(bot);
                sub_batch_bytes += sub_packets.last().map_or(0, |p| p.len());
            }
        }

        // Затем отправляем постройки
        for (code, px, py, cid, off) in state.get_packs_in_chunk_area(ncx, ncy) {
            if let Some(block_pos) = state.pack_block_pos(i32::from(px), i32::from(py)) {
                sub_packets.push(hb_packs(block_pos, &[(code, px, py, cid, off)]));
                sub_batch_bytes += sub_packets.last().map_or(0, |p| p.len());
            }
        }

        if sub_packets.len() >= CHUNK_BUNDLE_MAX_SUBPACKETS
            || sub_batch_bytes >= CHUNK_BUNDLE_MAX_BYTES
        {
            flush_sub_packets(
                &mut sub_packets,
                &mut sent_batches,
                &mut sub_batch_bytes,
                tx,
                pid,
            );
        }
    }

    for (ocx, ocy) in old_visible {
        if new_visible.contains(&(ocx, ocy)) {
            continue;
        }
        for (_, px, py, _, _) in state.get_packs_in_chunk_area(ocx, ocy) {
            if let Some(block_pos) = state.pack_block_pos(i32::from(px), i32::from(py)) {
                sub_packets.push(hb_packs(block_pos, &[]));
                sub_batch_bytes += sub_packets.last().map_or(0, |p| p.len());
                if sub_packets.len() >= CHUNK_BUNDLE_MAX_SUBPACKETS
                    || sub_batch_bytes >= CHUNK_BUNDLE_MAX_BYTES
                {
                    flush_sub_packets(
                        &mut sub_packets,
                        &mut sent_batches,
                        &mut sub_batch_bytes,
                        tx,
                        pid,
                    );
                }
            }
        }
    }

    if !sub_packets.is_empty() {
        let bundle = hb_bundle(sub_packets.as_slice()).1;
        send_b_packet(tx, "HB", &bundle);
    }

    // Обновляем ECS (view) и chunk_players РАЗДЕЛЬНО, чтобы не держать ecs.write()
    // одновременно с chunk_players — иначе deadlock с broadcast_to_nearby,
    // который держит chunk_players.read() и хочет ecs.read().
    let chunk_update = state
        .modify_player(pid, |ecs, entity| {
            let (ncx, ncy) = {
                let pos = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
                (pos.chunk_x(), pos.chunk_y())
            };
            let mut view = ecs.get_mut::<crate::game::player::PlayerView>(entity)?;
            let (ocx, ocy) = view.last_chunk.unwrap_or((0, 0));
            view.last_chunk = Some((ncx, ncy));
            view.visible_chunks = new_visible;
            if (ocx, ocy) != (ncx, ncy) {
                Some((ocx, ocy, ncx, ncy))
            } else {
                None
            }
        })
        .flatten();
    // ecs.write() отпущен — безопасно обновляем chunk_players.
    if let Some((ocx, ocy, ncx, ncy)) = chunk_update {
        state
            .chunk_players
            .get_mut(&(ocx, ocy))
            .map(|mut e| e.retain(|&id| id != pid));
        state.chunk_players.entry((ncx, ncy)).or_default().push(pid);
    }
}
