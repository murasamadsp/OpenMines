//! Typed market mutation command helpers.

use crate::game::{CommandEffects, GameState};
use std::sync::Arc;

pub(super) fn apply_market_get_profit(
    state: &Arc<GameState>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    building_x: i32,
    building_y: i32,
) -> CommandEffects {
    let batch = crate::net::session::wire::PacketBatch::default();
    let Some(view) = state.get_pack_at(building_x, building_y) else {
        return CommandEffects::default();
    };
    if view.pack_type != crate::game::structures::buildings::PackType::Market
        || view.owner_id != player_id
    {
        return CommandEffects::default();
    }
    let player_ready = state
        .query_player(player_id, |ecs, entity| {
            ecs.get::<crate::game::player::PlayerStats>(entity)
                .is_some()
                && ecs
                    .get::<crate::game::player::PlayerFlags>(entity)
                    .is_some()
        })
        .unwrap_or(false);
    if !player_ready {
        crate::net::session::wire::send_u_packet(
            &batch,
            "OK",
            &crate::protocol::packets::ok_message("РЫНОК", "Состояние игрока недоступно.").1,
        );
        return session_batch(session_id, player_id, batch);
    }
    let mut amount = 0i64;
    let updated = match crate::game::logic::buildings::modify_pack_with_db(
        state,
        building_x,
        building_y,
        |ecs, entity| {
            let mut storage = ecs
                .get_mut::<crate::game::buildings::BuildingStorage>(entity)
                .expect("BuildingStorage checked before market profit mutation");
            amount = storage.money;
            storage.money = 0;
            true
        },
    ) {
        Ok(updated) => updated,
        Err(e) => {
            tracing::error!(x = building_x, y = building_y, error = %e, "Market profit withdrawal failed");
            crate::net::session::wire::send_u_packet(
                &batch,
                "OK",
                &crate::protocol::packets::ok_message("РЫНОК", "Ошибка снятия прибыли.").1,
            );
            return session_batch(session_id, player_id, batch);
        }
    };
    if !updated {
        crate::net::session::wire::send_u_packet(
            &batch,
            "OK",
            &crate::protocol::packets::ok_message("РЫНОК", "Здание не найдено.").1,
        );
        return session_batch(session_id, player_id, batch);
    }
    if amount > 0
        && let Some(Some((money_now, creds_now))) = state.modify_player(player_id, |ecs, entity| {
            let mut player_stats = ecs.get_mut::<crate::game::player::PlayerStats>(entity)?;
            player_stats.money = player_stats.money.saturating_add(amount);
            let result = (player_stats.money, player_stats.creds);
            ecs.get_mut::<crate::game::player::PlayerFlags>(entity)?
                .dirty = true;
            Some(result)
        })
    {
        crate::net::session::wire::send_u_packet(
            &batch,
            "P$",
            &crate::protocol::packets::money(money_now, creds_now).1,
        );
    }
    crate::game::logic::gui::market_gui::open_market_admin_gui(
        state, &batch, player_id, building_x, building_y,
    );
    session_batch(session_id, player_id, batch)
}

fn session_batch(
    session_id: crate::game::SessionId,
    player_id: crate::game::PlayerId,
    batch: crate::net::session::wire::PacketBatch,
) -> CommandEffects {
    CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: batch.into_packets(),
        }],
        ..CommandEffects::default()
    }
}
