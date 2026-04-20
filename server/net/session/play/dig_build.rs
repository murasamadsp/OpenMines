//! Копание клеток и установка блоков (Xdig, Xbld).
use crate::net::session::prelude::*;
use crate::net::session::social::misc::hurt_player_pure;

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
    let (px, py, actual_dir, dig_power, mine_mult, skin, clan_id) = {
        let player_data = state
            .modify_player(pid, |ecs, entity| {
                let (px, py, dig_p, m_mult, skin, clan_id) = {
                    let pos = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
                    let cd = ecs.get::<crate::game::player::PlayerCooldowns>(entity)?;
                    let ui = ecs.get::<crate::game::player::PlayerUI>(entity)?;
                    let skills = ecs.get::<crate::game::player::PlayerSkills>(entity)?;
                    let stats = ecs.get::<crate::game::player::PlayerStats>(entity)?;
                    if cd.last_dig.elapsed().as_millis() < 120 {
                        return None;
                    }
                    if ui.current_window.is_some() {
                        return None;
                    }
                    let dp = crate::game::skills::get_player_skill_effect(
                        &skills.states,
                        SkillType::Digging,
                    );
                    let mm = crate::game::skills::get_player_skill_effect(
                        &skills.states,
                        SkillType::MineGeneral,
                    );
                    (pos.x, pos.y, dp, mm, stats.skin, stats.clan_id.unwrap_or(0))
                };
                // Референс: `player.Move(player.x, player.y, dir)` сначала, потом `player.Bz()`.
                // Move с target == own position просто обновляет направление, не перемещает игрока.
                {
                    let mut pos_mut = ecs.get_mut::<crate::game::player::PlayerPosition>(entity)?;
                    if (0..=3).contains(&dir) {
                        pos_mut.dir = dir;
                    }
                }
                {
                    let mut cd_mut = ecs.get_mut::<crate::game::player::PlayerCooldowns>(entity)?;
                    cd_mut.last_dig = std::time::Instant::now();
                }
                Some((px, py, dir, dig_p, m_mult, skin, clan_id))
            })
            .flatten();
        let Some(data) = player_data else {
            return;
        };
        data
    };

    let (dx, dy) = dir_offset(actual_dir);
    let (tx_c, ty_c) = (px + dx, py + dy);
    if !state.world.valid_coord(tx_c, ty_c) {
        return;
    }

    let cell = state.world.get_cell(tx_c, ty_c);
    let (touch_damage, diggable) = {
        let defs = state.world.cell_defs();
        let p = defs.get(cell);
        (p.damage, p.is_diggable())
    };
    // Референс `Player.Bz`: сначала `Hurt(damage)` если `GetProp(cell).damage > 0`, потом проверка `is_diggable`.
    if touch_damage > 0 {
        hurt_player_pure(state, pid, touch_damage);
    }
    if !diggable {
        return;
    }

    let hit = if is_crystal(cell) {
        1.0
    } else {
        ((dig_power / 500.0) * dig_mult()).max(1.0e-6)
    };
    let destroyed = state.world.damage_cell(tx_c, ty_c, hit);
    let cry_idx = crystal_type(cell);

    state.modify_player(pid, |ecs, entity| {
        {
            let mut skills = ecs.get_mut::<crate::game::player::PlayerSkills>(entity)?;
            let leveled_dig = add_skill_exp(&mut skills.states, "d", 1.0);
            let leveled_mine = if cry_idx.is_some() {
                add_skill_exp(&mut skills.states, "m", 1.0)
            } else {
                false
            };
            if leveled_dig || leveled_mine {
                let sk = skills_packet(&skill_progress_payload(&skills.states));
                send_u_packet(tx, sk.0, &sk.1);
            }
        }
        {
            let mut flags = ecs.get_mut::<crate::game::player::PlayerFlags>(entity)?;
            flags.dirty = true;
        }
        Some(())
    });

    if destroyed {
        if let Some(idx) = cry_idx {
            let amount = (crystal_multiplier(cell) as f32 * mine_mult).round() as i64;
            state.modify_player(pid, |ecs, entity| {
                let mut stats = ecs.get_mut::<crate::game::player::PlayerStats>(entity)?;
                stats.crystals[idx] += amount.max(1);
                let c_data = stats.crystals;
                send_u_packet(tx, "@B", &basket(&c_data, 1000).1);
                Some(())
            });
        }
        broadcast_cell_update(state, tx_c, ty_c);
        if is_boulder(cell) {
            let (bx, by) = (tx_c + dx, ty_c + dy);
            if state.world.valid_coord(bx, by) && state.world.is_empty(bx, by) {
                state.world.set_cell(bx, by, cell);
                broadcast_cell_update(state, bx, by);
            }
        }
    }
    let (cx, cy) = World::chunk_pos(px, py);
    // Референс: `player.Bz()` → `SendDFToBots(...)` — рассылает FX копания соседям.
    let fx = hb_directed_fx(
        net_u16_nonneg(pid),
        net_u16_nonneg(px),
        net_u16_nonneg(py),
        0,
        actual_dir as u8,
        0,
    );
    state.broadcast_to_nearby(cx, cy, &encode_hb_bundle(&hb_bundle(&[fx]).1), Some(pid));
    // Референс: `player.Move(player.x, player.y, dir)` → `SendMyMove()` — рассылает hb_bot
    // с обновлённым направлением соседям (position не изменилась, только dir).
    let bot = hb_bot(
        net_u16_nonneg(pid),
        net_u16_nonneg(px),
        net_u16_nonneg(py),
        net_u8_clamped(actual_dir, 3),
        net_u8_clamped(skin, 255),
        net_u16_nonneg(clan_id),
        0,
    );
    state.broadcast_to_nearby(cx, cy, &encode_hb_bundle(&hb_bundle(&[bot]).1), Some(pid));
}

pub fn handle_build(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    bld: &XbldClient,
) {
    let (px, py, pdir) = {
        let data = state
            .modify_player(pid, |ecs, entity| {
                let (px, py, pdir) = {
                    let pos = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
                    let ui = ecs.get::<crate::game::player::PlayerUI>(entity)?;
                    if ui.current_window.is_some() {
                        return None;
                    }
                    (pos.x, pos.y, pos.dir)
                };
                {
                    let mut pos_mut = ecs.get_mut::<crate::game::player::PlayerPosition>(entity)?;
                    if (0..=3).contains(&bld.direction) {
                        pos_mut.dir = bld.direction;
                    }
                }
                Some((px, py, pdir))
            })
            .flatten();
        let Some(d) = data else {
            return;
        };
        d
    };

    let (dx, dy) = dir_offset(pdir);
    let (tx_c, ty_c) = (px + dx, py + dy);
    if !state.world.valid_coord(tx_c, ty_c) {
        return;
    }

    let cur = state.world.get_cell(tx_c, ty_c);
    let binding = state.world.cell_defs();
    let prop = binding.get(cur);

    match bld.block_type.as_str() {
        "G" => {
            if prop.cell_is_empty() || prop.is_sand() {
                if try_spend_crystal(state, tx, pid, 0, 1) {
                    place_block(state, tx_c, ty_c, cell_type::GREEN_BLOCK);
                }
            } else if cur == cell_type::GREEN_BLOCK {
                if try_spend_crystal(state, tx, pid, 4, 1) {
                    place_block(state, tx_c, ty_c, cell_type::YELLOW_BLOCK);
                }
            } else if cur == cell_type::YELLOW_BLOCK {
                if try_spend_crystal(state, tx, pid, 2, 1) {
                    place_block(state, tx_c, ty_c, cell_type::RED_BLOCK);
                }
            }
        }
        "R" => {
            if is_truly_empty(cur) && try_spend_crystal(state, tx, pid, 0, 1) {
                place_block(state, tx_c, ty_c, cell_type::ROAD);
            }
        }
        "O" => {
            if (prop.cell_is_empty() || prop.is_sand()) && try_spend_crystal(state, tx, pid, 0, 1) {
                place_block(state, tx_c, ty_c, cell_type::SUPPORT);
            }
        }
        "V" => {
            if is_truly_empty(cur) && try_spend_crystal(state, tx, pid, 5, 1) {
                place_block(state, tx_c, ty_c, cell_type::MILITARY_BLOCK);
            }
        }
        _ => {}
    }
}

pub fn try_spend_crystal(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    idx: usize,
    amount: i64,
) -> bool {
    state
        .modify_player(pid, |ecs, entity| {
            let mut s = ecs.get_mut::<crate::game::player::PlayerStats>(entity)?;
            if s.crystals[idx] >= amount {
                s.crystals[idx] -= amount;
                let c_data = s.crystals;
                send_u_packet(tx, "@B", &basket(&c_data, 1000).1);
                Some(true)
            } else {
                Some(false)
            }
        })
        .flatten()
        .unwrap_or(false)
}

pub fn broadcast_cell_update(state: &Arc<GameState>, x: i32, y: i32) {
    let sub = hb_cell(x as u16, y as u16, state.world.get_cell(x, y));
    let (cx, cy) = World::chunk_pos(x, y);
    state.broadcast_to_nearby(cx, cy, &encode_hb_bundle(&hb_bundle(&[sub]).1), None);
}

fn place_block(state: &Arc<GameState>, x: i32, y: i32, cell: u8) {
    state.world.set_cell(x, y, cell);
    broadcast_cell_update(state, x, y);
}

pub const fn is_truly_empty(cell: u8) -> bool {
    cell == cell_type::NOTHING || cell == cell_type::EMPTY
}
