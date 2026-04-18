//! Копание клеток и установка блоков (Xdig, Xbld).
//!
//! Следует методологии server-authoritative (`server/AGENTS.md`):
//! rate-limit тихо дропает, остальные rejection — `warn!` + корректирующий снапшот,
//! FX broadcast исключает отправителя (он уже проиграл анимацию).
use crate::net::session::prelude::*;

// ─── Digging ────────────────────────────────────────────────────────────────

fn trace_dig_enabled() -> bool {
    std::env::var("M3R_TRACE_DIG").ok().is_some_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn dig_mult() -> f32 {
    std::env::var("M3R_DIG_MULT")
        .ok()
        .and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(1.0)
}

pub fn handle_dig(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    dir: i32,
) {
    // Rate-limiting digging
    {
        let Some(p) = state.active_players.get(&pid) else {
            return;
        };
        let elapsed = p.last_dig_ts.elapsed().as_millis() as u64;
        if elapsed < 120 {
            return;
        }
    }

    let (px, py, pdir, in_window) = {
        let Some(mut p) = state.active_players.get_mut(&pid) else {
            return;
        };
        if (0..=3).contains(&dir) {
            p.data.dir = dir;
        }
        p.last_dig_ts = std::time::Instant::now();
        (p.data.x, p.data.y, p.data.dir, p.current_window.is_some())
    };
    if in_window {
        tracing::warn!("handle_dig: pid={pid} rejected: player in GUI window");
        return;
    }

    let (step_x, step_y) = dir_offset(pdir);
    let (target_x, target_y) = (px + step_x, py + step_y);

    if !state.world.valid_coord(target_x, target_y) {
        tracing::warn!(
            "handle_dig: pid={pid} rejected: invalid target=({target_x},{target_y}) from ({px},{py}) dir={pdir}"
        );
        return;
    }

    let cell = state.world.get_cell(target_x, target_y);
    let cell_defs = state.world.cell_defs();
    let prop = cell_defs.get(cell);

    if trace_dig_enabled() && prop.is_sand() {
        let d = state.world.get_durability(target_x, target_y);
        tracing::info!(
            "trace_dig: pid={pid} sand cell={cell} at ({target_x},{target_y}) d={d} def_dur={} can_place_over={} empty={}",
            prop.durability,
            prop.can_place_over(),
            prop.cell_is_empty(),
        );
    }

    // Референс `Player.Bz`: без is_diggable урон не наносится (в т.ч. 114/117 — оболочка генератора).
    if !prop.is_diggable() {
        tracing::warn!(
            "handle_dig: pid={pid} rejected: cell {cell} at ({target_x},{target_y}) is not diggable"
        );
        return;
    }

    // Damage cell — dig power from Digging skill
    let (dig_power, mine_mult) = {
        let skills_ref = state
            .active_players
            .get(&pid)
            .map(|p| p.data.skills.clone())
            .unwrap_or_default();
        (
            get_player_skill_effect(&skills_ref, SkillType::Digging),
            get_player_skill_effect(&skills_ref, SkillType::MineGeneral),
        )
    };
    let hit_dmg = if is_crystal(cell) {
        1.0
    } else {
        // Минимальный урон: иначе при нулевом/битом `dig_power` клетка никогда не изнашивается.
        ((dig_power / 500.0) * dig_mult()).max(1.0e-6)
    };

    if trace_dig_enabled() && prop.is_sand() {
        tracing::info!("trace_dig: pid={pid} sand hit_dmg={hit_dmg} dig_power={dig_power}");
    }
    let destroyed = state.world.damage_cell(target_x, target_y, hit_dmg);

    // If crystal, add to player's basket only after the cell is destroyed
    let crystal_type_index = crystal_type(cell);
    let crystal_mined = crystal_type_index.is_some();
    let mut maybe_crystal_gain = None;

    // Gain skill exp for digging
    {
        let leveled_dig;
        let leveled_mine;
        let skill_data;
        {
            let Some(mut p) = state.active_players.get_mut(&pid) else {
                return;
            };
            leveled_dig = add_skill_exp(&mut p.data.skills, "d", 1.0);
            leveled_mine = if crystal_mined {
                add_skill_exp(&mut p.data.skills, "m", 1.0)
            } else {
                false
            };
            if leveled_dig || leveled_mine {
                skill_data = Some(skill_progress_payload(&p.data.skills));
            } else {
                skill_data = None;
            }
        }
        if let Some(sd) = skill_data {
            send_u_packet(tx, "SK", &skills_packet(&sd).1);
        }
        // Mark dirty after skill exp change
        if let Some(mut p) = state.active_players.get_mut(&pid) {
            p.dirty = true;
        }
    }

    // If cell was destroyed, send update to nearby players
    if destroyed {
        if let Some(cry_idx) = crystal_type_index {
            let base_amount = crystal_multiplier(cell);
            #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
            let amount = (base_amount as f32 * mine_mult).round() as i64;
            maybe_crystal_gain = Some((cry_idx, amount.max(1)));
        }
        broadcast_cell_update(state, target_x, target_y);

        // Boulder pushing: if destroyed cell was a boulder, try to push it
        if is_boulder(cell) {
            let push_x = target_x + step_x;
            let push_y = target_y + step_y;
            if state.world.valid_coord(push_x, push_y) && state.world.is_empty(push_x, push_y) {
                state.world.set_cell(push_x, push_y, cell);
                broadcast_cell_update(state, push_x, push_y);
            }
        }
    }

    if let Some((cry_idx, amount)) = maybe_crystal_gain {
        if let Some(mut p) = state.active_players.get_mut(&pid) {
            p.data.crystals[cry_idx] += amount;
            let crys = p.data.crystals;
            send_u_packet(tx, "@B", &basket(&crys, 1000).1);
        }
    }

    // Send dig FX to nearby
    let (cx, cy) = World::chunk_pos(px, py);
    let fx = hb_directed_fx(
        net_u16_nonneg(pid),
        net_u16_nonneg(px),
        net_u16_nonneg(py),
        0,
        net_u8_clamped(pdir, 3),
        0,
    );
    let fx_data = encode_hb_bundle(&hb_bundle(&[fx]).1);
    // Отправитель уже проиграл анимацию у себя — шлём только соседям.
    state.broadcast_to_nearby(cx, cy, &fx_data, Some(pid));
}

// ─── Building ───────────────────────────────────────────────────────────────

pub fn handle_build(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    bld: &XbldClient,
) {
    let (px, py, pdir, in_window) = {
        let Some(mut p) = state.active_players.get_mut(&pid) else {
            return;
        };
        if (0..=3).contains(&bld.direction) {
            p.data.dir = bld.direction;
        }
        (p.data.x, p.data.y, p.data.dir, p.current_window.is_some())
    };
    if in_window {
        tracing::warn!("handle_build: pid={pid} rejected: player in GUI window");
        return;
    }

    let (dx, dy) = dir_offset(pdir);
    let (target_x, target_y) = (px + dx, py + dy);

    if !state.world.valid_coord(target_x, target_y) {
        tracing::warn!("handle_build: pid={pid} rejected: invalid target=({target_x},{target_y})");
        return;
    }

    let current_cell = state.world.get_cell(target_x, target_y);
    let cell_defs = state.world.cell_defs();
    let prop = cell_defs.get(current_cell);

    match bld.block_type.as_str() {
        "G" => {
            // Green block: place on empty/sand, costs 1 green crystal
            // Upgrade chain: empty → GreenBlock(101) → YellowBlock(102, costs white) → RedBlock(105, costs red)
            match current_cell {
                _ if prop.cell_is_empty() || prop.is_sand() => {
                    if try_spend_crystal(state, tx, pid, 0, 1) {
                        place_block(state, target_x, target_y, cell_type::GREEN_BLOCK);
                    }
                }
                cell_type::GREEN_BLOCK => {
                    // Upgrade to yellow — costs 1 white crystal
                    if try_spend_crystal(state, tx, pid, 4, 1) {
                        place_block(state, target_x, target_y, cell_type::YELLOW_BLOCK);
                    }
                }
                cell_type::YELLOW_BLOCK => {
                    // Upgrade to red — costs 1 red crystal
                    if try_spend_crystal(state, tx, pid, 2, 1) {
                        place_block(state, target_x, target_y, cell_type::RED_BLOCK);
                    }
                }
                _ => {}
            }
        }
        "R" => {
            // Road: place on truly empty, costs 1 green crystal
            if is_truly_empty(current_cell) && try_spend_crystal(state, tx, pid, 0, 1) {
                place_block(state, target_x, target_y, cell_type::ROAD);
            }
        }
        "O" => {
            // Support: place on empty/sand, costs 1 green crystal
            if (prop.cell_is_empty() || prop.is_sand()) && try_spend_crystal(state, tx, pid, 0, 1) {
                place_block(state, target_x, target_y, cell_type::SUPPORT);
            }
        }
        "V" => {
            // Military block: place on truly empty, costs 1 cyan crystal
            if is_truly_empty(current_cell) && try_spend_crystal(state, tx, pid, 5, 1) {
                place_block(state, target_x, target_y, cell_type::MILITARY_BLOCK);
            }
        }
        _ => {
            tracing::warn!(
                "handle_build: pid={pid} rejected: unknown block_type={:?}",
                bld.block_type
            );
        }
    }
}

/// Try to remove crystals from player's basket. Returns true if successful.
pub fn try_spend_crystal(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    crystal_idx: usize,
    amount: i64,
) -> bool {
    let Some(mut p) = state.active_players.get_mut(&pid) else {
        return false;
    };
    if p.data.crystals[crystal_idx] >= amount {
        p.data.crystals[crystal_idx] -= amount;
        let crys = p.data.crystals;
        send_u_packet(tx, "@B", &basket(&crys, 1000).1);
        true
    } else {
        false
    }
}

/// Broadcast a single cell update to all nearby players
pub fn broadcast_cell_update(state: &Arc<GameState>, x: i32, y: i32) {
    let new_cell = state.world.get_cell(x, y);
    let sub = hb_cell(net_u16_nonneg(x), net_u16_nonneg(y), new_cell);
    let hb_data = encode_hb_bundle(&hb_bundle(&[sub]).1);
    let (cx, cy) = World::chunk_pos(x, y);
    state.broadcast_to_nearby(cx, cy, &hb_data, None);
}

fn place_block(state: &Arc<GameState>, x: i32, y: i32, cell: u8) {
    state.world.set_cell(x, y, cell);
    broadcast_cell_update(state, x, y);
}

/// Check if cell is truly empty (cell 0 or cell 32) — for road/military placement
pub const fn is_truly_empty(cell: u8) -> bool {
    cell == cell_type::NOTHING || cell == cell_type::EMPTY
}
