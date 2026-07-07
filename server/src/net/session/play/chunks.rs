//! Подгрузка и синхронизация чанков вокруг игрока.
use crate::game::PackOverlay;
use crate::net::session::prelude::*;
use std::collections::HashMap;

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
        if let Some(pids) = state.chunk_players.get(&(ncx, ncy).into()) {
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
        for botspot in state.botspots_in_chunk(ncx, ncy) {
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
        if let Some(pids) = state.chunk_players.get(&(ocx, ocy).into()) {
            for &opid in pids.iter() {
                if opid == pid {
                    continue;
                }
                sub_packets.push(hb_bot_del(net_u16_nonneg(opid)));
                sub_batch_bytes += sub_packets.last().map_or(0, |p| p.len());
            }
        }

        // Notify BotSpots removal
        for botspot in state.botspots_in_chunk(ocx, ocy) {
            sub_packets.push(hb_bot_del(botspot.bot_id as u16));
            sub_batch_bytes += sub_packets.last().map_or(0, |p| p.len());
        }

        // Notify buildings removal — собираем уникальные block_pos, чтобы не
        // слать дубликаты и не триггерить преждевременный flush (иначе клиент
        // снесёт паки в visible-чанках, чей block_pos коллизит, а ре-отправка
        // окажется в отдельном бандле — «паки пропадают»).
        for pack in state.get_packs_in_single_chunk(ocx, ocy) {
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
        state.broadcast_to_nearby_specific_chunk(ocx, ocy, &we_del_data, Some(pid));
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
        for pack in state.get_packs_in_single_chunk(vcx, vcy) {
            if let Some(bp) = state.pack_block_pos(i32::from(pack.x), i32::from(pack.y)) {
                by_block.entry(bp).or_default().push(pack);
            }
        }
    }
    for (bp, packs) in by_block {
        let wire: Vec<(u8, u16, u16, u8, u8)> = packs
            .iter()
            .map(|p| (p.code, p.x, p.y, p.clan, p.charged))
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
    // ecs.write() отпущен — безопасно обновляем chunk_players.
    if let Some((old, (ncx, ncy))) = chunk_update {
        if let Some((ocx, ocy)) = old {
            if let Some(mut e) = state.chunk_players.get_mut(&(ocx, ocy).into()) {
                e.retain(|&id| id != pid);
            }
        }
        state
            .chunk_players
            .entry((ncx, ncy).into())
            .or_default()
            .push(pid);
    }
}

/// Периодический ре-рендер ботов вокруг игрока (1:1 C# `Player.BotsRender`).
/// Шлёт `X` (spawn/update) всех видимых ботов — игроков и `BotSpot` — одним
/// HB-бандлом. Game-tick вызывает каждые 4с: без этого клиентский
/// `RobotsGarbageCollector` (6с без пинга) удаляет простаивающих ботов —
/// они мигают при ходьбе и исчезают в покое. Вызывать ВНЕ `ecs.write()`
/// (берёт `ecs.read()` сам, как `check_chunk_changed`).
pub fn bots_render(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId) {
    let Some((cx, cy)) = state.query_player_opt(pid, |ecs, entity| {
        let pos = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
        Some((pos.chunk_x(), pos.chunk_y()))
    }) else {
        return;
    };

    let mut subs: Vec<Vec<u8>> = Vec::new();
    for (ncx, ncy) in state.visible_chunks_around(cx, cy) {
        // Игроки в чанке (кроме самого наблюдателя).
        if let Some(pids) = state.chunk_players.get(&(ncx, ncy).into()) {
            for &opid in pids.iter() {
                if opid == pid {
                    continue;
                }
                let bot = state.query_player_opt(opid, |ecs, entity| {
                    let p = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
                    let s = ecs.get::<crate::game::player::PlayerStats>(entity)?;
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
                if let Some(bot) = bot {
                    subs.push(bot);
                }
            }
        }

        // BotSpot-сущности (skin/tail константы, id = wrap отрицательного owner).
        for botspot in state.botspots_in_chunk(ncx, ncy) {
            subs.push(hb_bot(
                botspot.bot_id as u16,
                net_u16_nonneg(botspot.x),
                net_u16_nonneg(botspot.y),
                net_u8_clamped(botspot.dir, 3),
                crate::game::botspot::BotSpotData::SKIN,
                net_u16_nonneg(botspot.clan_id),
                crate::game::botspot::BotSpotData::TAIL,
            ));
        }
    }

    // Пустой бандл не шлём (девиация от C#, который шлёт всегда — безвредно:
    // клиент из пустого HB ничего не извлекает, экономим трафик при 1 игроке).
    if !subs.is_empty() {
        let bundle = hb_bundle(subs.as_slice()).1;
        send_b_packet(tx, "HB", &bundle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

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
            logging: crate::config::LoggingConfig::default(),
            cron: crate::config::CronConfig::default(),
            gameplay: crate::config::GameplayConfig::default(),
        };
        let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config)
            .await
            .unwrap();
        let (tx, _rx) = mpsc::unbounded_channel();

        // Детерминированно спавним в чанке (0,0): клетка (15,8) → (15>>5,8>>5)=(0,0).
        // На СТАРОМ коде (`last_chunk.unwrap_or((0,0))`) коннект→check_chunk_changed
        // видел ncx,ncy==(0,0)==дефолт → «смены нет» → игрок НЕ регистрировался.
        let mut player = player;
        player.x = 15;
        player.y = 8;
        crate::net::session::player::init::connect_in_tick(&state, &tx, &player, 1);

        let registered = state.chunk_players.get(&(0, 0).into()).is_some_and(|e| {
            e.value()
                .contains(&crate::game::player::PlayerId(player.id))
        });
        assert!(
            registered,
            "игрок со спавном в чанке (0,0) обязан попасть в chunk_players на коннекте"
        );

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_v2.map")));
        let _ = std::fs::remove_file(dir.join(format!("{world_name}_durability.mapb")));
    }
}
