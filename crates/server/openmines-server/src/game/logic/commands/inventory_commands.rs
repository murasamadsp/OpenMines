//! Typed dispatch for inventory and settings commands.

use super::{
    apply_inventory_result, setting_toggle_effects, settings_open_effects,
    spawn_inventory_building_insert_task,
};
use crate::game::{CommandEffects, GameState, PlayerCommand};
use std::sync::Arc;

pub(super) fn apply_inventory_command(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    command: PlayerCommand,
    due_actions: &mut crate::game::logic::due::DueActionQueue,
) -> CommandEffects {
    match command {
        PlayerCommand::InventoryToggle => apply_inventory_result(
            session_id,
            player_id,
            crate::game::logic::inventory::toggle_inventory(state, player_id),
            "toggle",
        ),
        PlayerCommand::InventoryChoose { payload } => apply_inventory_result(
            session_id,
            player_id,
            crate::game::logic::inventory::choose_inventory(state, player_id, &payload),
            "choose",
        ),
        PlayerCommand::InventoryUse => {
            apply_inventory_use(state, session_id, player_id, due_actions)
        }
        PlayerCommand::ToggleAutoDig => setting_toggle_effects(
            session_id,
            player_id,
            crate::game::logic::settings::toggle_auto_dig(state, player_id),
            "auto-dig",
        ),
        PlayerCommand::ToggleAggression => setting_toggle_effects(
            session_id,
            player_id,
            crate::game::logic::settings::toggle_aggression(state, player_id),
            "aggression",
        ),
        PlayerCommand::SettingsSave { payload } => {
            if !payload.is_empty() {
                tracing::debug!(player_id = %player_id, bytes = payload.len(), "Sett TY payload ignored");
            }
            settings_open_effects(state, session_id, player_id)
        }
        _ => unreachable!("non-inventory command routed to inventory command handler"),
    }
}

fn apply_inventory_use(
    state: &Arc<GameState>,
    session_id: crate::game::SessionId,
    player_id: crate::game::PlayerId,
    due_actions: &mut crate::game::logic::due::DueActionQueue,
) -> CommandEffects {
    if state
        .active_player_entity_for_session(player_id, session_id)
        .is_none()
    {
        return CommandEffects::default();
    }
    let batch = crate::net::session::wire::PacketBatch::default();
    let mut effects = CommandEffects::default();
    if crate::game::logic::heal_inventory::handle_inventory_use_sync_nonbuilding(
        state,
        &batch,
        player_id,
        session_id,
        due_actions,
        &mut effects.broadcasts,
    ) {
        effects.events.push(crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: batch.into_packets(),
        });
        return effects;
    }
    if let Some(placement) =
        crate::game::logic::heal_inventory::prepare_inventory_building_use(state, &batch, player_id)
    {
        spawn_inventory_building_insert_task(state, placement);
    }
    effects.events.push(crate::game::GameEvent::SessionBatch {
        session_id,
        player_id,
        packets: batch.into_packets(),
    });
    effects
}
