#![allow(
    clippy::too_many_lines,
    clippy::needless_pass_by_value,
    clippy::option_if_let_else,
    clippy::assigning_clones,
    clippy::items_after_statements,
    clippy::used_underscore_binding,
    clippy::semicolon_if_nothing_returned,
    clippy::missing_panics_doc,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::significant_drop_tightening,
    clippy::map_unwrap_or,
    clippy::manual_let_else,
    clippy::format_push_string,
    clippy::single_match_else,
    clippy::nonminimal_bool,
    clippy::collapsible_if,
    clippy::cast_possible_wrap,
    clippy::redundant_closure_for_method_calls
)]
use crate::game::player::{PlayerPosition, PlayerUI};
use crate::net::session::prelude::*;

const MAP_RADIUS_CHUNKS: i32 = 8;

pub fn render(view: &crate::game::TeleportGuiView) -> Vec<u8> {
    use openmines_common::gui;

    let text = if view.destinations.is_empty() {
        format!(
            "Заряд: {}\nПрочность: {}/{}\n\nНет доступных телепортов поблизости.",
            view.charge, view.hp, view.max_hp
        )
    } else {
        format!(
            "Заряд: {}\nПрочность: {}/{}\n\nДоступные телепорты:",
            view.charge, view.hp, view.max_hp
        )
    };

    let markers: Vec<(i32, i32, String)> = view
        .destinations
        .iter()
        .map(|pos| (pos.0, pos.1, format!("tp:{}:{}", pos.0, pos.1)))
        .collect();
    let destinations = &view.destinations;
    gui! {
        <window title="Тп">
            <text>{text}</text>
            <minimap
                center-x=view.source.0
                center-y=view.source.1
                radius=MAP_RADIUS_CHUNKS
                cell-empty={|x, y| map_tile(view, x, y)}
                markers=markers
            />
            <for each=destinations item=destination>
                <list>
                    <row
                        title={format!("TP {}:{}", destination.0, destination.1)}
                        subtitle="ТЕЛЕПОРТ"
                        action={format!("tp:{}:{}", destination.0, destination.1)}
                    />
                </list>
            </for>
            <buttons>
                <button label="Забрать деньги" action={format!("pack_op:take_money:{}:{}", view.source.0, view.source.1)} />
                <button label="Забрать кристаллы" action={format!("pack_op:take_crys:{}:{}", view.source.0, view.source.1)} />
                <button label="Удалить" action={format!("pack_op:remove:{}:{}", view.source.0, view.source.1)} />
            </buttons>
            <close-button />
        </window>
    }
    .payload()
}

fn map_tile(view: &crate::game::TeleportGuiView, x: i32, y: i32) -> Option<bool> {
    let center_chunk = (view.source.0.div_euclid(32), view.source.1.div_euclid(32));
    let dx = x.div_euclid(32) - center_chunk.0;
    let dy = y.div_euclid(32) - center_chunk.1;
    if dx.abs() > MAP_RADIUS_CHUNKS || dy.abs() > MAP_RADIUS_CHUNKS {
        return None;
    }
    let side = MAP_RADIUS_CHUNKS * 2 + 1;
    let index = (dy + MAP_RADIUS_CHUNKS) * side + dx + MAP_RADIUS_CHUNKS;
    view.map_tiles.get(usize::try_from(index).ok()?).copied()?
}

pub fn apply(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, coords: &str) {
    let parts: Vec<&str> = coords.split(':').collect();
    if parts.len() != 2 {
        return;
    }
    let Ok(dest_x) = parts[0].parse::<i32>() else {
        return;
    };
    let Ok(dest_y) = parts[1].parse::<i32>() else {
        return;
    };

    let Some(dest_view) = state.get_pack_at(dest_x, dest_y) else {
        tracing::warn!(player_id = %pid, destination_x = dest_x, destination_y = dest_y, "TP action: destination not found");
        return;
    };
    if dest_view.pack_type != PackType::Teleport || dest_view.charge <= 0 {
        tracing::warn!(player_id = %pid, destination_x = dest_x, destination_y = dest_y, "TP action: destination not a valid teleport");
        return;
    }

    let src_coords = state.query_player_opt(pid, |ecs, entity| {
        let ui = ecs.get::<PlayerUI>(entity)?;
        let window = ui.current_window.as_deref()?;
        let rest = window.strip_prefix("pack:")?;
        let p: Vec<&str> = rest.split(':').collect();
        if p.len() == 2 {
            Some((p[0].parse::<i32>().ok()?, p[1].parse::<i32>().ok()?))
        } else {
            None
        }
    });
    let Some((src_x, src_y)) = src_coords else {
        tracing::warn!(player_id = %pid, "TP action: player not at a teleport window");
        return;
    };
    let Some(src_view) = state.get_pack_at(src_x, src_y) else {
        return;
    };
    if src_view.pack_type != PackType::Teleport || src_view.charge <= 0 {
        return;
    }

    state.modify_player(pid, |ecs, entity| {
        ecs.get_mut::<PlayerUI>(entity)?.current_window = None;
        Some(())
    });
    let close = gu_close();
    send_u_packet(tx, close.0, &close.1);

    let tp_y = dest_y + 3;
    state.modify_player(pid, |ecs, entity| {
        let mut position = ecs.get_mut::<PlayerPosition>(entity)?;
        position.x = dest_x;
        position.y = tp_y;
        Some(())
    });
    if let Some(entity) = state.get_player_entity(pid) {
        state.schedule_hazard(entity, std::time::Instant::now());
    }
    state.seed_granular_region(dest_x, tp_y);
    state.seed_alive_region(dest_x, tp_y);
    let packet = tp(dest_x, tp_y);
    send_u_packet(tx, packet.0, &packet.1);
    crate::game::logic::chunks::check_chunk_changed(state, tx, pid);

    tracing::info!(player_id = %pid, from_x = src_x, from_y = src_y, to_x = dest_x, to_y = tp_y, "Teleported player");
}
