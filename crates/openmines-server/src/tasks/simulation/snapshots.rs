//! Periodic admission of dirty ECS snapshots to the persistence owner.

use super::TickPendingWork;
use crate::game::GameState;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(super) const PLAYER_DIRTY_FLUSH_INTERVAL: Duration = Duration::from_secs(10);
pub(super) const BUILDING_DIRTY_FLUSH_INTERVAL: Duration = Duration::from_secs(45);

pub(super) fn flush_due_dirty_snapshots(
    state: &Arc<GameState>,
    persistence: &crate::persistence::PersistenceHandle,
    pending: &mut TickPendingWork,
    now: Instant,
) -> bool {
    let player_due = now >= pending.next_player_flush;
    if player_due {
        pending.next_player_flush = now + PLAYER_DIRTY_FLUSH_INTERVAL;
        let accepted = flush_dirty_players_once(state, persistence);
        if accepted > 0 {
            tracing::debug!(accepted, "Periodic player snapshots admitted");
        }
    }
    let building_due = now >= pending.next_building_flush;
    if building_due {
        pending.next_building_flush = now + BUILDING_DIRTY_FLUSH_INTERVAL;
        let accepted = flush_dirty_buildings_once(state, persistence);
        if accepted > 0 {
            tracing::debug!(accepted, "Periodic building snapshots admitted");
        }
    }
    player_due || building_due
}

pub(super) fn flush_dirty_players_once(
    state: &Arc<GameState>,
    persistence: &crate::persistence::PersistenceHandle,
) -> usize {
    let mut dirty_entities = state.take_dirty_player_entities();
    let mut accepted = 0usize;
    while let Some((entity, incarnation)) = dirty_entities.pop() {
        let permit = match persistence.try_reserve(crate::game::SaveKind::Player) {
            Ok(permit) => permit,
            Err(crate::persistence::PersistenceAdmissionError::Full) => {
                dirty_entities.push((entity, incarnation));
                state.requeue_dirty_player_entities(dirty_entities);
                break;
            }
            Err(crate::persistence::PersistenceAdmissionError::Closed) => {
                panic!("persistence worker closed during periodic player flush");
            }
        };
        let row = state.snapshot_dirty_player(entity, incarnation);
        if let Some(row) = row {
            permit.publish(crate::game::SaveCommand::Player { row: Box::new(row) });
            accepted = accepted.saturating_add(1);
        }
    }
    accepted
}

pub(super) fn flush_dirty_buildings_once(
    state: &Arc<GameState>,
    persistence: &crate::persistence::PersistenceHandle,
) -> usize {
    let mut dirty_entities = state.take_dirty_building_entities();
    let mut accepted = 0usize;
    while let Some(entity) = dirty_entities.pop() {
        let permit = match persistence.try_reserve(crate::game::SaveKind::Building) {
            Ok(permit) => permit,
            Err(crate::persistence::PersistenceAdmissionError::Full) => {
                dirty_entities.push(entity);
                state.requeue_dirty_building_entities(dirty_entities);
                break;
            }
            Err(crate::persistence::PersistenceAdmissionError::Closed) => {
                panic!("persistence worker closed during periodic building flush");
            }
        };
        let row = state.modify_building(entity, |ecs, entity| {
            if !ecs
                .get::<crate::game::BuildingFlags>(entity)
                .is_some_and(|flags| flags.dirty)
            {
                return None;
            }
            let row = crate::game::buildings::extract_building_row(ecs, entity)?;
            ecs.get_mut::<crate::game::BuildingFlags>(entity)?.dirty = false;
            Some(row)
        });
        if let Some(row) = row {
            permit.publish(crate::game::SaveCommand::Building { row: Box::new(row) });
            accepted = accepted.saturating_add(1);
        }
    }
    accepted
}
