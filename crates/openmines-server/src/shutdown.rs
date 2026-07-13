use crate::game;
use crate::world::WorldProvider as _;

const SHUTDOWN_DB_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const SHUTDOWN_WORLD_FLUSH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Финальное сохранение при остановке: игроки (в одной транзакции с ретраем) +
/// грязные здания (в одной транзакции) + flush мира.
pub async fn shutdown_flush(game_state: &std::sync::Arc<game::GameState>) {
    tracing::info!("Shutdown: saving players and buildings, then flushing world...");

    let player_rows = collect_player_rows(game_state);
    save_players_on_shutdown(game_state, &player_rows).await;

    let building_rows = collect_dirty_building_rows(game_state);
    save_buildings_on_shutdown(game_state, &building_rows).await;

    wait_pending_db_tasks(game_state).await;
    flush_world_on_shutdown(game_state).await;

    tracing::info!("Shutdown: flush pipeline finished");
}

fn collect_player_rows(game_state: &std::sync::Arc<game::GameState>) -> Vec<crate::db::PlayerRow> {
    let shutdown_pids = game_state.player_entity_ids();
    let mut player_rows = Vec::with_capacity(shutdown_pids.len());
    for pid in shutdown_pids {
        if let Some(row) = game_state.query_player_opt(pid, |ecs, entity| {
            crate::game::player::extract_player_row(ecs, entity)
        }) {
            player_rows.push(row);
        }
    }
    player_rows
}

async fn save_players_on_shutdown(
    game_state: &std::sync::Arc<game::GameState>,
    player_rows: &[crate::db::PlayerRow],
) {
    if player_rows.is_empty() {
        tracing::debug!("Shutdown: no players to save");
        return;
    }

    tracing::info!(count = player_rows.len(), "Shutdown: saving players batch");
    let mut ok = false;
    for attempt in 1..=3u32 {
        match tokio::time::timeout(
            SHUTDOWN_DB_TIMEOUT,
            game_state.db.save_players_batch(player_rows),
        )
        .await
        {
            Ok(Ok(())) => {
                ok = true;
                break;
            }
            Ok(Err(e)) => {
                tracing::warn!(
                    attempt,
                    error = ?e,
                    "Shutdown players batch save attempt failed"
                );
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            Err(_) => {
                tracing::warn!(
                    attempt,
                    timeout_ms = SHUTDOWN_DB_TIMEOUT.as_millis(),
                    "Shutdown players batch save attempt timed out"
                );
            }
        }
    }
    if !ok {
        tracing::error!("Shutdown save failed for players batch after 3 attempts");
    }
}

fn collect_dirty_building_rows(
    game_state: &std::sync::Arc<game::GameState>,
) -> Vec<crate::db::BuildingRow> {
    let dirty_entities = game_state.take_dirty_building_entities();

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
    building_rows
}

async fn save_buildings_on_shutdown(
    game_state: &std::sync::Arc<game::GameState>,
    building_rows: &[crate::db::BuildingRow],
) {
    if building_rows.is_empty() {
        tracing::debug!("Shutdown: no dirty buildings to save");
        return;
    }

    tracing::info!(
        count = building_rows.len(),
        "Shutdown: saving buildings batch"
    );
    match tokio::time::timeout(
        SHUTDOWN_DB_TIMEOUT,
        game_state.db.save_buildings_batch(building_rows),
    )
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::error!(error = ?e, "Shutdown buildings batch save failed"),
        Err(_) => tracing::error!(
            timeout_ms = SHUTDOWN_DB_TIMEOUT.as_millis(),
            "Shutdown buildings batch save timed out"
        ),
    }
}

async fn wait_pending_db_tasks(game_state: &std::sync::Arc<game::GameState>) {
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
}

async fn flush_world_on_shutdown(game_state: &std::sync::Arc<game::GameState>) {
    tracing::info!("Shutdown: flushing world mmap layers");
    let world = game_state.world.clone();
    match tokio::time::timeout(
        SHUTDOWN_WORLD_FLUSH_TIMEOUT,
        tokio::task::spawn_blocking(move || world.flush()),
    )
    .await
    {
        Ok(Ok(Ok(_))) => {}
        Ok(Ok(Err(e))) => tracing::error!(error = ?e, "Shutdown world flush error"),
        Ok(Err(e)) => tracing::error!(error = ?e, "Shutdown world flush task failed"),
        Err(_) => tracing::error!(
            timeout_ms = SHUTDOWN_WORLD_FLUSH_TIMEOUT.as_millis(),
            "Shutdown world flush timed out"
        ),
    }
}
