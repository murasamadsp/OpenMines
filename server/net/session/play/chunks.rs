//! Подгрузка и синхронизация чанков вокруг игрока.
use crate::net::session::prelude::*;
use std::collections::HashMap;

/// HB-оверлей пака: `(code, x, y, clan, charged)` (как `PackOverlay` в `game`).
type PackEntry = (u8, u16, u16, u8, u8);

pub fn check_chunk_changed(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let p_data = state.query_player_opt(pid, |ecs, entity| {
        let pos = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
        let view = ecs.get::<crate::game::player::PlayerView>(entity)?;
        Some((
            pos.chunk_x(),
            pos.chunk_y(),
            view.last_chunk,
            view.visible_chunks.clone(),
        ))
    });

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
        let ox = u16::try_from((ncx * 32).min(u32::from(u16::MAX))).unwrap_or(u16::MAX);
        let oy = u16::try_from((ncy * 32).min(u32::from(u16::MAX))).unwrap_or(u16::MAX);
        sub_packets.push(hb_map(ox, oy, 32, 32, &cells));
        sub_batch_bytes += sub_packets.last().map_or(0, |p| p.len());

        // Сначала отправляем ботов (игроков), чтобы клиент знал о них до обработки построек
        if let Some(pids) = state.chunk_players.get(&(ncx, ncy)) {
            for &opid in pids.iter() {
                if opid == pid {
                    continue;
                }
                let bot_data = state.query_player_opt(opid, |ecs, entity| {
                    let p = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
                    let s = ecs.get::<crate::game::player::PlayerStats>(entity)?;
                    // tail = 1 when programmator running (C#: Player.tail => programsData.ProgRunning ? 1 : 0)
                    let tail = ecs
                        .get::<crate::game::programmator::ProgrammatorState>(entity)
                        .map_or(0, |ps| u8::from(ps.running));
                    Some(hb_bot(
                        net_u16_nonneg(opid),
                        net_u16_nonneg(p.x),
                        net_u16_nonneg(p.y),
                        net_u8_clamped(p.dir, 3),
                        net_u8_clamped(s.skin, 255),
                        net_u16_nonneg(s.clan_id.unwrap_or(0)),
                        tail,
                    ))
                });
                if let Some(bot) = bot_data {
                    sub_packets.push(bot);
                    sub_batch_bytes += sub_packets.last().map_or(0, |p| p.len());
                }
            }
        }

        // BotSpot entities (C# BotSpot: skin=3, tail=1, id=-owner_id)
        {
            if let Some(botspot_entities) = state.chunk_botspots.get(&(ncx, ncy)) {
                let ecs = state.ecs.read();
                for &botspot_entity in botspot_entities.iter() {
                    if let Some(data) = ecs.get::<crate::game::botspot::BotSpotData>(botspot_entity)
                    {
                        // C# `BotSpot.tail => 1` (always 1, unlike Player which checks running).
                        // C# casts negative bot_id to u16 (wraps around).
                        let wire_id = data.bot_id as u16;
                        sub_packets.push(hb_bot(
                            wire_id,
                            net_u16_nonneg(data.x),
                            net_u16_nonneg(data.y),
                            net_u8_clamped(data.dir, 3),
                            crate::game::botspot::BotSpotData::SKIN,
                            net_u16_nonneg(data.clan_id),
                            crate::game::botspot::BotSpotData::TAIL,
                        ));
                        sub_batch_bytes += sub_packets.last().map_or(0, |p| p.len());
                    }
                }
            }
        }

        // Постройки шлём ПОСЛЕ всех чанков, сгруппированными по block_pos
        // (см. ниже): клиент `RemoveObjectInBlock` чистит ВЕСЬ block_pos, а
        // `PACKPOS=x+y*chunks_w` коллизит (клетки ~8 апарт делят block_pos) —
        // отправка по одному паку затирала бы соседние («паки пропадают»).

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

    let mut cleared_blocks: HashSet<i32> = HashSet::new();
    for (ocx, ocy) in old_visible {
        if new_visible.contains(&(ocx, ocy)) {
            continue;
        }

        // Notify the player to remove bots that are now too far away
        if let Some(pids) = state.chunk_players.get(&(ocx, ocy)) {
            for &opid in pids.iter() {
                if opid == pid {
                    continue;
                }
                sub_packets.push(hb_bot_del(net_u16_nonneg(opid)));
                sub_batch_bytes += sub_packets.last().map_or(0, |p| p.len());
            }
        }

        // Notify BotSpots removal
        if let Some(botspot_entities) = state.chunk_botspots.get(&(ocx, ocy)) {
            let ecs = state.ecs.read();
            for &botspot_entity in botspot_entities.iter() {
                if let Some(data) = ecs.get::<crate::game::botspot::BotSpotData>(botspot_entity) {
                    sub_packets.push(hb_bot_del(data.bot_id as u16));
                    sub_batch_bytes += sub_packets.last().map_or(0, |p| p.len());
                }
            }
        }

        // Notify buildings removal — собираем уникальные block_pos, чтобы не
        // слать дубликаты и не триггерить преждевременный flush (иначе клиент
        // снесёт паки в visible-чанках, чей block_pos коллизит, а ре-отправка
        // окажется в отдельном бандле — «паки пропадают»).
        for (_, px, py, _, _) in state.get_packs_in_single_chunk(ocx, ocy) {
            if let Some(block_pos) = state.pack_block_pos(i32::from(px), i32::from(py)) {
                // Dedup: если два пака в уходящем чанке делят block_pos,
                // слать очистку дважды бессмысленно (и тратит лимит).
                if cleared_blocks.insert(block_pos) {
                    sub_packets.push(hb_packs(block_pos, &[]));
                    sub_batch_bytes += sub_packets.last().map_or(0, |p| p.len());
                }
            }
        }

        // IMPORTANT: Notify players in the OLD chunk that WE left their view
        let we_del = hb_bot_del(net_u16_nonneg(pid));
        let we_del_data = encode_hb_bundle(&hb_bundle(&[we_del]).1);
        state.broadcast_to_nearby_specific_chunk(ocx, ocy, &we_del_data, Some(pid));
    }

    // Постройки: группируем ВСЕ видимые паки по block_pos и шлём каждый block_pos
    // ОДНИМ `hb_packs` со всеми его паками. Клиент `RemoveObjectInBlock(block_pos)`
    // чистит весь блок перед добавлением, а `PACKPOS=x+y*chunks_w` коллизит
    // (клетки ~8 апарт делят block_pos) — отправка по одному паку затирала бы
    // соседние («паки пропадают»). Идёт ПОСЛЕ clears ушедших чанков, чтобы
    // перекрыть ошибочную очистку видимого пака с совпавшим block_pos.
    let mut by_block: HashMap<i32, Vec<PackEntry>> = HashMap::new();
    for (vcx, vcy) in new_visible.iter().copied() {
        for pack in state.get_packs_in_single_chunk(vcx, vcy) {
            if let Some(bp) = state.pack_block_pos(i32::from(pack.1), i32::from(pack.2)) {
                by_block.entry(bp).or_default().push(pack);
            }
        }
    }
    for (bp, packs) in by_block {
        sub_packets.push(hb_packs(bp, &packs));
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
        if let Some(mut e) = state.chunk_players.get_mut(&(ocx, ocy)) {
            e.retain(|&id| id != pid);
        }
        state.chunk_players.entry((ncx, ncy)).or_default().push(pid);
    }
}
