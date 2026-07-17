#![allow(
    warnings,
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

use crate::game::buildings::{
    BuildingCrafting, BuildingFlags, BuildingOwnership, BuildingStats, BuildingStorage,
};
use crate::game::crafting;
use crate::game::logic::buildings::{broadcast_pack_update, modify_pack_with_db};
use crate::game::player::{PlayerFlags, PlayerInventory, PlayerPosition, PlayerStats, PlayerUI};
use crate::net::session::outbound::inventory_sync::send_inventory;
use crate::net::session::prelude::*;

// ─── Crafter GUI ──────────────────────────────────────────────────────────

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn send_crafter_action_error(tx: &Outbox) {
    send_u_packet(tx, "OK", &ok_message("КРАФТЕР", "Некорректное действие.").1);
}

pub fn send_crafter_state_error(tx: &Outbox) {
    send_u_packet(
        tx,
        "OK",
        &ok_message("КРАФТЕР", "Состояние крафтера недоступно.").1,
    );
}

/// Open Crafter GUI: if craft in progress show progress, else show recipe list.
/// C# ref: `Crafter.GUIWin` -> `StaticSystem.FilledPage` / `GlobalFirstPage`.
pub fn open_crafter_gui(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, view: &PackView) {
    if view.owner_id != pid {
        return;
    }

    let craft_state = state.query_building_opt(view.x, view.y, |ecs, entity| {
        let c = ecs.get::<BuildingCrafting>(entity)?;
        Some((c.recipe_id, c.num, c.end_ts))
    });

    let Some((recipe_id, num, end_ts)) = craft_state else {
        return;
    };

    if let Some(rid) = recipe_id {
        show_crafter_progress(tx, view, rid, num, end_ts);
    } else {
        show_crafter_recipes(tx, view);
    }

    state.modify_player(pid, |ecs, entity| {
        if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) {
            ui.current_window = Some(format!("pack:{}:{}", view.x, view.y));
        }
        Some(())
    });
}

fn show_crafter_progress(tx: &Outbox, view: &PackView, recipe_id: i32, num: i32, end_ts: i64) {
    let now = now_ts();
    let recipe = crafting::recipe_by_id(recipe_id);
    let recipe_name = recipe.map_or("?", |r| r.title);

    let done = now >= end_ts;
    let progress = if done {
        100
    } else {
        let total_sec = recipe.map_or(1, |r| i64::from(r.time_sec) * i64::from(num));
        let start_ts = end_ts - total_sec;
        let elapsed = now - start_ts;
        ((elapsed * 100) / total_sec.max(1)).clamp(0, 99) as i32
    };

    let bar_filled = progress / 2;
    let bar_empty = 50 - bar_filled;
    let bar = format!(
        "{}{}",
        "|".repeat(bar_filled as usize),
        "-".repeat(bar_empty as usize)
    );

    let status = if done {
        "ГОТОВО".to_string()
    } else {
        let remain = end_ts - now;
        format!("осталось {remain}с")
    };

    let text = format!("Крафт: {recipe_name} x{num}\n\n[{bar}] {progress}%\n{status}");

    use crate::game::logic::horb::{Button, Horb};
    let mut win = Horb::new("Крафтер").text(text);
    if done {
        win = win.button(Button::new(
            "Забрать",
            format!("craft_claim:{}:{}", view.x, view.y),
        ));
    }
    win.close_button().send_raw(tx);
}

fn show_crafter_recipes(tx: &Outbox, view: &PackView) {
    let recipes = crafting::recipes();
    let crys_names = ["зель", "синь", "крась", "фиоль", "бель", "голь"];

    let mut text = String::from("Выберите рецепт:\n");
    use crate::game::logic::horb::{Button, Horb};
    let mut win = Horb::new("Крафтер");

    for r in recipes {
        let cost_str: Vec<String> = r
            .cost_crys
            .iter()
            .map(|c| {
                let name = crys_names.get(c.id as usize).unwrap_or(&"?");
                format!("{name}x{}", c.num)
            })
            .collect();
        let cost_display = if cost_str.is_empty() {
            String::new()
        } else {
            format!(" ({})", cost_str.join("+"))
        };

        text.push_str(&format!(
            "\n- {} x{} - {}с{}",
            r.title, r.result.num, r.time_sec, cost_display
        ));

        win = win.button(Button::new(
            r.title,
            format!("craft_recipe:{}:{}:{}", r.id, view.x, view.y),
        ));
    }

    win.text(text)
        .button(Button::new(
            "Удалить",
            format!("pack_op:remove:{}:{}", view.x, view.y),
        ))
        .close_button()
        .send_raw(tx);
}

/// Show recipe details + Start button.
/// Called from `craft_recipe:{id}:{x}:{y}` but `handle_complex_button` parses
/// only the prefix `craft_recipe:` and passes the rest as a string.
pub fn handle_craft_recipe_view(state: &Arc<GameState>, tx: &Outbox, _pid: PlayerId, args: &str) {
    let _ = state;
    let parts: Vec<&str> = args.split(':').collect();
    if parts.len() < 3 {
        send_crafter_action_error(tx);
        return;
    }
    let (recipe_id, bx, by) = match (
        parts[0].parse::<i32>(),
        parts[1].parse::<i32>(),
        parts[2].parse::<i32>(),
    ) {
        (Ok(recipe_id), Ok(bx), Ok(by)) => (recipe_id, bx, by),
        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
            tracing::warn!(action = args, error = ?e, "Invalid craft recipe action");
            send_crafter_action_error(tx);
            return;
        }
    };

    let Some(recipe) = crafting::recipe_by_id(recipe_id) else {
        return;
    };

    let crys_names = ["зель", "синь", "крась", "фиоль", "бель", "голь"];

    let mut cost_lines = String::new();
    for c in recipe.cost_crys {
        let name = crys_names.get(c.id as usize).unwrap_or(&"?");
        cost_lines.push_str(&format!("  {name} x{}\n", c.num));
    }
    for c in recipe.cost_res {
        cost_lines.push_str(&format!("  предмет#{} x{}\n", c.id, c.num));
    }

    let text = format!(
        "Рецепт: {}\nРезультат: x{}\nВремя: {}с\n\nСтоимость:\n{}",
        recipe.title, recipe.result.num, recipe.time_sec, cost_lines
    );

    use crate::game::logic::horb::{Button, Horb};
    Horb::new("Крафтер")
        .text(text)
        .button(Button::new(
            "Запустить (x1)",
            format!("craft_start:{recipe_id}:1:{bx}:{by}"),
        ))
        .close_button()
        .send_raw(tx);
}

/// Start crafting: deduct resources, set timer.
/// Button format: `craft_start:{recipe_id}:{num}:{x}:{y}`
pub fn handle_craft_start(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, args: &str) {
    let parts: Vec<&str> = args.split(':').collect();
    if parts.len() < 4 {
        send_crafter_action_error(tx);
        return;
    }
    let (recipe_id, num, bx, by) = match (
        parts[0].parse::<i32>(),
        parts[1].parse::<i32>(),
        parts[2].parse::<i32>(),
        parts[3].parse::<i32>(),
    ) {
        (Ok(recipe_id), Ok(num), Ok(bx), Ok(by)) => (recipe_id, num.max(1), bx, by),
        (Err(e), _, _, _) | (_, Err(e), _, _) | (_, _, Err(e), _) | (_, _, _, Err(e)) => {
            tracing::warn!(player_id = %pid, action = args, error = ?e, "Invalid craft start action");
            send_crafter_action_error(tx);
            return;
        }
    };

    let Some(recipe) = crafting::recipe_by_id(recipe_id) else {
        return;
    };

    let Some(view) = state.get_pack_at(bx, by) else {
        return;
    };
    if view.pack_type != PackType::Craft || view.owner_id != pid {
        return;
    }

    let standing_on_crafter = state
        .query_player_opt(pid, |ecs, entity| {
            let pos = ecs.get::<PlayerPosition>(entity)?;
            Some(pos.x == bx && pos.y == by)
        })
        .unwrap_or(false);
    if !standing_on_crafter {
        send_u_packet(tx, "OK", &ok_message("Недостаточно ресов", "...").1);
        return;
    }

    let craft_state = state.query_building_opt(bx, by, |ecs, entity| {
        let c = ecs.get::<BuildingCrafting>(entity)?;
        Some(c.recipe_id.is_some())
    });
    let Some(already_crafting) = craft_state else {
        tracing::error!(
            x = bx,
            y = by,
            "Building crafting component missing for craft start"
        );
        send_crafter_action_error(tx);
        return;
    };
    if already_crafting {
        send_u_packet(tx, "OK", &ok_message("Крафтер", "Крафт уже запущен").1);
        return;
    }

    let deducted = state
        .modify_player(pid, |ecs, entity| {
            if ecs.get::<PlayerStats>(entity).is_none()
                || ecs.get::<PlayerFlags>(entity).is_none()
                || (!recipe.cost_res.is_empty() && ecs.get::<PlayerInventory>(entity).is_none())
            {
                send_crafter_state_error(tx);
                return None;
            }
            {
                let pstats = ecs.get::<PlayerStats>(entity)?;
                for c in recipe.cost_crys {
                    if pstats.crystals[c.id as usize] < i64::from(c.num) * i64::from(num) {
                        return Some(false);
                    }
                }
                if !recipe.cost_res.is_empty() {
                    let inv = ecs.get::<PlayerInventory>(entity)?;
                    for c in recipe.cost_res {
                        let have = inv.items.get(&c.id).copied().unwrap_or(0);
                        if have < c.num * num {
                            return Some(false);
                        }
                    }
                }
            }

            {
                let mut pstats = ecs.get_mut::<PlayerStats>(entity)?;
                for c in recipe.cost_crys {
                    pstats.crystals[c.id as usize] -= i64::from(c.num) * i64::from(num);
                }
                send_u_packet(tx, "@B", &basket(&pstats.crystals, 1).1);
            }

            if !recipe.cost_res.is_empty() {
                let mut inv = ecs.get_mut::<PlayerInventory>(entity)?;
                for c in recipe.cost_res {
                    let entry = inv.items.entry(c.id).or_insert(0);
                    *entry -= c.num * num;
                }
                send_inventory(tx, &mut inv);
            }
            let mut flags = ecs.get_mut::<PlayerFlags>(entity)?;
            flags.dirty = true;

            Some(true)
        })
        .flatten();

    let Some(deducted) = deducted else {
        return;
    };
    if !deducted {
        send_u_packet(tx, "OK", &ok_message("Крафтер", "Недостаточно ресурсов").1);
        return;
    }

    let end_ts = now_ts() + i64::from(recipe.time_sec) * i64::from(num);
    let updated = match modify_pack_with_db(state, bx, by, |ecs, entity| {
        if let Some(mut c) = ecs.get_mut::<BuildingCrafting>(entity) {
            c.recipe_id = Some(recipe_id);
            c.num = num;
            c.end_ts = end_ts;
            c.ready = false;
            true
        } else {
            false
        }
    }) {
        Ok(updated) => updated,
        Err(e) => {
            tracing::error!(x = bx, y = by, error = %e, "Craft start failed after resource deduction");
            false
        }
    };
    if !updated {
        state.modify_player(pid, |ecs, entity| {
            if ecs.get::<PlayerStats>(entity).is_none()
                || ecs.get::<PlayerFlags>(entity).is_none()
                || (!recipe.cost_res.is_empty() && ecs.get::<PlayerInventory>(entity).is_none())
            {
                send_crafter_state_error(tx);
                return None;
            }
            {
                let mut pstats = ecs.get_mut::<PlayerStats>(entity)?;
                for c in recipe.cost_crys {
                    pstats.crystals[c.id as usize] += i64::from(c.num) * i64::from(num);
                }
                send_u_packet(tx, "@B", &basket(&pstats.crystals, 1).1);
            }

            if !recipe.cost_res.is_empty() {
                let mut inv = ecs.get_mut::<PlayerInventory>(entity)?;
                for c in recipe.cost_res {
                    let entry = inv.items.entry(c.id).or_insert(0);
                    *entry += c.num * num;
                }
                send_inventory(tx, &mut inv);
            }

            let mut f = ecs.get_mut::<PlayerFlags>(entity)?;
            f.dirty = true;
            Some(())
        });
        send_crafter_action_error(tx);
        return;
    }

    if let Some(entity) = state.building_entity_at(bx, by) {
        state.schedule_crafting_completion(entity, end_ts);
    }

    broadcast_pack_update(state, &view);
    show_crafter_progress(tx, &view, recipe_id, num, end_ts);
}

/// Claim finished craft. Button format: `craft_claim:{x}:{y}`
pub fn handle_craft_claim(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, args: &str) {
    let parts: Vec<&str> = args.split(':').collect();
    if parts.len() < 2 {
        send_crafter_action_error(tx);
        return;
    }
    let (bx, by) = match (parts[0].parse::<i32>(), parts[1].parse::<i32>()) {
        (Ok(bx), Ok(by)) => (bx, by),
        (Err(e), _) | (_, Err(e)) => {
            tracing::warn!(player_id = %pid, action = args, error = ?e, "Invalid craft claim action");
            send_crafter_action_error(tx);
            return;
        }
    };

    let Some(view) = state.get_pack_at(bx, by) else {
        return;
    };
    if view.pack_type != PackType::Craft || view.owner_id != pid {
        return;
    }

    let craft_info = state.query_building_opt(bx, by, |ecs, entity| {
        let c = ecs.get::<BuildingCrafting>(entity)?;
        Some((c.recipe_id, c.num, c.end_ts))
    });

    let Some((Some(recipe_id), num, end_ts)) = craft_info else {
        return;
    };

    if now_ts() < end_ts {
        send_u_packet(tx, "OK", &ok_message("Крафтер", "Крафт ещё не завершён").1);
        return;
    }

    let Some(recipe) = crafting::recipe_by_id(recipe_id) else {
        return;
    };

    let Some(player_entity) = state.get_player_entity(pid) else {
        tracing::error!(player_id = %pid, "Player entity missing for craft claim");
        send_crafter_action_error(tx);
        return;
    };
    let Some(building_entity) = state.building_entity_at(bx, by) else {
        tracing::error!(player_id = %pid, x = bx, y = by, "Craft building entity missing for claim");
        send_crafter_action_error(tx);
        return;
    };

    let claimed = {
        let mut ecs = state.ecs_write_profiled("gui.craft_claim");
        if ecs.get::<PlayerInventory>(player_entity).is_none()
            || ecs.get::<PlayerFlags>(player_entity).is_none()
            || ecs.get::<BuildingCrafting>(building_entity).is_none()
            || ecs.get::<BuildingFlags>(building_entity).is_none()
        {
            tracing::error!(player_id = %pid, x = bx, y = by, "Required state missing for craft claim");
            send_crafter_state_error(tx);
            return;
        }
        let Some(craft) = ecs.get::<BuildingCrafting>(building_entity) else {
            tracing::error!(player_id = %pid, x = bx, y = by, "Building crafting component missing for claim");
            return;
        };
        if craft.recipe_id != Some(recipe_id) || craft.num != num || craft.end_ts != end_ts {
            false
        } else {
            {
                let mut inv = ecs
                    .get_mut::<PlayerInventory>(player_entity)
                    .expect("PlayerInventory checked before craft claim mutation");
                let entry = inv.items.entry(recipe.result.id).or_insert(0);
                *entry += recipe.result.num * num;
                send_inventory(tx, &mut inv);
            }

            {
                let mut craft = ecs
                    .get_mut::<BuildingCrafting>(building_entity)
                    .expect("BuildingCrafting checked before craft claim mutation");
                craft.recipe_id = None;
                craft.num = 0;
                craft.end_ts = 0;
                craft.ready = false;
            }
            let mut flags = ecs
                .get_mut::<PlayerFlags>(player_entity)
                .expect("PlayerFlags checked before craft claim mutation");
            flags.dirty = true;
            let mut flags = ecs
                .get_mut::<BuildingFlags>(building_entity)
                .expect("BuildingFlags checked before craft claim mutation");
            flags.dirty = true;
            true
        }
    };

    if !claimed {
        send_crafter_action_error(tx);
        return;
    }
    assert!(state.mark_building_dirty(building_entity));

    broadcast_pack_update(state, &view);
    show_crafter_recipes(tx, &view);
}
