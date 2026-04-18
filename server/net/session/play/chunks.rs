//! Подгрузка и синхронизация чанков вокруг игрока.
use crate::net::session::prelude::*;

pub fn check_chunk_changed(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let (cx, cy, last_chunk, old_visible) = {
        let Some(p) = state.active_players.get(&pid) else {
            return;
        };
        (
            p.chunk_x(),
            p.chunk_y(),
            p.last_chunk,
            p.visible_chunks.clone(),
        )
    };

    if Some((cx, cy)) == last_chunk {
        return;
    }

    let new_visible = state.visible_chunks_around(cx, cy);
    let old_visible_set: HashSet<(u32, u32)> = old_visible.iter().copied().collect();
    let mut sub_packets: Vec<Vec<u8>> = Vec::new();
    let mut sub_batch_bytes = 0usize;
    let mut sent_batches = 0usize;
    let mut added_chunks = 0usize;
    let mut removed_chunks = 0usize;
    let mut map_packets = 0usize;
    let mut pack_packets = 0usize;
    let mut bot_packets = 0usize;
    let mut clear_pack_packets = 0usize;

    let flush_sub_packets = |sub_packets: &mut Vec<Vec<u8>>,
                             sent_batches: &mut usize,
                             sub_batch_bytes: &mut usize,
                             tx: &mpsc::UnboundedSender<Vec<u8>>,
                             pid: PlayerId| {
        if sub_packets.is_empty() {
            return;
        }
        let bundle = hb_bundle(sub_packets.as_slice()).1;
        tracing::debug!(
            "check_chunk_changed hb pid={pid} batch={sent_batches} packets={} bytes={}",
            sub_packets.len(),
            bundle.len()
        );
        send_b_packet(tx, "HB", &bundle);
        *sent_batches += 1;
        sub_packets.clear();
        *sub_batch_bytes = 0;
    };

    // Send newly visible chunks in HB batches
    // in the same logical order as reference:
    // M (map), O (packs), X (bots).
    for (ncx, ncy) in new_visible.iter().copied() {
        if old_visible_set.contains(&(ncx, ncy)) {
            continue;
        }
        added_chunks += 1;

        let cells = state.world.read_chunk_cells(ncx, ncy);
        let chunk_base_x = ncx.saturating_mul(32);
        let chunk_base_y = ncy.saturating_mul(32);
        let ox = u16::try_from(chunk_base_x.min(u32::from(u16::MAX))).unwrap_or(u16::MAX);
        let oy = u16::try_from(chunk_base_y.min(u32::from(u16::MAX))).unwrap_or(u16::MAX);
        sub_packets.push(hb_map(ox, oy, 32, 32, &cells));
        map_packets += 1;
        sub_batch_bytes += sub_packets.last().map_or(0, |p| p.len());

        // Send packs/buildings in this chunk
        let mut packs_by_block = Vec::<(i32, Vec<(u8, u16, u16, u16, u8)>)>::new();
        for (code, px, py, cid, off) in state.get_packs_in_chunk_area(ncx, ncy) {
            let Some(block_pos) = state.pack_block_pos(i32::from(px), i32::from(py)) else {
                continue;
            };
            if let Some((_, bucket)) = packs_by_block.iter_mut().find(|(k, _)| *k == block_pos) {
                bucket.push((code, px, py, cid, off));
            } else {
                packs_by_block.push((block_pos, vec![(code, px, py, cid, off)]));
            }
        }
        for (block_pos, packs) in packs_by_block {
            sub_packets.push(hb_packs(block_pos, &packs));
            pack_packets += 1;
            sub_batch_bytes += sub_packets.last().map_or(0, |p| p.len());
        }

        // Also send bots in this chunk
        for entry in &state.active_players {
            let op = entry.value();
            if op.chunk_x() == ncx && op.chunk_y() == ncy {
                sub_packets.push(hb_bot(
                    net_u16_nonneg(*entry.key()),
                    net_u16_nonneg(op.data.x),
                    net_u16_nonneg(op.data.y),
                    net_u8_clamped(op.data.dir, 3),
                    net_u8_clamped(op.data.skin, i32::from(u8::MAX)),
                    net_u16_nonneg(op.data.clan_id.unwrap_or(0)),
                    0,
                ));
                bot_packets += 1;
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

    // For chunks that are no longer visible, clear their existing pack entities.
    for (ocx, ocy) in old_visible {
        if new_visible.contains(&(ocx, ocy)) {
            continue;
        }
        removed_chunks += 1;
        for (_, px, py, _, _) in state.get_packs_in_chunk_area(ocx, ocy) {
            if let Some(block_pos) = state.pack_block_pos(i32::from(px), i32::from(py)) {
                sub_packets.push(hb_packs(block_pos, &[]));
                clear_pack_packets += 1;
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
        tracing::debug!(
            "check_chunk_changed hb pid={pid}: M={} O={} X={} clear_O={} packets={} bytes={}",
            map_packets,
            pack_packets,
            bot_packets,
            clear_pack_packets,
            sub_packets.len(),
            bundle.len()
        );
        send_b_packet(tx, "HB", &bundle);
        sent_batches += 1;
    } else {
        tracing::debug!("check_chunk_changed no hb changes pid={pid}");
    }

    tracing::debug!(
        "check_chunk_changed state pid={pid}: moved from {:?} to ({cx},{cy}), +{added_chunks}/-{removed_chunks}, packets={}",
        last_chunk,
        sent_batches,
    );

    // Update tracked state and spatial chunk index
    if let Some(mut p) = state.active_players.get_mut(&pid) {
        // Update chunk_players index if chunk changed
        if let Some((old_cx, old_cy)) = p.last_chunk {
            if (old_cx, old_cy) != (cx, cy) {
                let should_remove_chunk = state
                    .chunk_players
                    .get_mut(&(old_cx, old_cy))
                    .is_some_and(|mut entry| {
                        entry.retain(|&id| id != pid);
                        entry.is_empty()
                    });
                if should_remove_chunk {
                    state.chunk_players.remove(&(old_cx, old_cy));
                }
                state.chunk_players.entry((cx, cy)).or_default().push(pid);
            }
        }
        p.last_chunk = Some((cx, cy));
        p.visible_chunks = new_visible;
    }
}
