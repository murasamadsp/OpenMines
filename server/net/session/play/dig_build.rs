//! Копание клеток и установка блоков (Xdig, Xbld).
use crate::net::session::play::death::hurt_player_pure;
use crate::net::session::prelude::*;

pub fn handle_dig(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    dir: i32,
) {
    let (px, py, actual_dir, dig_power, mine_mult, skin, clan_id, crystal_carry_init) = {
        let player_data = state
            .modify_player(pid, |ecs, entity| {
                let (px, py, dig_p, m_mult, skin, clan_id, cc) = {
                    let pos = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
                    let cd = ecs.get::<crate::game::player::PlayerCooldowns>(entity)?;
                    let ui = ecs.get::<crate::game::player::PlayerUI>(entity)?;
                    let skills = ecs.get::<crate::game::player::PlayerSkills>(entity)?;
                    let p_stats = ecs.get::<crate::game::player::PlayerStats>(entity)?;
                    // 1:1 ref `Session.cs:230` `DigHandler => TryAct(..., 200)`
                    if cd.last_dig.elapsed().as_millis() < 200 {
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
                    (
                        pos.x,
                        pos.y,
                        dp,
                        mm,
                        p_stats.skin,
                        p_stats.clan_id.unwrap_or(0),
                        p_stats.crystal_carry,
                    )
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
                Some((px, py, dir, dig_p, m_mult, skin, clan_id, cc))
            })
            .flatten();
        let Some(data) = player_data else {
            return;
        };
        data
    };

    let (dx, dy) = dir_offset(actual_dir);
    let (tgt_x, tgt_y) = (px + dx, py + dy);
    if !state.world.valid_coord(tgt_x, tgt_y) {
        return;
    }

    let cell = state.world.get_cell(tgt_x, tgt_y);
    let (touch_damage, diggable) = {
        let defs = state.world.cell_defs();
        let p = defs.get(cell);
        (p.damage, p.is_diggable())
    };
    // Референс `Player.Bz`: сначала `Hurt(damage)` если `GetProp(cell).damage > 0`, потом проверка `is_diggable`.
    if touch_damage > 0 {
        hurt_player_pure(state, pid, touch_damage);
    }

    let tail = state
        .query_player(pid, |ecs, entity| {
            ecs.get::<crate::game::programmator::ProgrammatorState>(entity)
                .map_or(0, |ps| u8::from(ps.running))
        })
        .unwrap_or(0);

    let (cx, cy) = World::chunk_pos(px, py);

    // Fix 4: FX broadcast BEFORE the !diggable check — C# sends it unconditionally at top of Bz().
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

    // Fix 2: BOX (90) special case — pick up crystals and destroy.
    // H-1 фикс: in-memory `box_take` вместо sync SQLite на tick-пути.
    if cell == cell_type::BOX {
        if let Some(bc) = state.box_take(tgt_x, tgt_y) {
            state.modify_player(pid, |ecs, entity| {
                let mut p_stats = ecs.get_mut::<crate::game::player::PlayerStats>(entity)?;
                for i in 0..6 {
                    p_stats.crystals[i] += bc[i];
                }
                let c_data = p_stats.crystals;
                send_u_packet(tx, "@B", &basket(&c_data, 1).1);
                Some(())
            });
        }
        state.world.damage_cell(tgt_x, tgt_y, 1.0);
        broadcast_cell_update(state, tgt_x, tgt_y);
        // Референс: `player.Move(player.x, player.y, dir)` → `SendMyMove()`.
        let bot = hb_bot(
            net_u16_nonneg(pid),
            net_u16_nonneg(px),
            net_u16_nonneg(py),
            net_u8_clamped(actual_dir, 3),
            net_u8_clamped(skin, 255),
            net_u16_nonneg(clan_id),
            tail,
        );
        state.broadcast_to_nearby(cx, cy, &encode_hb_bundle(&hb_bundle(&[bot]).1), Some(pid));
        return;
    }

    if !diggable {
        return;
    }

    // Fix 3: MilitaryBlock (81) special case — fixed 1.0 damage, no multiplier, no crystal/exp/FX2.
    if cell == cell_type::MILITARY_BLOCK {
        let destroyed = state.world.damage_cell(tgt_x, tgt_y, 1.0);
        if destroyed {
            broadcast_cell_update(state, tgt_x, tgt_y);
        }
        let bot = hb_bot(
            net_u16_nonneg(pid),
            net_u16_nonneg(px),
            net_u16_nonneg(py),
            net_u8_clamped(actual_dir, 3),
            net_u8_clamped(skin, 255),
            net_u16_nonneg(clan_id),
            tail,
        );
        state.broadcast_to_nearby(cx, cy, &encode_hb_bundle(&hb_bundle(&[bot]).1), Some(pid));
        return;
    }

    let hit = if is_crystal(cell) {
        1.0
    } else {
        (dig_power / 500.0).max(1.0e-6)
    };
    let destroyed = state.world.damage_cell(tgt_x, tgt_y, hit);
    let cry_idx = crystal_type(cell);

    // D8+D9: Crystal mining happens on EVERY hit (1:1 with C# Player.Bz → Mine(cell,x,y)).
    // Dig exp happens only on destroy. MineGeneral exp happens every hit with crystals.
    let mined_amount = cry_idx.map_or(0_i64, |idx| {
        // cb fractional crystal accumulator.
        let mut carry = crystal_carry_init;
        let pre_mult_dob = 1.0_f32 + carry.trunc() + mine_mult;
        let mine_exp = pre_mult_dob.trunc();
        let dob = pre_mult_dob * crystal_multiplier(cell) as f32;
        carry -= carry.trunc();
        let odob = dob.trunc() as i64;
        carry += dob - odob as f32;
        let amount = odob.max(1);

        // Update crystal_carry + add crystals + MineGeneral exp on every hit.
        state.modify_player(pid, |ecs, entity| {
            {
                let mut p_stats = ecs.get_mut::<crate::game::player::PlayerStats>(entity)?;
                p_stats.crystal_carry = carry;
                p_stats.crystals[idx] += amount;
                let c_data = p_stats.crystals;
                send_u_packet(tx, "@B", &basket(&c_data, 1).1);
            }
            {
                let mut skills = ecs.get_mut::<crate::game::player::PlayerSkills>(entity)?;
                if add_skill_exp(&mut skills.states, "m", mine_exp) {
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

        // Crystal mine FX (fx=2) on every hit.
        // Color remapping: type 1→3, 2→1, 3→2, other→same.
        let color_remapped = match idx {
            1 => 3_u8,
            2 => 1_u8,
            3 => 2_u8,
            other => other as u8,
        };
        let mine_fx = hb_directed_fx(
            net_u16_nonneg(pid),
            net_u16_nonneg(tgt_x),
            net_u16_nonneg(tgt_y),
            2,
            (amount.min(255)) as u8,
            color_remapped,
        );
        state.broadcast_to_nearby(
            cx,
            cy,
            &encode_hb_bundle(&hb_bundle(&[mine_fx]).1),
            Some(pid),
        );

        amount
    });
    let _ = mined_amount;

    // Boulder push на КАЖДЫЙ удар (не только при разрушении), 1:1 C# `Player.Bz`
    // (390-404) → `World.MoveCell`: валун ПЕРЕМЕЩАЕТСЯ на клетку в направлении копки,
    // если она пуста. `MoveCell` ОЧИЩАЕТ источник и ПЕРЕНОСИТ durability — раньше
    // Rust только `set_cell(dest)` без очистки источника → валун ДУБЛИРОВАЛСЯ
    // (эксплойт: копай валун → плодятся). Гейт `!destroyed`: при разрушении
    // `damage_cell` уже очистил клетку, а `cell` захвачен ДО удара (иначе воскресим);
    // C# в этом случае MoveCell'ит пустую клетку = no-op.
    let pushed_boulder = if is_boulder(cell) && !destroyed {
        let (bx, by) = (tgt_x + dx, tgt_y + dy);
        if state.world.valid_coord(bx, by) && state.world.is_empty(bx, by) {
            // durability читаем ПОСЛЕ damage_cell (как C# GetDurability после DamageCell).
            let dur = state.world.get_durability(tgt_x, tgt_y);
            state.world.destroy(tgt_x, tgt_y);
            broadcast_cell_update(state, tgt_x, tgt_y);
            state.world.set_cell(bx, by, cell);
            state.world.set_durability(bx, by, dur);
            broadcast_cell_update(state, bx, by);
            true
        } else {
            false
        }
    } else {
        false
    };

    // Fix 10: Boulder push exp.
    if pushed_boulder {
        state.modify_player(pid, |ecs, entity| {
            let mut skills = ecs.get_mut::<crate::game::player::PlayerSkills>(entity)?;
            if add_skill_exp(&mut skills.states, "d", 1.0) {
                let sk = skills_packet(&skill_progress_payload(&skills.states));
                send_u_packet(tx, sk.0, &sk.1);
            }
            Some(())
        });
    }

    if destroyed {
        // Dig exp only on destroy (1:1 with C# OnDestroy → AddExp("d")).
        state.modify_player(pid, |ecs, entity| {
            {
                let mut skills = ecs.get_mut::<crate::game::player::PlayerSkills>(entity)?;
                add_skill_exp(&mut skills.states, "d", 1.0);
            }
            {
                let mut flags = ecs.get_mut::<crate::game::player::PlayerFlags>(entity)?;
                flags.dirty = true;
            }
            Some(())
        });

        broadcast_cell_update(state, tgt_x, tgt_y);
    } else if cry_idx.is_none() {
        // Mark dirty on non-destroying, non-crystal hits too (for save consistency).
        state.modify_player(pid, |ecs, entity| {
            let mut flags = ecs.get_mut::<crate::game::player::PlayerFlags>(entity)?;
            flags.dirty = true;
            Some(())
        });
    }

    // Референс: `player.Move(player.x, player.y, dir)` → `SendMyMove()` — рассылает hb_bot
    // с обновлённым направлением соседям (position не изменилась, только dir).
    let bot = hb_bot(
        net_u16_nonneg(pid),
        net_u16_nonneg(px),
        net_u16_nonneg(py),
        net_u8_clamped(actual_dir, 3),
        net_u8_clamped(skin, 255),
        net_u16_nonneg(clan_id),
        tail,
    );
    state.broadcast_to_nearby(cx, cy, &encode_hb_bundle(&hb_bundle(&[bot]).1), Some(pid));
}

pub fn handle_build(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    bld: &XbldClient,
) {
    // Fix 11: Extract player data including clan_id, skills, and build cooldown check.
    let (px, py, pdir, clan_id, build_skill_effect, build_skill_hp) = {
        let data = state
            .modify_player(pid, |ecs, entity| {
                let pos = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
                let ui = ecs.get::<crate::game::player::PlayerUI>(entity)?;
                let cd = ecs.get::<crate::game::player::PlayerCooldowns>(entity)?;
                let skills = ecs.get::<crate::game::player::PlayerSkills>(entity)?;
                let p_stats = ecs.get::<crate::game::player::PlayerStats>(entity)?;
                if ui.current_window.is_some() {
                    return None;
                }
                // 1:1 ref `Session.cs:233` `BuildHandler => TryAct(..., 200)`
                if cd.last_build.elapsed().as_millis() < 200 {
                    return None;
                }
                let (px, py, pdir) = (pos.x, pos.y, pos.dir);
                let clan_id = p_stats.clan_id.unwrap_or(0);

                // Fix 12: Crystal cost from skill.Effect.
                // Fix 16: Durability from skill AdditionalEffect (on_bld_hp).
                let (skill_type, hp_skill_type) = match bld.block_type.as_str() {
                    "G" => (SkillType::BuildGreen, Some(SkillType::BuildGreen)),
                    "R" => (SkillType::BuildRoad, None),
                    "O" => (SkillType::BuildStructure, None),
                    "V" => (SkillType::BuildWar, Some(SkillType::BuildWar)),
                    _ => return None,
                };
                let effect = get_player_skill_effect(&skills.states, skill_type);
                // on_bld_hp: for BuildGreen/BuildYellow/BuildRed/BuildWar return level as f32.
                let hp_effect = hp_skill_type
                    .map(|hst| {
                        skills
                            .states
                            .find(hst.code())
                            .map_or(1.0_f32, |s| s.level as f32)
                    })
                    .unwrap_or(1.0);

                {
                    let mut pos_mut = ecs.get_mut::<crate::game::player::PlayerPosition>(entity)?;
                    if (0..=3).contains(&bld.direction) {
                        pos_mut.dir = bld.direction;
                    }
                }
                {
                    let mut cd_mut = ecs.get_mut::<crate::game::player::PlayerCooldowns>(entity)?;
                    cd_mut.last_build = std::time::Instant::now();
                }
                Some((px, py, pdir, clan_id, effect, hp_effect))
            })
            .flatten();
        let Some(d) = data else {
            return;
        };
        d
    };

    let (dx, dy) = dir_offset(pdir);
    let (tgt_x, tgt_y) = (px + dx, py + dy);
    if !state.world.valid_coord(tgt_x, tgt_y) {
        return;
    }

    // Fix 13: AccessGun check — block build in enemy gun zone.
    if !state.access_gun(tgt_x, tgt_y, clan_id) {
        return;
    }

    // Fix 14: PackPart check — can't build on a building cell.
    if state.building_index.contains_key(&(tgt_x, tgt_y)) {
        return;
    }

    let cur = state.world.get_cell(tgt_x, tgt_y);
    let binding = state.world.cell_defs();
    let prop = binding.get(cur);

    // Fix 12: cost = effect.max(1.0) as i64.
    let cost = build_skill_effect.max(1.0) as i64;
    // Fix 16: durability from on_bld_hp.
    let durability = build_skill_hp;

    let mut placed_skill: Option<SkillType> = None;

    match bld.block_type.as_str() {
        "G" => {
            if prop.cell_is_empty() || prop.is_sand() {
                if try_spend_crystal(state, tx, pid, 0, cost) {
                    place_block(state, tgt_x, tgt_y, cell_type::GREEN_BLOCK);
                    state.world.set_durability(tgt_x, tgt_y, durability);
                    placed_skill = Some(SkillType::BuildGreen);
                }
            } else if cur == cell_type::GREEN_BLOCK {
                // Upgrading green → yellow uses BuildYellow skill effect/cost.
                let yellow_effect = {
                    state
                        .query_player(pid, |ecs, entity| {
                            let skills = ecs.get::<crate::game::player::PlayerSkills>(entity)?;
                            let eff =
                                get_player_skill_effect(&skills.states, SkillType::BuildYellow);
                            let hp = skills
                                .states
                                .find(SkillType::BuildYellow.code())
                                .map_or(1.0_f32, |s| s.level as f32);
                            Some((eff, hp))
                        })
                        .flatten()
                        .unwrap_or((1.0, 1.0))
                };
                let y_cost = yellow_effect.0.max(1.0) as i64;
                if try_spend_crystal(state, tx, pid, 4, y_cost) {
                    // D7: Yellow upgrade adds durability to existing (C# GetDurability + AdditionalEffect).
                    let existing_dur = state.world.get_durability(tgt_x, tgt_y);
                    place_block(state, tgt_x, tgt_y, cell_type::YELLOW_BLOCK);
                    state
                        .world
                        .set_durability(tgt_x, tgt_y, existing_dur + yellow_effect.1);
                    placed_skill = Some(SkillType::BuildYellow);
                }
            } else if cur == cell_type::YELLOW_BLOCK {
                // Upgrading yellow → red uses BuildRed skill effect/cost.
                let red_effect = {
                    state
                        .query_player(pid, |ecs, entity| {
                            let skills = ecs.get::<crate::game::player::PlayerSkills>(entity)?;
                            let eff = get_player_skill_effect(&skills.states, SkillType::BuildRed);
                            let hp = skills
                                .states
                                .find(SkillType::BuildRed.code())
                                .map_or(1.0_f32, |s| s.level as f32);
                            Some((eff, hp))
                        })
                        .flatten()
                        .unwrap_or((1.0, 1.0))
                };
                let r_cost = red_effect.0.max(1.0) as i64;
                if try_spend_crystal(state, tx, pid, 2, r_cost) {
                    // D7: Red upgrade adds durability to existing (C# GetDurability + AdditionalEffect).
                    let existing_dur = state.world.get_durability(tgt_x, tgt_y);
                    place_block(state, tgt_x, tgt_y, cell_type::RED_BLOCK);
                    state
                        .world
                        .set_durability(tgt_x, tgt_y, existing_dur + red_effect.1);
                    placed_skill = Some(SkillType::BuildRed);
                }
            }
        }
        "R" => {
            if is_truly_empty(cur) && try_spend_crystal(state, tx, pid, 0, cost) {
                place_block(state, tgt_x, tgt_y, cell_type::ROAD);
                state.world.set_durability(tgt_x, tgt_y, durability);
                placed_skill = Some(SkillType::BuildRoad);
            }
        }
        "O" => {
            if (prop.cell_is_empty() || prop.is_sand())
                && try_spend_crystal(state, tx, pid, 0, cost)
            {
                place_block(state, tgt_x, tgt_y, cell_type::SUPPORT);
                state.world.set_durability(tgt_x, tgt_y, durability);
                placed_skill = Some(SkillType::BuildStructure);
            }
        }
        "V" => {
            // 1:1 ref C# Player.Build("V"): place MilitaryBlockFrame (80) first, then
            // World.W.StupidAction(10, x, y, () => convert to MilitaryBlock (81)) after 10 ticks.
            if is_truly_empty(cur) && try_spend_crystal(state, tx, pid, 5, cost) {
                place_block(state, tgt_x, tgt_y, cell_type::MILITARY_BLOCK_FRAME);
                // Schedule conversion: frame→block after 10 ticks (1:1 C# StupidAction).
                let mut ecs = state.ecs.write();
                ecs.resource_mut::<crate::game::PendingCellConversions>()
                    .0
                    .push(crate::game::PendingConversion {
                        x: tgt_x,
                        y: tgt_y,
                        ticks_left: 10,
                        required_cell: cell_type::MILITARY_BLOCK_FRAME,
                        target_cell: cell_type::MILITARY_BLOCK,
                        durability,
                    });
                placed_skill = Some(SkillType::BuildWar);
            }
        }
        _ => {}
    }

    // Fix 15: Build skill exp after successful placement.
    if let Some(skill) = placed_skill {
        state.modify_player(pid, |ecs, entity| {
            let mut skills = ecs.get_mut::<crate::game::player::PlayerSkills>(entity)?;
            add_skill_exp(&mut skills.states, skill.code(), 1.0);
            Some(())
        });
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
                send_u_packet(tx, "@B", &basket(&c_data, 1).1);
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
