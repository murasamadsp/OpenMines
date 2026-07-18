use crate::game::buildings::{
    BuildingDeletePending, BuildingFlags, BuildingMetadata, BuildingOwnership, BuildingStats,
    BuildingStorage, GridPosition, PackType, PackView,
};
use crate::game::player::{
    PlayerConnection, PlayerFlags, PlayerInventory, PlayerMetadata, PlayerPosition, PlayerStats,
    PlayerUI,
};
use crate::game::{
    BuildingDeleteOperationId, BuildingDeleteOrigin, BuildingDeleteRequest, BuildingDeleteResult,
    BuildingIdentity, GameState, RemovePack, SessionId, WorldPos,
};
use rand::Rng as _;
use std::sync::Arc;

const ITEM_DROP_THRESHOLD: u32 = 40;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuildingDeleteError {
    NotFound,
    StateUnavailable,
    NoRights,
    NotAtObject,
    StaleSession,
    AlreadyPending,
    IdentityMismatch,
    PersistenceFailure,
}

impl BuildingDeleteError {
    pub const fn message(self) -> &'static str {
        match self {
            Self::NotFound => "Объект не найден",
            Self::StateUnavailable => "Состояние здания недоступно",
            Self::NoRights => "Нет прав",
            Self::NotAtObject => "Вы не у объекта",
            Self::StaleSession => "Сессия устарела",
            Self::AlreadyPending => "Удаление уже выполняется",
            Self::IdentityMismatch => "Объект изменился до удаления",
            Self::PersistenceFailure => "Не удалось удалить объект",
        }
    }
}

pub struct InventoryDropEffect {
    pub session_id: Option<SessionId>,
    pub position: WorldPos,
    pub inventory: PlayerInventory,
}

pub struct BuildingDeleteEffects {
    pub view: PackView,
    pub changed_cells: Vec<WorldPos>,
    pub closed_sessions: Vec<SessionId>,
    pub box_position: Option<WorldPos>,
    pub inventory_drop: Option<InventoryDropEffect>,
}

pub enum BuildingDeleteCompletion {
    Applied(BuildingDeleteEffects),
    Rejected {
        origin: Option<BuildingDeleteOrigin>,
        error: BuildingDeleteError,
    },
    Stale,
}

pub fn admit(
    state: &Arc<GameState>,
    remove: RemovePack,
    operation_id: BuildingDeleteOperationId,
) -> Result<BuildingDeleteRequest, BuildingDeleteError> {
    let roll = rand::rng().random_range(1_u32..=100);
    admit_with_roll(state, remove, operation_id, roll)
}

fn admit_with_roll(
    state: &Arc<GameState>,
    remove: RemovePack,
    operation_id: BuildingDeleteOperationId,
    item_drop_roll: u32,
) -> Result<BuildingDeleteRequest, BuildingDeleteError> {
    if let Some(origin) = remove.cause.origin()
        && state.sessions.session_for_player(origin.player_id) != Some(origin.session_id)
    {
        return Err(BuildingDeleteError::StaleSession);
    }

    let entity = state
        .building_entity_at(remove.x, remove.y)
        .ok_or(BuildingDeleteError::NotFound)?;
    let mut ecs = state.ecs_write_profiled("building_delete.admit");
    if ecs.get::<BuildingDeletePending>(entity).is_some() {
        return Err(BuildingDeleteError::AlreadyPending);
    }

    let (view, storage_crystals, dirty_before) = {
        let metadata = ecs
            .get::<BuildingMetadata>(entity)
            .ok_or(BuildingDeleteError::StateUnavailable)?;
        let position = ecs
            .get::<GridPosition>(entity)
            .ok_or(BuildingDeleteError::StateUnavailable)?;
        let ownership = ecs
            .get::<BuildingOwnership>(entity)
            .ok_or(BuildingDeleteError::StateUnavailable)?;
        let building_stats = ecs
            .get::<BuildingStats>(entity)
            .ok_or(BuildingDeleteError::StateUnavailable)?;
        let flags = ecs
            .get::<BuildingFlags>(entity)
            .ok_or(BuildingDeleteError::StateUnavailable)?;
        if position.x != remove.x || position.y != remove.y {
            return Err(BuildingDeleteError::NotFound);
        }
        let view = PackView {
            id: metadata.id,
            pack_type: metadata.pack_type,
            x: position.x,
            y: position.y,
            owner_id: ownership.owner_id,
            clan_id: ownership.clan_id,
            charge: building_stats.charge,
            max_charge: building_stats.max_charge,
            hp: building_stats.hp,
            max_hp: building_stats.max_hp,
        };
        validate_origin(&ecs, state, remove, &view)?;
        let storage_crystals = if view.pack_type == PackType::Storage {
            Some(
                ecs.get::<BuildingStorage>(entity)
                    .ok_or(BuildingDeleteError::StateUnavailable)?
                    .crystals,
            )
        } else {
            None
        };
        (view, storage_crystals, flags.dirty)
    };

    let box_write = destroy_box_write(&view, storage_crystals);
    let inventory_drop_item = remove
        .cause
        .trigger_player_id()
        .filter(|_| should_drop_item(item_drop_roll))
        .and_then(|_| crate::game::logic::items::destroyed_building_drop(view.pack_type));

    ecs.entity_mut(entity)
        .remove::<BuildingFlags>()
        .insert(BuildingDeletePending {
            operation_id,
            dirty_before,
        });
    drop(ecs);

    Ok(BuildingDeleteRequest {
        operation_id,
        expected: BuildingIdentity {
            building_id: view.id,
            x: view.x,
            y: view.y,
        },
        view,
        cause: remove.cause,
        box_write,
        inventory_drop_item,
    })
}

const fn should_drop_item(roll: u32) -> bool {
    roll < ITEM_DROP_THRESHOLD
}

fn validate_origin(
    ecs: &bevy_ecs::prelude::World,
    state: &Arc<GameState>,
    remove: RemovePack,
    view: &PackView,
) -> Result<(), BuildingDeleteError> {
    let Some(origin) = remove.cause.origin() else {
        return Ok(());
    };
    let player_entity = state
        .get_player_entity(origin.player_id)
        .ok_or(BuildingDeleteError::StateUnavailable)?;
    let (player_x, player_y, player_clan) = {
        let position = ecs
            .get::<PlayerPosition>(player_entity)
            .ok_or(BuildingDeleteError::StateUnavailable)?;
        let player_stats = ecs
            .get::<PlayerStats>(player_entity)
            .ok_or(BuildingDeleteError::StateUnavailable)?;
        (position.x, position.y, player_stats.clan_id.unwrap_or(0))
    };
    if view.owner_id != origin.player_id && !(view.clan_id != 0 && view.clan_id == player_clan) {
        return Err(BuildingDeleteError::NoRights);
    }
    let cells = view
        .pack_type
        .building_cells()
        .map_err(|_| BuildingDeleteError::StateUnavailable)?;
    if !cells
        .iter()
        .any(|(dx, dy, _)| view.x + dx == player_x && view.y + dy == player_y)
    {
        return Err(BuildingDeleteError::NotAtObject);
    }
    Ok(())
}

fn destroy_box_write(
    view: &PackView,
    storage_crystals: Option<[i64; 6]>,
) -> Option<crate::db::BoxWrite> {
    let crystals = match view.pack_type {
        PackType::Teleport if view.charge > 0 => [0, 0, 0, 0, i64::from(view.charge), 0],
        PackType::Storage => storage_crystals
            .filter(|values| values.iter().copied().fold(0_i64, i64::saturating_add) > 0)?,
        _ => return None,
    };
    Some(crate::db::BoxWrite {
        x: view.x,
        y: view.y,
        crystals: Some(crystals),
    })
}

pub fn apply_completion(
    state: &Arc<GameState>,
    request: BuildingDeleteRequest,
    result: BuildingDeleteResult,
) -> BuildingDeleteCompletion {
    let Some((entity, pending)) = current_pending_delete(state, &request) else {
        return BuildingDeleteCompletion::Stale;
    };
    match result {
        BuildingDeleteResult::Deleted { .. } => apply_deleted(state, request, entity),
        BuildingDeleteResult::IdentityMismatch => reject_delete(
            state,
            &request,
            entity,
            pending,
            BuildingDeleteError::IdentityMismatch,
        ),
        BuildingDeleteResult::PermanentFailure { message } => {
            tracing::error!(
                building_id = request.expected.building_id,
                x = request.expected.x,
                y = request.expected.y,
                error = message,
                "Building delete permanently rejected by persistence"
            );
            reject_delete(
                state,
                &request,
                entity,
                pending,
                BuildingDeleteError::PersistenceFailure,
            )
        }
    }
}

fn current_pending_delete(
    state: &Arc<GameState>,
    request: &BuildingDeleteRequest,
) -> Option<(bevy_ecs::prelude::Entity, BuildingDeletePending)> {
    let entity = state.building_entity_at(request.expected.x, request.expected.y)?;
    let ecs = state.ecs_read_profiled("building_delete.completion_validate");
    let identity_matches = ecs
        .get::<BuildingMetadata>(entity)
        .is_some_and(|metadata| metadata.id == request.expected.building_id)
        && ecs.get::<GridPosition>(entity).is_some_and(|position| {
            position.x == request.expected.x && position.y == request.expected.y
        });
    let pending = ecs.get::<BuildingDeletePending>(entity).copied()?;
    drop(ecs);
    (identity_matches && pending.operation_id == request.operation_id).then_some((entity, pending))
}

fn reject_delete(
    state: &Arc<GameState>,
    request: &BuildingDeleteRequest,
    entity: bevy_ecs::prelude::Entity,
    pending: BuildingDeletePending,
    error: BuildingDeleteError,
) -> BuildingDeleteCompletion {
    let mut ecs = state.ecs_write_profiled("building_delete.completion_reject");
    if !is_current_delete(&ecs, entity, request) {
        drop(ecs);
        return BuildingDeleteCompletion::Stale;
    }
    ecs.entity_mut(entity)
        .remove::<BuildingDeletePending>()
        .insert(BuildingFlags {
            dirty: pending.dirty_before,
        });
    drop(ecs);
    BuildingDeleteCompletion::Rejected {
        origin: request.cause.origin(),
        error,
    }
}

fn is_current_delete(
    ecs: &bevy_ecs::prelude::World,
    entity: bevy_ecs::prelude::Entity,
    request: &BuildingDeleteRequest,
) -> bool {
    ecs.get::<BuildingMetadata>(entity)
        .is_some_and(|metadata| metadata.id == request.expected.building_id)
        && ecs.get::<GridPosition>(entity).is_some_and(|position| {
            position.x == request.expected.x && position.y == request.expected.y
        })
        && ecs
            .get::<BuildingDeletePending>(entity)
            .is_some_and(|pending| pending.operation_id == request.operation_id)
}

fn apply_deleted(
    state: &Arc<GameState>,
    request: BuildingDeleteRequest,
    entity: bevy_ecs::prelude::Entity,
) -> BuildingDeleteCompletion {
    let Some(changed_cells) = state.remove_building_runtime(&request.view, entity) else {
        return BuildingDeleteCompletion::Stale;
    };
    let (closed_sessions, inventory_drop) = apply_player_effects(state, &request);
    let box_position = apply_box(state, request.box_write.as_ref());
    BuildingDeleteCompletion::Applied(BuildingDeleteEffects {
        view: request.view,
        changed_cells,
        closed_sessions,
        box_position,
        inventory_drop,
    })
}

fn apply_player_effects(
    state: &Arc<GameState>,
    request: &BuildingDeleteRequest,
) -> (Vec<SessionId>, Option<InventoryDropEffect>) {
    let mut ecs = state.ecs_write_profiled("building_delete.completion_apply");
    let closed_sessions = close_pack_windows(&mut ecs, &request.view);
    if request.view.pack_type == PackType::Resp {
        clear_resp_bindings(&mut ecs, request.view.x, request.view.y);
    }
    let inventory_drop = apply_inventory_drop(state, &mut ecs, request);
    drop(ecs);
    (closed_sessions, inventory_drop)
}

fn close_pack_windows(ecs: &mut bevy_ecs::prelude::World, view: &PackView) -> Vec<SessionId> {
    let window_key = format!("pack:{}:{}", view.x, view.y);
    let mut closed_sessions = Vec::new();
    let mut players = ecs.query::<(Option<&PlayerConnection>, Option<&mut PlayerUI>)>();
    for (connection, ui) in players.iter_mut(ecs) {
        if let Some(mut ui) = ui
            && ui.current_window.as_deref() == Some(window_key.as_str())
        {
            ui.current_window = None;
            if let Some(connection) = connection {
                closed_sessions.push(connection.session_id);
            }
        }
    }
    closed_sessions
}

fn clear_resp_bindings(ecs: &mut bevy_ecs::prelude::World, x: i32, y: i32) {
    let mut players = ecs.query::<(&mut PlayerMetadata, Option<&mut PlayerFlags>)>();
    for (mut metadata, flags) in players.iter_mut(ecs) {
        if metadata.resp_x == Some(x) && metadata.resp_y == Some(y) {
            metadata.resp_x = None;
            metadata.resp_y = None;
            if let Some(mut flags) = flags {
                flags.dirty = true;
            }
        }
    }
}

fn apply_inventory_drop(
    state: &Arc<GameState>,
    ecs: &mut bevy_ecs::prelude::World,
    request: &BuildingDeleteRequest,
) -> Option<InventoryDropEffect> {
    let player_id = request.cause.trigger_player_id()?;
    let item_id = request.inventory_drop_item?;
    let entity = state.get_player_entity(player_id)?;
    let session_id = ecs
        .get::<PlayerConnection>(entity)
        .map(|connection| connection.session_id);
    ecs.get::<PlayerFlags>(entity)?;
    let inventory = {
        let mut inventory = ecs.get_mut::<PlayerInventory>(entity)?;
        let count = inventory.items.entry(item_id).or_insert(0);
        *count = count.saturating_add(1);
        inventory.clone()
    };
    ecs.get_mut::<PlayerFlags>(entity)?.dirty = true;
    Some(InventoryDropEffect {
        session_id,
        position: (request.view.x, request.view.y).into(),
        inventory,
    })
}

fn apply_box(state: &Arc<GameState>, write: Option<&crate::db::BoxWrite>) -> Option<WorldPos> {
    let write = write?;
    let crystals = write.crystals?;
    state.put_box_cell_authoritative(write.x, write.y, crystals);
    Some(WorldPos(write.x, write.y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::WorldProvider as _;

    fn remove_at(x: i32, y: i32) -> RemovePack {
        RemovePack {
            x,
            y,
            cause: crate::game::BuildingDeleteCause::Damage {
                trigger_player_id: None,
            },
        }
    }

    async fn insert_test_pack(
        test: &crate::test_support::ServerTestHarness,
        pack_type: PackType,
        charge: i32,
    ) -> (i32, bevy_ecs::prelude::Entity) {
        let extra = crate::db::BuildingExtra {
            charge,
            max_charge: 100,
            cost: 10,
            hp: 100,
            max_hp: 100,
            money_inside: 0,
            crystals_inside: [0; 6],
            items_inside: std::collections::HashMap::new(),
            craft_recipe_id: None,
            craft_num: 0,
            craft_end_ts: 0,
            craft_ready: false,
            clanzone: 0,
        };
        let type_code = match pack_type {
            PackType::Resp => "R",
            PackType::Teleport => "T",
            _ => panic!("unsupported test pack type"),
        };
        test.state
            .insert_building_runtime(&crate::game::BuildingInsertSpec {
                type_code,
                pack_type,
                x: 10,
                y: 10,
                owner_id: crate::game::PlayerId(test.player.id),
                clan_id: 0,
                extra: &extra,
            })
            .await
            .unwrap()
    }

    async fn persist_delete(
        state: &Arc<GameState>,
        request: &BuildingDeleteRequest,
    ) -> BuildingDeleteResult {
        match state
            .db
            .apply_building_delete(&crate::db::BuildingDeleteWrite {
                building_id: request.expected.building_id,
                x: request.expected.x,
                y: request.expected.y,
                clear_resp_bindings: request.view.pack_type == PackType::Resp,
                box_write: request.box_write.clone(),
            })
            .await
            .unwrap()
        {
            crate::db::BuildingDeleteOutcome::Deleted {
                cleared_resp_bindings,
            } => BuildingDeleteResult::Deleted {
                cleared_resp_bindings,
            },
            crate::db::BuildingDeleteOutcome::IdentityMismatch => {
                BuildingDeleteResult::IdentityMismatch
            }
        }
    }

    #[test]
    fn item_drop_roll_keeps_reference_one_to_thirty_nine_range() {
        for (roll, expected) in [(1, true), (39, true), (40, false), (100, false)] {
            assert_eq!(should_drop_item(roll), expected);
        }
    }

    #[tokio::test]
    async fn damage_trigger_does_not_require_session_ownership_or_proximity() {
        let test = crate::test_support::ServerTestHarness::new(
            "building_delete_damage_authorization",
            "damage-building-owner",
        )
        .await;
        let (_, entity) = insert_test_pack(&test, PackType::Resp, 100).await;
        let trigger_player_id = crate::game::PlayerId(i32::MAX);

        let request = admit_with_roll(
            &test.state,
            RemovePack {
                x: 10,
                y: 10,
                cause: crate::game::BuildingDeleteCause::Damage {
                    trigger_player_id: Some(trigger_player_id),
                },
            },
            BuildingDeleteOperationId::new(3),
            100,
        )
        .expect("damage attribution must not become player-request authorization");

        assert_eq!(request.cause.trigger_player_id(), Some(trigger_player_id));
        assert!(
            test.state
                .ecs
                .read()
                .get::<BuildingDeletePending>(entity)
                .is_some()
        );
    }

    #[tokio::test]
    async fn player_request_still_requires_rights_and_proximity() {
        let test = crate::test_support::ServerTestHarness::new(
            "building_delete_player_authorization",
            "request-building-owner",
        )
        .await;
        insert_test_pack(&test, PackType::Resp, 100).await;

        let owner_id = crate::game::PlayerId(test.player.id);
        let owner_session_id = SessionId::new(101);
        let _owner_receiver = test.connect(owner_session_id.get());
        test.state
            .modify_player(owner_id, |ecs, entity| {
                let mut position = ecs.get_mut::<PlayerPosition>(entity).unwrap();
                position.x = 0;
                position.y = 0;
            })
            .unwrap();

        let owner_request = RemovePack {
            x: 10,
            y: 10,
            cause: crate::game::BuildingDeleteCause::PlayerRequest(BuildingDeleteOrigin {
                session_id: owner_session_id,
                player_id: owner_id,
            }),
        };
        assert!(matches!(
            admit_with_roll(
                &test.state,
                owner_request,
                BuildingDeleteOperationId::new(4),
                100,
            ),
            Err(BuildingDeleteError::NotAtObject)
        ));

        let attacker = test.create_player("request-building-attacker").await;
        let attacker_id = crate::game::PlayerId(attacker.id);
        let attacker_session_id = SessionId::new(102);
        let (_attacker_outbox, _attacker_receiver) =
            test.connect_player_with_outbox(&attacker, attacker_session_id.get());
        test.state
            .modify_player(attacker_id, |ecs, entity| {
                let mut position = ecs.get_mut::<PlayerPosition>(entity).unwrap();
                position.x = 10;
                position.y = 10;
            })
            .unwrap();

        let attacker_request = RemovePack {
            x: 10,
            y: 10,
            cause: crate::game::BuildingDeleteCause::PlayerRequest(BuildingDeleteOrigin {
                session_id: attacker_session_id,
                player_id: attacker_id,
            }),
        };
        assert!(matches!(
            admit_with_roll(
                &test.state,
                attacker_request,
                BuildingDeleteOperationId::new(5),
                100,
            ),
            Err(BuildingDeleteError::NoRights)
        ));
    }

    #[tokio::test]
    async fn rejected_delete_restores_exact_dirty_state() {
        let test = crate::test_support::ServerTestHarness::new(
            "building_delete_unfreeze",
            "delete-unfreeze",
        )
        .await;
        let (_, entity) = insert_test_pack(&test, PackType::Resp, 100).await;
        test.state
            .ecs
            .write()
            .get_mut::<BuildingFlags>(entity)
            .unwrap()
            .dirty = true;
        let request = admit_with_roll(
            &test.state,
            remove_at(10, 10),
            BuildingDeleteOperationId::new(7),
            100,
        )
        .unwrap();

        assert!(test.state.get_pack_at(10, 10).is_none());
        assert!(matches!(
            apply_completion(&test.state, request, BuildingDeleteResult::IdentityMismatch),
            BuildingDeleteCompletion::Rejected {
                error: BuildingDeleteError::IdentityMismatch,
                ..
            }
        ));
        let ecs = test.state.ecs.read();
        assert!(ecs.get::<BuildingDeletePending>(entity).is_none());
        assert!(
            ecs.get::<BuildingFlags>(entity)
                .is_some_and(|flags| flags.dirty)
        );
        drop(ecs);
        assert!(test.state.get_pack_at(10, 10).is_some());
    }

    #[tokio::test]
    async fn stale_operation_cannot_delete_reused_runtime_identity() {
        let test =
            crate::test_support::ServerTestHarness::new("building_delete_aba", "delete-aba").await;
        let (_, entity) = insert_test_pack(&test, PackType::Resp, 100).await;
        let cell_before = test.state.world.get_cell_typed(10, 10);
        let request = admit_with_roll(
            &test.state,
            remove_at(10, 10),
            BuildingDeleteOperationId::new(11),
            100,
        )
        .unwrap();
        test.state
            .ecs
            .write()
            .get_mut::<BuildingDeletePending>(entity)
            .unwrap()
            .operation_id = BuildingDeleteOperationId::new(12);

        assert!(matches!(
            apply_completion(
                &test.state,
                request,
                BuildingDeleteResult::Deleted {
                    cleared_resp_bindings: 0
                }
            ),
            BuildingDeleteCompletion::Stale
        ));
        assert_eq!(test.state.building_entity_at(10, 10), Some(entity));
        assert_eq!(test.state.world.get_cell_typed(10, 10), cell_before);
    }

    #[tokio::test]
    async fn successful_delete_returns_complete_ordered_effects() {
        let test = crate::test_support::ServerTestHarness::new(
            "building_delete_effects",
            "delete-effects",
        )
        .await;
        insert_test_pack(&test, PackType::Teleport, 7).await;
        let request = admit_with_roll(
            &test.state,
            remove_at(10, 10),
            BuildingDeleteOperationId::new(21),
            100,
        )
        .unwrap();
        let expected_cells = request
            .view
            .pack_type
            .building_cells()
            .unwrap()
            .into_iter()
            .map(|(dx, dy, _)| WorldPos(request.view.x + dx, request.view.y + dy))
            .collect::<Vec<_>>();
        let result = persist_delete(&test.state, &request).await;
        let effects = crate::game::logic::commands::apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::BuildingDeleted { request, result },
        );

        assert!(effects.events.is_empty());
        assert!(effects.saves.is_empty());
        assert_eq!(effects.broadcasts.len(), expected_cells.len() + 2);
        for (effect, expected) in effects.broadcasts.iter().zip(&expected_cells) {
            assert!(
                matches!(effect, crate::game::BroadcastEffect::CellUpdate(pos) if pos == expected)
            );
        }
        assert!(matches!(
            effects.broadcasts.get(expected_cells.len()),
            Some(crate::game::BroadcastEffect::BlockUpdate(WorldPos(10, 10)))
        ));
        assert!(matches!(
            effects.broadcasts.last(),
            Some(crate::game::BroadcastEffect::CellUpdate(WorldPos(10, 10)))
        ));
        assert!(test.state.building_entity_at(10, 10).is_none());
        assert_eq!(
            test.state.world.get_cell_typed(10, 10),
            crate::world::CellType(crate::world::cells::cell_type::BOX)
        );
    }
}
