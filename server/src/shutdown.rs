use crate::game;
use crate::world::WorldProvider as _;

/// Финальное сохранение при остановке: игроки (в одной транзакции с ретраем) +
/// грязные здания (в одной транзакции) + отложенная очередь боксов + flush мира.
#[allow(clippy::significant_drop_tightening)]
pub async fn shutdown_flush(game_state: &std::sync::Arc<game::GameState>) {
    tracing::info!("Shutdown: saving players, buildings, boxes and flushing world...");

    // Игроки — сохраняем все ECS-сущности игроков, включая offline programmator.
    let shutdown_pids: Vec<_> = game_state
        .player_entities
        .iter()
        .map(|e| *e.key())
        .collect();
    let mut player_rows = Vec::with_capacity(shutdown_pids.len());
    for pid in shutdown_pids {
        if let Some(row) = game_state.query_player_opt(pid, |ecs, entity| {
            crate::game::player::extract_player_row(ecs, entity)
        }) {
            player_rows.push(row);
        }
    }

    // Сохраняем пакетно в одной транзакции
    if !player_rows.is_empty() {
        let mut ok = false;
        for attempt in 1..=3u32 {
            match game_state.db.save_players_batch(&player_rows).await {
                Ok(()) => {
                    ok = true;
                    break;
                }
                Err(e) => {
                    tracing::warn!(
                        attempt,
                        error = ?e,
                        "Shutdown players batch save attempt failed"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            }
        }
        if !ok {
            tracing::error!("Shutdown save failed for players batch after 3 attempts");
        }
    }

    // Грязные здания — собираем все здания под ecs-локом
    let dirty_entities: Vec<bevy_ecs::prelude::Entity> = {
        let mut ecs = game_state.ecs.write();
        let mut query = ecs.query::<(bevy_ecs::prelude::Entity, &game::BuildingFlags)>();
        query
            .iter(&ecs)
            .filter_map(|(e, f)| f.dirty.then_some(e))
            .collect()
    };

    let mut building_rows = Vec::with_capacity(dirty_entities.len());
    for entity in dirty_entities {
        let row = game_state.modify_building(entity, |ecs, ent| {
            ecs.get::<game::BuildingFlags>(ent)
                .filter(|f| f.dirty)
                .and_then(|_| crate::game::buildings::extract_building_row(ecs, ent))
        });
        if let Some(r) = row {
            building_rows.push(r);
        }
    }

    // Сохраняем здания пакетно в одной транзакции
    if !building_rows.is_empty() {
        let res = game_state.db.save_buildings_batch(&building_rows).await;
        if let Err(e) = res {
            tracing::error!(error = ?e, "Shutdown buildings batch save failed");
        }
    }

    // Боксы — слить отложенную очередь персистенции (in-memory авторитетно)
    for (pos, op) in game_state.drain_box_persist() {
        let (bx, by): (i32, i32) = pos.into();
        let r = match op {
            None => game_state.db.delete_box_at(bx, by).await,
            Some(crystals) => game_state.db.upsert_box(bx, by, &crystals).await,
        };
        if let Err(e) = r {
            tracing::error!(x = bx, y = by, error = ?e, "Shutdown box persist failed");
        }
    }

    // Ожидание завершения всех фоновых транзакций к БД
    let start_t = std::time::Instant::now();
    while game_state
        .db_pending_tasks
        .load(std::sync::atomic::Ordering::SeqCst)
        > 0
    {
        if start_t.elapsed() > std::time::Duration::from_secs(5) {
            tracing::warn!("Timeout waiting for background DB tasks to complete during shutdown");
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    if let Err(e) = game_state.world.flush() {
        tracing::error!(error = ?e, "Shutdown world flush error");
    }
}
