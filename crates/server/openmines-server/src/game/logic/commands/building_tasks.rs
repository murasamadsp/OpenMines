//! Async persistence tasks for building placement.

use std::sync::Arc;

use crate::game::GameState;

pub fn spawn_inventory_building_insert_task(
    state: &Arc<GameState>,
    placement: crate::game::logic::contracts::InventoryBuildingPlacement,
) {
    let Some(session_id) = state.sessions.session_for_player(placement.owner_id) else {
        return;
    };
    let task_state = state.clone();
    super::spawn_session_async_task(state, "inventory_building_insert", async move {
        let inserted = task_state
            .db
            .insert_building(
                &placement.type_code,
                placement.x,
                placement.y,
                placement.owner_id.into(),
                placement.clan_id,
                &placement.extra,
            )
            .await;
        match inserted {
            Ok(db_id) => {
                task_state
                    .enqueue_internal(
                        placement.owner_id,
                        session_id,
                        crate::game::PlayerCommand::ApplyInventoryBuildingPlaced {
                            placement,
                            db_id,
                        },
                    )
                    .await;
            }
            Err(e) => {
                tracing::error!(
                    player_id = %placement.owner_id,
                    x = placement.x,
                    y = placement.y,
                    pack_type = ?placement.pack_type,
                    error = ?e,
                    "DB insert failed for inventory building placement"
                );
                task_state
                    .enqueue_internal(
                        placement.owner_id,
                        session_id,
                        crate::game::PlayerCommand::InventoryBuildingPlacementFailed,
                    )
                    .await;
            }
        }
    });
}

pub fn spawn_paid_building_insert_task(
    state: &Arc<GameState>,
    placement: crate::game::logic::contracts::PaidBuildingPlacement,
) {
    let Some(session_id) = state.sessions.session_for_player(placement.owner_id) else {
        return;
    };
    let task_state = state.clone();
    super::spawn_session_async_task(state, "paid_building_insert", async move {
        let inserted = task_state
            .db
            .insert_building(
                &placement.type_code,
                placement.x,
                placement.y,
                placement.owner_id.into(),
                placement.building_clan_id,
                &placement.extra,
            )
            .await;
        match inserted {
            Ok(db_id) => {
                task_state
                    .enqueue_internal(
                        placement.owner_id,
                        session_id,
                        crate::game::PlayerCommand::ApplyPaidBuildingPlaced { placement, db_id },
                    )
                    .await;
            }
            Err(e) => {
                tracing::error!(
                    player_id = %placement.owner_id,
                    x = placement.x,
                    y = placement.y,
                    pack_type = ?placement.pack_type,
                    error = ?e,
                    "DB insert failed for paid building placement"
                );
                task_state
                    .enqueue_internal(
                        placement.owner_id,
                        session_id,
                        crate::game::PlayerCommand::RefundPaidBuildingPlacement {
                            cost: placement.cost,
                        },
                    )
                    .await;
            }
        }
    });
}
