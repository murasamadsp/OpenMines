//! Подгрузка и синхронизация чанков вокруг игрока.
use crate::game::PackOverlay;
use crate::net::session::prelude::*;
use std::collections::HashMap;

pub struct ChunkFanout {
    pub recipients: Vec<crate::game::SessionId>,
    pub data: Vec<u8>,
}

pub fn check_chunk_changed(state: &Arc<GameState>, tx: &dyn PacketSink, pid: PlayerId) {
    for fanout in prepare_chunk_changed(state, tx, pid) {
        state.sessions.fanout(&fanout.recipients, &fanout.data);
    }
}

pub fn prepare_chunk_changed(
    state: &Arc<GameState>,
    tx: &dyn PacketSink,
    pid: PlayerId,
) -> Vec<ChunkFanout> {
    let mut events = Vec::new();
    let ecs = state.ecs_read_profiled("chunk_sync.snapshot");
    let Some(entity) = state.get_player_entity(pid) else {
        return events;
    };
    let (Some(pos), Some(view)) = (
        ecs.get::<crate::game::player::PlayerPosition>(entity),
        ecs.get::<crate::game::player::PlayerView>(entity),
    ) else {
        return events;
    };
    let (cx, cy) = (pos.chunk_x(), pos.chunk_y());
    let last_chunk = view.last_chunk;
    let old_visible = view.visible_chunks.clone();
    if Some((cx, cy)) == last_chunk {
        return events;
    }

    let new_visible = state.visible_chunks_around(cx, cy);
    let old_visible_set: HashSet<(u32, u32)> = old_visible.iter().copied().collect();
    let mut sub_packets: Vec<Vec<u8>> = Vec::new();
    let mut sub_batch_bytes = 0usize;
    let mut sent_batches = 0usize;

    let flush_sub_packets = |sub_packets: &mut Vec<Vec<u8>>,
                             sent_batches: &mut usize,
                             sub_batch_bytes: &mut usize,
                             tx: &dyn PacketSink,
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
        for opid in state.players_in_chunk(ncx, ncy) {
            if opid == pid {
                continue;
            }
            let bot_data = state.get_player_entity(opid).and_then(|entity| {
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

        // BotSpot entities (C# BotSpot: skin=3, tail=1, id=-owner_id)
        for botspot in state.botspots_in_chunk_with_ecs(&ecs, ncx, ncy) {
            // C# `BotSpot.tail => 1` (always 1, unlike Player which checks running).
            // C# casts negative bot_id to u16 (wraps around).
            sub_packets.push(hb_bot(
                botspot.bot_id as u16,
                net_u16_nonneg(botspot.x),
                net_u16_nonneg(botspot.y),
                net_u8_clamped(botspot.dir, 3),
                crate::game::botspot::BotSpotData::SKIN,
                net_u16_nonneg(botspot.clan_id),
                crate::game::botspot::BotSpotData::TAIL,
            ));
            sub_batch_bytes += sub_packets.last().map_or(0, |p| p.len());
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
        for opid in state.players_in_chunk(ocx, ocy) {
            if opid == pid {
                continue;
            }
            sub_packets.push(hb_bot_del(net_u16_nonneg(opid)));
            sub_batch_bytes += sub_packets.last().map_or(0, |p| p.len());
        }

        // Notify BotSpots removal
        for botspot in state.botspots_in_chunk_with_ecs(&ecs, ocx, ocy) {
            sub_packets.push(hb_bot_del(botspot.bot_id as u16));
            sub_batch_bytes += sub_packets.last().map_or(0, |p| p.len());
        }

        // Notify buildings removal — собираем уникальные block_pos, чтобы не
        // слать дубликаты и не триггерить преждевременный flush (иначе клиент
        // снесёт паки в visible-чанках, чей block_pos коллизит, а ре-отправка
        // окажется в отдельном бандле — «паки пропадают»).
        for pack in state.get_packs_in_single_chunk_with_ecs(&ecs, ocx, ocy) {
            if let Some(block_pos) = state.pack_block_pos(i32::from(pack.x), i32::from(pack.y)) {
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
        events.push(ChunkFanout {
            recipients: state.session_ids_in_chunk(ocx, ocy, Some(pid)),
            data: we_del_data,
        });
    }

    // Постройки: группируем ВСЕ видимые паки по block_pos и шлём каждый block_pos
    // ОДНИМ `hb_packs` со всеми его паками. Клиент `RemoveObjectInBlock(block_pos)`
    // чистит весь блок перед добавлением, а `PACKPOS=x+y*chunks_w` коллизит
    // (клетки ~8 апарт делят block_pos) — отправка по одному паку затирала бы
    // соседние («паки пропадают»). Идёт ПОСЛЕ clears ушедших чанков, чтобы
    // перекрыть ошибочную очистку видимого пака с совпавшим block_pos.
    // ВНИМАНИЕ: здесь группируются ТОЛЬКО здания (`get_packs_in_single_chunk`),
    // БЕЗ активных расходников (`consumable_packs`). Это намеренно: пересечение
    // границы чанка сотрёт чужие transient-бумы на клиенте этого игрока, но они
    // живут 1–2с и самовосстановятся. НЕ добавлять сюда частичный сенд расходников
    // — инвариант «один `O` несёт весь блок» держит `gather_block_packs`.
    let mut by_block: HashMap<i32, Vec<PackOverlay>> = HashMap::new();
    for (vcx, vcy) in new_visible.iter().copied() {
        for pack in state.get_packs_in_single_chunk_with_ecs(&ecs, vcx, vcy) {
            if let Some(bp) = state.pack_block_pos(i32::from(pack.x), i32::from(pack.y)) {
                by_block.entry(bp).or_default().push(pack);
            }
        }
    }
    for (bp, packs) in by_block {
        let wire: Vec<(u8, u16, u16, u8, u8)> = packs
            .iter()
            .map(|p| (p.code, p.x, p.y, p.clan, p.off))
            .collect();
        sub_packets.push(hb_packs(bp, &wire));
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

    drop(ecs);

    // Обновляем ECS (view) и chunk player index РАЗДЕЛЬНО, чтобы не держать
    // ecs.write() одновременно с spatial index — иначе deadlock с
    // broadcast_to_nearby, который держит индекс игроков и хочет ecs.read().
    let chunk_update = state
        .modify_player(pid, |ecs, entity| {
            let (ncx, ncy) = {
                let pos = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
                (pos.chunk_x(), pos.chunk_y())
            };
            let mut view = ecs.get_mut::<crate::game::player::PlayerView>(entity)?;
            let prev = view.last_chunk;
            view.last_chunk = Some((ncx, ncy));
            view.visible_chunks = new_visible;
            // `prev == None` — первый вызов после спавна: регистрируем БЕЗ снятия
            // со старого чанка. Прежний `unwrap_or((0,0))` был ядовитым сентинелом:
            // у игрока, заспавненного в чанке (0,0), дефолт совпадал с реальным
            // чанком → «смены нет» → его НИКОГДА не клали в chunk_players →
            // broadcast_to_nearby (идёт по chunk_players) не доставлял ему его же
            // X-пакет. Ручной ход рисуется предсказанием (поэтому работал), а
            // программаторный ход живёт только этим broadcast'ом → бот замирал.
            match prev {
                None => Some((None, (ncx, ncy))),
                Some((ocx, ocy)) if (ocx, ocy) != (ncx, ncy) => {
                    Some((Some((ocx, ocy)), (ncx, ncy)))
                }
                Some(_) => None,
            }
        })
        .flatten();
    // ecs.write() отпущен — безопасно обновляем chunk player index.
    if let Some((old, (ncx, ncy))) = chunk_update {
        if let Some((ocx, ocy)) = old {
            state.unregister_player_from_chunk(pid, ocx, ocy);
        }
        state.register_player_chunk(pid, ncx, ncy);
    }
    events
}

struct BotsRenderSnapshotItem {
    player_id: Option<PlayerId>,
    packet: Vec<u8>,
}

struct BotsRenderObserver {
    due: crate::game::BotsRenderDue,
    tx: Outbox,
    center: (u32, u32),
}

#[derive(Default)]
pub struct BotsRenderBatchResult {
    pub completed: Vec<crate::game::BotsRenderDue>,
    pub deferred: Vec<crate::game::BotsRenderDue>,
    pub observers_sent: usize,
    pub bytes_enqueued: usize,
    pub snapshot_chunks: usize,
}

/// Один immutable ECS/chunk snapshot на весь due batch. Для hotspot все
/// observers переиспользуют одни и те же encoded `X` sub-packets.
pub fn bots_render_batch(
    state: &Arc<GameState>,
    due: Vec<crate::game::BotsRenderDue>,
    byte_budget: usize,
) -> BotsRenderBatchResult {
    let ecs = state.ecs_read_profiled("bots_render.snapshot");
    let mut observers = Vec::with_capacity(due.len());
    let mut unresolved = Vec::new();
    for due in due {
        let Some(entity) = state.get_player_entity(due.player_id) else {
            unresolved.push(due);
            continue;
        };
        let Some(pos) = ecs.get::<crate::game::player::PlayerPosition>(entity) else {
            unresolved.push(due);
            continue;
        };
        let Some(tx) = state.player_sender(due.player_id) else {
            unresolved.push(due);
            continue;
        };
        observers.push(BotsRenderObserver {
            due,
            tx,
            center: (pos.chunk_x(), pos.chunk_y()),
        });
    }

    let mut needed_chunks: HashSet<(u32, u32)> = HashSet::new();
    for observer in &observers {
        needed_chunks.extend(state.visible_chunks_iter(observer.center.0, observer.center.1));
    }

    let mut chunk_cache = HashMap::with_capacity(needed_chunks.len());
    for chunk in needed_chunks {
        let (cx, cy) = chunk;
        let mut items = Vec::new();
        for player_id in state.players_in_chunk(cx, cy) {
            let Some(entity) = state.get_player_entity(player_id) else {
                continue;
            };
            let (Some(pos), Some(player_stats)) = (
                ecs.get::<crate::game::player::PlayerPosition>(entity),
                ecs.get::<crate::game::player::PlayerStats>(entity),
            ) else {
                continue;
            };
            let tail = ecs
                .get::<crate::game::programmator::ProgrammatorState>(entity)
                .map_or(0, |program| u8::from(program.running));
            items.push(BotsRenderSnapshotItem {
                player_id: Some(player_id),
                packet: hb_bot(
                    net_u16_nonneg(player_id),
                    net_u16_nonneg(pos.x),
                    net_u16_nonneg(pos.y),
                    net_u8_clamped(pos.dir, 3),
                    net_u8_clamped(player_stats.skin, 255),
                    net_u16_nonneg(player_stats.clan_id.unwrap_or(0)),
                    tail,
                ),
            });
        }
        for botspot in state.botspots_in_chunk_with_ecs(&ecs, cx, cy) {
            items.push(BotsRenderSnapshotItem {
                player_id: None,
                packet: hb_bot(
                    botspot.bot_id as u16,
                    net_u16_nonneg(botspot.x),
                    net_u16_nonneg(botspot.y),
                    net_u8_clamped(botspot.dir, 3),
                    crate::game::botspot::BotSpotData::SKIN,
                    net_u16_nonneg(botspot.clan_id),
                    crate::game::botspot::BotSpotData::TAIL,
                ),
            });
        }
        chunk_cache.insert(chunk, items);
    }
    drop(ecs);

    let mut result = BotsRenderBatchResult {
        deferred: unresolved,
        snapshot_chunks: chunk_cache.len(),
        ..BotsRenderBatchResult::default()
    };
    let mut observers = observers.into_iter();
    while let Some(observer) = observers.next() {
        let payload_len: usize = state
            .visible_chunks_iter(observer.center.0, observer.center.1)
            .filter_map(|chunk| chunk_cache.get(&chunk))
            .flat_map(|items| items.iter())
            .filter(|item| item.player_id != Some(observer.due.player_id))
            .map(|item| item.packet.len())
            .sum();
        let wire_len = payload_len.saturating_add(7);
        if result.observers_sent > 0 && result.bytes_enqueued.saturating_add(wire_len) > byte_budget
        {
            result.deferred.push(observer.due);
            result.deferred.extend(observers.map(|item| item.due));
            break;
        }

        if payload_len > 0 {
            let mut payload = Vec::with_capacity(payload_len);
            for chunk in state.visible_chunks_iter(observer.center.0, observer.center.1) {
                if let Some(items) = chunk_cache.get(&chunk) {
                    for item in items {
                        if item.player_id != Some(observer.due.player_id) {
                            payload.extend_from_slice(&item.packet);
                        }
                    }
                }
            }
            send_b_packet(&observer.tx, "HB", &payload);
            result.observers_sent += 1;
            result.bytes_enqueued = result.bytes_enqueued.saturating_add(wire_len);
        }
        result.completed.push(observer.due);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    // Регрессия: игрок, заспавненный в чанке (0,0), ОБЯЗАН попасть в
    // chunk_players с первого `check_chunk_changed`. Прежний `unwrap_or((0,0))`
    // совпадал с реальным чанком → регистрации не было → игрок не получал свой
    // же X-broadcast → программаторный ход был невидим.
    #[tokio::test]
    async fn spawn_in_chunk_zero_zero_registers_in_chunk_players() {
        let dir = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_path = dir.join(format!("chunkreg_{}_{}.db", std::process::id(), nonce));
        let _ = std::fs::remove_file(&db_path);
        let database = crate::db::Database::open(&db_path).await.unwrap();
        let player = database
            .create_player("chunk-user", "p", "h")
            .await
            .unwrap();

        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();
        let world_name = format!("chunkreg_world_{}_{}", std::process::id(), nonce);
        let world = crate::world::World::new(&world_name, 2, 2, cell_defs, &dir).unwrap();
        let config = crate::config::Config {
            world_name: world_name.clone(),
            port: 8090,
            world_chunks_w: 2,
            world_chunks_h: 2,
            data_dir: dir.to_string_lossy().to_string(),
            logging: crate::config::LoggingConfig::runtime_baseline(),
            cron: crate::config::CronConfig::runtime_baseline(),
            gameplay: crate::config::GameplayConfig::runtime_baseline(),
        };
        let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config)
            .await
            .unwrap();
        let (tx, _rx) = crate::net::session::outbox::channel();

        // Детерминированно спавним в чанке (0,0): клетка (15,8) → (15>>5,8>>5)=(0,0).
        // На СТАРОМ коде (`last_chunk.unwrap_or((0,0))`) коннект→check_chunk_changed
        // видел ncx,ncy==(0,0)==дефолт → «смены нет» → игрок НЕ регистрировался.
        let mut player = player;
        player.x = 15;
        player.y = 8;
        crate::net::session::player::init::connect_in_tick(&state, &tx, &player, 1);

        let registered = state
            .players_in_chunk(0, 0)
            .contains(&crate::game::player::PlayerId(player.id));
        assert!(
            registered,
            "игрок со спавном в чанке (0,0) обязан попасть в chunk_players на коннекте"
        );

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_v2.map")));
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_durability.map")));
    }

    #[tokio::test]
    async fn bots_render_batch_reuses_snapshot_and_excludes_observer() {
        let dir = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_path = dir.join(format!("bots_batch_{}_{}.db", std::process::id(), nonce));
        let database = crate::db::Database::open(&db_path).await.unwrap();
        let mut first = database.create_player("batch-a", "p", "h").await.unwrap();
        let mut second = database.create_player("batch-b", "p", "h").await.unwrap();
        first.x = 10;
        first.y = 10;
        second.x = 11;
        second.y = 10;

        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();
        let world_name = format!("bots_batch_world_{}_{}", std::process::id(), nonce);
        let world = crate::world::World::new(&world_name, 2, 2, cell_defs, &dir).unwrap();
        let config = crate::config::Config {
            world_name: world_name.clone(),
            port: 8090,
            world_chunks_w: 2,
            world_chunks_h: 2,
            data_dir: dir.to_string_lossy().to_string(),
            logging: crate::config::LoggingConfig::runtime_baseline(),
            cron: crate::config::CronConfig::runtime_baseline(),
            gameplay: crate::config::GameplayConfig::runtime_baseline(),
        };
        let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config)
            .await
            .unwrap();
        let (tx1, mut rx1) = crate::net::session::outbox::channel();
        let (tx2, mut rx2) = crate::net::session::outbox::channel();
        crate::net::session::player::init::connect_in_tick(&state, &tx1, &first, 1);
        crate::net::session::player::init::connect_in_tick(&state, &tx2, &second, 2);
        while rx1.try_recv().is_ok() {}
        while rx2.try_recv().is_ok() {}

        let now = Instant::now();
        let result = bots_render_batch(
            &state,
            vec![
                crate::game::BotsRenderDue {
                    due_at: now,
                    player_id: first.id.into(),
                    session_token: 1,
                },
                crate::game::BotsRenderDue {
                    due_at: now,
                    player_id: second.id.into(),
                    session_token: 2,
                },
            ],
            usize::MAX,
        );

        assert_eq!(result.completed.len(), 2);
        assert!(result.deferred.is_empty());
        assert_eq!(result.observers_sent, 2);
        assert_eq!(result.snapshot_chunks, 4);
        for frame in [rx1.try_recv().unwrap(), rx2.try_recv().unwrap()] {
            let mut encoded = bytes::BytesMut::from(&frame[..]);
            let packet = crate::protocol::Packet::try_decode(&mut encoded)
                .unwrap()
                .unwrap();
            assert_eq!(packet.event_name, *b"HB");
            assert_eq!(packet.payload.len(), 12);
            assert_eq!(packet.payload[0], b'X');
        }

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_v2.map")));
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_road_v2.map")));
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_durability.map")));
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_world.journal")));
    }
}
