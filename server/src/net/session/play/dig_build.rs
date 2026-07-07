//! Копание клеток и установка блоков (Xdig, Xbld).
use crate::net::session::play::death::hurt_player_pure;
use crate::net::session::prelude::*;

/// Делитель силы копания (C# `Player.cs`: `digPower / 500`).
const DIG_POWER_DIVISOR: f32 = 500.0;
/// Минимальный урон за удар — не даём округлить до нуля (epsilon).
const MIN_HIT_POWER: f32 = 1.0e-6;

fn send_build_state_error(tx: &mpsc::UnboundedSender<Vec<u8>>) {
    send_u_packet(
        tx,
        "OK",
        &ok_message("СТРОЙКА", "Состояние игрока недоступно.").1,
    );
}

struct BuildPlayerData {
    x: i32,
    y: i32,
    dir: i32,
    clan_id: i32,
    skill_effect: f32,
    skill_hp: f32,
    yellow_effect: f32,
    yellow_hp: f32,
    red_effect: f32,
    red_hp: f32,
}

enum BuildPlayerRead {
    Ready(BuildPlayerData),
    Blocked,
    MissingState(&'static str),
}

struct DigPlayerData {
    x: i32,
    y: i32,
    dir: i32,
    dig_power: f32,
    mine_general: f32,
    mine_by_crystal: [f32; 6],
    skin: i32,
    clan_id: i32,
}

enum DigPlayerRead {
    Ready(DigPlayerData),
    Blocked,
    MissingState(&'static str),
}

enum BoxPickupResult {
    Picked([i64; 6]),
    Empty,
    MissingState(&'static str),
}

enum DigMutationRead {
    Ready,
    MissingState(&'static str),
}

struct CrystalMineYield {
    final_amount: i64,
    exp_amount: f32,
}

impl CrystalMineYield {
    fn calculate(
        mine_general: f32,
        mine_for_crystal: f32,
        crystal_multiplier: i64,
        ctx: crate::game::ExpContext,
    ) -> Self {
        let mining_amount = 1.0_f32 + mine_general + mine_for_crystal;
        let base_drop = crate::game::mechanics::random::probabilistic_i64(
            mining_amount * crystal_multiplier as f32,
        )
        .max(1);
        Self {
            final_amount: ctx.apply_drop(base_drop),
            exp_amount: mining_amount,
        }
    }
}

pub fn handle_dig(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    dir: i32,
    programmatic: bool,
) {
    let ctx = crate::game::ExpContext::from_state(state);
    let player_data = state
        .modify_player(pid, |ecs, entity| {
            let Some(pos) = ecs.get::<crate::game::player::PlayerPosition>(entity) else {
                return Some(DigPlayerRead::MissingState("PlayerPosition"));
            };
            let Some(cd) = ecs.get::<crate::game::player::PlayerCooldowns>(entity) else {
                return Some(DigPlayerRead::MissingState("PlayerCooldowns"));
            };
            let Some(ui) = ecs.get::<crate::game::player::PlayerUI>(entity) else {
                return Some(DigPlayerRead::MissingState("PlayerUI"));
            };
            let Some(skills) = ecs.get::<crate::game::player::PlayerSkillsComp>(entity) else {
                return Some(DigPlayerRead::MissingState("PlayerSkillsComp"));
            };
            let Some(p_stats) = ecs.get::<crate::game::player::PlayerStats>(entity) else {
                return Some(DigPlayerRead::MissingState("PlayerStats"));
            };
            let Some(prog) = ecs.get::<crate::game::programmator::ProgrammatorState>(entity) else {
                return Some(DigPlayerRead::MissingState("ProgrammatorState"));
            };
            if ecs
                .get::<crate::game::player::PlayerFlags>(entity)
                .is_none()
            {
                return Some(DigPlayerRead::MissingState("PlayerFlags"));
            }
            if !programmatic && !prog.is_manual_control_allowed() {
                return Some(DigPlayerRead::Blocked);
            }
            // 1:1 ref `Session.cs:230` `DigHandler => TryAct(..., 200)`;
            // дефолт 200ms, тюнится `gameplay.cooldowns.dig_ms`.
            if !programmatic
                && cd.last_dig.elapsed().as_millis()
                    < u128::from(state.config.gameplay.cooldowns.dig_ms)
            {
                return Some(DigPlayerRead::Blocked);
            }
            if ui.current_window.is_some() && !programmatic {
                return Some(DigPlayerRead::Blocked);
            }
            let dp =
                crate::game::skills::get_player_skill_effect(&skills.states, SkillType::Digging);
            let mm = crate::game::skills::get_player_skill_effect(
                &skills.states,
                SkillType::MineGeneral,
            );
            let mine_by_crystal = [
                crate::game::skills::get_player_skill_effect(&skills.states, SkillType::MineGreen),
                crate::game::skills::get_player_skill_effect(&skills.states, SkillType::MineBlue),
                crate::game::skills::get_player_skill_effect(&skills.states, SkillType::MineRed),
                crate::game::skills::get_player_skill_effect(&skills.states, SkillType::MineViolet),
                crate::game::skills::get_player_skill_effect(&skills.states, SkillType::MineWhite),
                crate::game::skills::get_player_skill_effect(&skills.states, SkillType::MineCyan),
            ];
            let data = DigPlayerData {
                x: pos.x,
                y: pos.y,
                dir,
                dig_power: dp,
                mine_general: mm,
                mine_by_crystal,
                skin: p_stats.skin,
                clan_id: p_stats.clan_id.unwrap_or(0),
            };
            // Референс: `player.Move(player.x, player.y, dir)` сначала, потом `player.Bz()`.
            // Move с target == own position просто обновляет направление, не перемещает игрока.
            let dir_changed = {
                let Some(mut pos_mut) = ecs.get_mut::<crate::game::player::PlayerPosition>(entity)
                else {
                    return Some(DigPlayerRead::MissingState("PlayerPosition"));
                };
                let dir_changed = (0..=3).contains(&dir) && pos_mut.dir != dir;
                if dir_changed {
                    pos_mut.dir = dir;
                }
                dir_changed
            };
            if dir_changed {
                ecs.get_mut::<crate::game::player::PlayerFlags>(entity)
                    .expect("PlayerFlags checked before dig direction update")
                    .dirty = true;
            }
            {
                let Some(mut cd_mut) = ecs.get_mut::<crate::game::player::PlayerCooldowns>(entity)
                else {
                    return Some(DigPlayerRead::MissingState("PlayerCooldowns"));
                };
                cd_mut.last_dig = std::time::Instant::now();
            }
            {
                // C# `Move(own, dir)` при distance 0 начисляет Movement exp + @S
                // (Player.cs:441-452) — было пропущено, Movement не качался от копания.
                let Some(mut skills) = ecs.get_mut::<crate::game::player::PlayerSkillsComp>(entity)
                else {
                    return Some(DigPlayerRead::MissingState("PlayerSkillsComp"));
                };
                if let Some(sk) = ctx.add_skill_exp(&mut skills.states, "M", 1.0) {
                    send_u_packet(tx, sk.0, &sk.1);
                    ecs.get_mut::<crate::game::player::PlayerFlags>(entity)
                        .expect("PlayerFlags checked before dig movement exp")
                        .dirty = true;
                }
            }
            Some(DigPlayerRead::Ready(data))
        })
        .flatten();
    let Some(player_data) = player_data else {
        tracing::error!(player_id = %pid, "Player entity missing for dig");
        send_build_state_error(tx);
        return;
    };
    let player_data = match player_data {
        DigPlayerRead::Ready(player_data) => player_data,
        DigPlayerRead::Blocked => return,
        DigPlayerRead::MissingState(component) => {
            tracing::error!(player_id = %pid, component, "Player component missing for dig");
            send_build_state_error(tx);
            return;
        }
    };
    let DigPlayerData {
        x: px,
        y: py,
        dir: actual_dir,
        dig_power,
        mine_general,
        mine_by_crystal,
        skin,
        clan_id,
    } = player_data;

    let (dx, dy) = dir_offset(actual_dir);
    let (tgt_x, tgt_y) = (px + dx, py + dy);
    if !state.world.valid_coord(tgt_x, tgt_y) {
        return;
    }

    let cell = state.world.get_cell_typed(tgt_x, tgt_y);
    let (touch_damage, diggable) = {
        let defs = state.world.cell_defs();
        let p = defs.get_typed(cell);
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
    let exclude_self = if programmatic { None } else { Some(pid) };
    state.broadcast_hb_at(px, py, &[fx], exclude_self);

    // Fix 2: BOX (90) special case — pick up crystals and destroy.
    // H-1 фикс: in-memory `box_take` вместо sync SQLite на tick-пути.
    if cell.0 == cell_type::BOX {
        let pickup = state
            .modify_player(pid, |ecs, entity| {
                if ecs
                    .get::<crate::game::player::PlayerStats>(entity)
                    .is_none()
                {
                    return Some(BoxPickupResult::MissingState("PlayerStats"));
                }
                if ecs
                    .get::<crate::game::player::PlayerFlags>(entity)
                    .is_none()
                {
                    return Some(BoxPickupResult::MissingState("PlayerFlags"));
                }
                let Some(bc) = state.remove_box_cell(tgt_x, tgt_y) else {
                    return Some(BoxPickupResult::Empty);
                };
                let mut p_stats = ecs
                    .get_mut::<crate::game::player::PlayerStats>(entity)
                    .expect("PlayerStats checked before box pickup");
                for i in 0..6 {
                    p_stats.crystals[i] = p_stats.crystals[i].saturating_add(bc[i]);
                }
                let c_data = p_stats.crystals;
                ecs.get_mut::<crate::game::player::PlayerFlags>(entity)
                    .expect("PlayerFlags checked before box pickup")
                    .dirty = true;
                send_u_packet(tx, "@B", &basket(&c_data, 1).1);
                Some(BoxPickupResult::Picked(bc))
            })
            .flatten();
        let Some(pickup) = pickup else {
            tracing::error!(player_id = %pid, "Player entity missing for box pickup");
            send_build_state_error(tx);
            return;
        };
        match pickup {
            BoxPickupResult::Picked(bc) => {
                // C# `Player.GetBox` → `HBChatPacket(0, x, y, "+ " + AllCrys)` — бабл
                // с суммой кристаллов над боксом, ТОЛЬКО своему соединению (bot_id=0).
                let total: i64 = bc.iter().sum();
                let bubble = crate::protocol::packets::hb_chat(
                    0,
                    net_u16_nonneg(tgt_x),
                    net_u16_nonneg(tgt_y),
                    &format!("+ {total}"),
                );
                let _ = tx.send(crate::net::session::wire::encode_hb_bundle(
                    &crate::protocol::packets::hb_bundle(&[bubble]).1,
                ));
            }
            BoxPickupResult::Empty => {}
            BoxPickupResult::MissingState(component) => {
                tracing::error!(player_id = %pid, component, "Player component missing for box pickup");
                send_build_state_error(tx);
                return;
            }
        }
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
        state.broadcast_hb_at(px, py, &[bot], exclude_self);
        return;
    }

    if !diggable {
        return;
    }

    // Fix 3: MilitaryBlock (81) special case — fixed 1.0 damage, no multiplier, no crystal/exp/FX2.
    if cell.0 == cell_type::MILITARY_BLOCK {
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
        state.broadcast_hb_at(px, py, &[bot], exclude_self);
        return;
    }

    let cry_idx = cell.crystal_type();
    let mutation_state = state.query_player(pid, |ecs, entity| {
        if ecs
            .get::<crate::game::player::PlayerSkillsComp>(entity)
            .is_none()
        {
            return DigMutationRead::MissingState("PlayerSkillsComp");
        }
        if ecs
            .get::<crate::game::player::PlayerFlags>(entity)
            .is_none()
        {
            return DigMutationRead::MissingState("PlayerFlags");
        }
        if cry_idx.is_some()
            && ecs
                .get::<crate::game::player::PlayerStats>(entity)
                .is_none()
        {
            return DigMutationRead::MissingState("PlayerStats");
        }
        DigMutationRead::Ready
    });
    match mutation_state {
        Some(DigMutationRead::Ready) => {}
        Some(DigMutationRead::MissingState(component)) => {
            tracing::error!(player_id = %pid, component, "Player component missing for dig mutation");
            send_build_state_error(tx);
            return;
        }
        None => {
            tracing::error!(player_id = %pid, "Player entity missing for dig mutation");
            send_build_state_error(tx);
            return;
        }
    }

    let hit = if cell.is_crystal() {
        1.0
    } else {
        (dig_power / DIG_POWER_DIVISOR).max(MIN_HIT_POWER)
    };
    let destroyed = state.world.damage_cell(tgt_x, tgt_y, hit);

    // D8+D9: Crystal mining happens on EVERY hit (1:1 with C# Player.Bz → Mine(cell,x,y)).
    // Dig exp happens only on destroy. MineGeneral exp happens every hit with crystals.
    let mined_amount = cry_idx.map_or(0_i64, |idx| {
        let mined_yield = CrystalMineYield::calculate(
            mine_general,
            mine_by_crystal[idx],
            cell.crystal_multiplier(),
            ctx,
        );

        // Add crystals + MineGeneral exp on every hit.
        let mined = state
            .modify_player(pid, |ecs, entity| {
                if ecs
                    .get::<crate::game::player::PlayerStats>(entity)
                    .is_none()
                    || ecs
                        .get::<crate::game::player::PlayerSkillsComp>(entity)
                        .is_none()
                    || ecs
                        .get::<crate::game::player::PlayerFlags>(entity)
                        .is_none()
                {
                    return None;
                }
                {
                    let mut p_stats = ecs
                        .get_mut::<crate::game::player::PlayerStats>(entity)
                        .expect("PlayerStats checked before crystal mining");
                    p_stats.crystals[idx] += mined_yield.final_amount;
                    let c_data = p_stats.crystals;
                    send_u_packet(tx, "@B", &basket(&c_data, 1).1);
                }
                {
                    let mut skills = ecs
                        .get_mut::<crate::game::player::PlayerSkillsComp>(entity)
                        .expect("PlayerSkillsComp checked before crystal mining");
                    if let Some(sk) =
                        ctx.add_skill_exp(&mut skills.states, "m", mined_yield.exp_amount)
                    {
                        send_u_packet(tx, sk.0, &sk.1);
                    }
                }
                {
                    let mut flags = ecs
                        .get_mut::<crate::game::player::PlayerFlags>(entity)
                        .expect("PlayerFlags checked before crystal mining");
                    flags.dirty = true;
                }
                Some(())
            })
            .flatten()
            .is_some();
        if !mined {
            tracing::error!(player_id = %pid, "Player state missing for crystal mining");
            send_build_state_error(tx);
            return 0;
        }

        // AddDob tracks the same effective economy volume that reached inventory.
        crate::game::market::add_dob(state, idx, mined_yield.final_amount);

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
            (mined_yield.final_amount.min(255)) as u8,
            color_remapped,
        );
        // C# `SendDFToBots` шлёт через `vChunksAroundEx()` — 5×5 чанков ВКЛЮЧАЯ
        // свой (Entity.cs:43, центр входит), т.е. сам копающий тоже получает FX
        // добычи кристаллов. Раньше exclude=Some(pid) → игрок не видел анимацию
        // «сколько выкопал». Включаем себя (None).
        state.broadcast_hb_at(px, py, &[mine_fx], None);

        mined_yield.final_amount
    });
    let _ = mined_amount;

    // Boulder push на КАЖДЫЙ удар (не только при разрушении), 1:1 C# `Player.Bz`
    // (390-404) → `World.MoveCell`: валун ПЕРЕМЕЩАЕТСЯ на клетку в направлении копки,
    // если она пуста. `MoveCell` ОЧИЩАЕТ источник и ПЕРЕНОСИТ durability — раньше
    // Rust только `set_cell(dest)` без очистки источника → валун ДУБЛИРОВАЛСЯ
    // (эксплойт: копай валун → плодятся). Гейт `!destroyed`: при разрушении
    // `damage_cell` уже очистил клетку, а `cell` захвачен ДО удара (иначе воскресим);
    // C# в этом случае MoveCell'ит пустую клетку = no-op.
    let pushed_boulder = if cell.is_boulder() && !destroyed {
        let (bx, by) = (tgt_x + dx, tgt_y + dy);
        if state.world.valid_coord(bx, by) && state.world.is_empty(bx, by) {
            // durability читаем ПОСЛЕ damage_cell (как C# GetDurability после DamageCell).
            let dur = state.world.get_durability(tgt_x, tgt_y);
            state.world.destroy(tgt_x, tgt_y);
            broadcast_cell_update(state, tgt_x, tgt_y);
            state.world.write_world_cell(
                bx,
                by,
                crate::world::WorldCell {
                    cell_type: cell,
                    durability: dur,
                },
            );
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
            let mut skills = ecs.get_mut::<crate::game::player::PlayerSkillsComp>(entity)?;
            if let Some(sk) = ctx.add_skill_exp(&mut skills.states, "d", 1.0) {
                send_u_packet(tx, sk.0, &sk.1);
                ecs.get_mut::<crate::game::player::PlayerFlags>(entity)?
                    .dirty = true;
            }
            Some(())
        });
    }

    if destroyed {
        // Dig exp only on destroy (1:1 with C# OnDestroy → AddExp("d")).
        state.modify_player(pid, |ecs, entity| {
            {
                // C# `Skill.AddExp` всегда шлёт @S при изменении pct — было пропущено
                // на dig-destroy (полоса Digging не обновлялась до след. @S-события).
                let mut skills = ecs.get_mut::<crate::game::player::PlayerSkillsComp>(entity)?;
                if let Some(sk) = ctx.add_skill_exp(&mut skills.states, "d", 1.0) {
                    send_u_packet(tx, sk.0, &sk.1);
                }
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
    state.broadcast_hb_at(px, py, &[bot], exclude_self);
}

pub fn handle_build(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    bld: &XbldClient<'_>,
    programmatic: bool,
) {
    let ctx = crate::game::ExpContext::from_state(state);
    // Fix 11: Extract player data including clan_id, skills, and build cooldown check.
    let build_data = state
        .modify_player(pid, |ecs, entity| {
            let Some(pos) = ecs.get::<crate::game::player::PlayerPosition>(entity) else {
                return Some(BuildPlayerRead::MissingState("PlayerPosition"));
            };
            let Some(ui) = ecs.get::<crate::game::player::PlayerUI>(entity) else {
                return Some(BuildPlayerRead::MissingState("PlayerUI"));
            };
            let Some(cd) = ecs.get::<crate::game::player::PlayerCooldowns>(entity) else {
                return Some(BuildPlayerRead::MissingState("PlayerCooldowns"));
            };
            let Some(skills) = ecs.get::<crate::game::player::PlayerSkillsComp>(entity) else {
                return Some(BuildPlayerRead::MissingState("PlayerSkillsComp"));
            };
            let Some(p_stats) = ecs.get::<crate::game::player::PlayerStats>(entity) else {
                return Some(BuildPlayerRead::MissingState("PlayerStats"));
            };
            let Some(prog) = ecs.get::<crate::game::programmator::ProgrammatorState>(entity) else {
                return Some(BuildPlayerRead::MissingState("ProgrammatorState"));
            };
            if !programmatic && ui.current_window.is_some() {
                return Some(BuildPlayerRead::Blocked);
            }
            if !programmatic && !prog.is_manual_control_allowed() {
                return Some(BuildPlayerRead::Blocked);
            }
            // 1:1 ref `Session.cs:233` `BuildHandler => TryAct(..., 200)`;
            // дефолт 200ms, тюнится `gameplay.cooldowns.build_ms`.
            if !programmatic
                && cd.last_build.elapsed().as_millis()
                    < u128::from(state.config.gameplay.cooldowns.build_ms)
            {
                return Some(BuildPlayerRead::Blocked);
            }
            let (px, py, pdir) = (pos.x, pos.y, pos.dir);
            let clan_id = p_stats.clan_id.unwrap_or(0);

            // Fix 12: Crystal cost from skill.Effect.
            // Fix 16: Durability from skill AdditionalEffect (on_bld_hp).
            let (skill_type, hp_skill_type) = match bld.block_type {
                "G" => (SkillType::BuildGreen, Some(SkillType::BuildGreen)),
                "R" => (SkillType::BuildRoad, None),
                "O" => (SkillType::BuildStructure, None),
                "V" => (SkillType::BuildWar, Some(SkillType::BuildWar)),
                _ => return Some(BuildPlayerRead::Blocked),
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
            let yellow_effect = get_player_skill_effect(&skills.states, SkillType::BuildYellow);
            let yellow_hp = skills
                .states
                .find(SkillType::BuildYellow.code())
                .map_or(1.0_f32, |s| s.level as f32);
            let red_effect = get_player_skill_effect(&skills.states, SkillType::BuildRed);
            let red_hp = skills
                .states
                .find(SkillType::BuildRed.code())
                .map_or(1.0_f32, |s| s.level as f32);

            let dir_changed = {
                let Some(mut pos_mut) = ecs.get_mut::<crate::game::player::PlayerPosition>(entity)
                else {
                    return Some(BuildPlayerRead::MissingState("PlayerPosition"));
                };
                let dir_changed = (0..=3).contains(&bld.direction) && pos_mut.dir != bld.direction;
                if dir_changed {
                    pos_mut.dir = bld.direction;
                }
                dir_changed
            };
            if dir_changed {
                ecs.get_mut::<crate::game::player::PlayerFlags>(entity)
                    .expect("PlayerFlags checked before build direction update")
                    .dirty = true;
            }
            {
                let Some(mut cd_mut) = ecs.get_mut::<crate::game::player::PlayerCooldowns>(entity)
                else {
                    return Some(BuildPlayerRead::MissingState("PlayerCooldowns"));
                };
                cd_mut.last_build = std::time::Instant::now();
            }
            Some(BuildPlayerRead::Ready(BuildPlayerData {
                x: px,
                y: py,
                dir: pdir,
                clan_id,
                skill_effect: effect,
                skill_hp: hp_effect,
                yellow_effect,
                yellow_hp,
                red_effect,
                red_hp,
            }))
        })
        .flatten();
    let Some(build_data) = build_data else {
        tracing::error!(player_id = %pid, "Player entity missing for build");
        send_build_state_error(tx);
        return;
    };
    let build_data = match build_data {
        BuildPlayerRead::Ready(build_data) => build_data,
        BuildPlayerRead::Blocked => return,
        BuildPlayerRead::MissingState(component) => {
            tracing::error!(player_id = %pid, component, "Player component missing for build");
            send_build_state_error(tx);
            return;
        }
    };
    let BuildPlayerData {
        x: px,
        y: py,
        dir: pdir,
        clan_id,
        skill_effect: build_skill_effect,
        skill_hp: build_skill_hp,
        yellow_effect: build_yellow_effect,
        yellow_hp: build_yellow_hp,
        red_effect: build_red_effect,
        red_hp: build_red_hp,
    } = build_data;

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
    if state.has_building_origin(tgt_x, tgt_y) {
        return;
    }

    let cur = state.world.get_cell_typed(tgt_x, tgt_y);
    let binding = state.world.cell_defs();
    let prop = binding.get_typed(cur);

    // Fix 12: cost = effect.max(1.0) as i64.
    let cost = build_skill_effect.max(1.0) as i64;
    // Fix 16: durability from on_bld_hp.
    let durability = build_skill_hp;

    let mut placed_skill: Option<SkillType> = None;

    match bld.block_type {
        "G" => {
            if prop.cell_is_empty() || prop.is_sand() {
                if try_spend_crystal(state, tx, pid, 0, cost) {
                    place_world_cell(state, tgt_x, tgt_y, cell_type::GREEN_BLOCK, durability);
                    placed_skill = Some(SkillType::BuildGreen);
                }
            } else if cur.is(cell_type::GREEN_BLOCK) {
                // Upgrading green → yellow uses BuildYellow skill effect/cost.
                let y_cost = build_yellow_effect.max(1.0) as i64;
                if try_spend_crystal(state, tx, pid, 4, y_cost) {
                    // D7: Yellow upgrade adds durability to existing (C# GetDurability + AdditionalEffect).
                    let existing_dur = state.world.get_durability(tgt_x, tgt_y);
                    place_world_cell(
                        state,
                        tgt_x,
                        tgt_y,
                        cell_type::YELLOW_BLOCK,
                        existing_dur + build_yellow_hp,
                    );
                    placed_skill = Some(SkillType::BuildYellow);
                }
            } else if cur.is(cell_type::YELLOW_BLOCK) {
                // Upgrading yellow → red uses BuildRed skill effect/cost.
                let r_cost = build_red_effect.max(1.0) as i64;
                if try_spend_crystal(state, tx, pid, 2, r_cost) {
                    // D7: Red upgrade adds durability to existing (C# GetDurability + AdditionalEffect).
                    let existing_dur = state.world.get_durability(tgt_x, tgt_y);
                    place_world_cell(
                        state,
                        tgt_x,
                        tgt_y,
                        cell_type::RED_BLOCK,
                        existing_dur + build_red_hp,
                    );
                    placed_skill = Some(SkillType::BuildRed);
                }
            }
        }
        "R" => {
            if is_truly_empty(cur) && try_spend_crystal(state, tx, pid, 0, cost) {
                place_world_cell(state, tgt_x, tgt_y, cell_type::ROAD, durability);
                placed_skill = Some(SkillType::BuildRoad);
            }
        }
        "O" => {
            if (prop.cell_is_empty() || prop.is_sand())
                && try_spend_crystal(state, tx, pid, 0, cost)
            {
                // D5: опора ломается с первого удара (durability 0, 1:1 C#).
                // damage_cell рушит при `d - dmg <= 0`, поэтому 0 = разрушение
                // с любого удара. Не используем build_skill_hp (он делал опоры прочнее).
                place_world_cell(state, tgt_x, tgt_y, cell_type::SUPPORT, 0.0);
                placed_skill = Some(SkillType::BuildStructure);
            }
        }
        "V" if is_truly_empty(cur) && try_spend_crystal(state, tx, pid, 5, cost) => {
            place_block(state, tgt_x, tgt_y, cell_type::MILITARY_BLOCK_FRAME);
            // Schedule conversion: frame→block after 10 ticks (1:1 C# StupidAction).
            let mut ecs = state.ecs.write();
            ecs.resource_mut::<crate::game::PendingCellConversions>()
                .0
                .push(crate::game::PendingConversion {
                    pos: (tgt_x, tgt_y).into(),
                    ticks_left: 10,
                    required_cell: crate::world::CellType(cell_type::MILITARY_BLOCK_FRAME),
                    target_cell: crate::world::CellType(cell_type::MILITARY_BLOCK),
                    durability,
                    owner_pid: pid,
                });
            placed_skill = Some(SkillType::BuildWar);
        }
        _ => {}
    }

    // Fix 15: Build skill exp after successful placement.
    if let Some(skill) = placed_skill {
        state.modify_player(pid, |ecs, entity| {
            // C# `Skill.AddExp` всегда шлёт @S при изменении pct — было пропущено
            // на build (полоса Build* не обновлялась до след. @S-события).
            let mut skills = ecs.get_mut::<crate::game::player::PlayerSkillsComp>(entity)?;
            if let Some(sk) = ctx.add_skill_exp(&mut skills.states, skill.code(), 1.0) {
                send_u_packet(tx, sk.0, &sk.1);
            }
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
    let spent = state.modify_player(pid, |ecs, entity| {
        if ecs
            .get::<crate::game::player::PlayerStats>(entity)
            .is_none()
            || ecs
                .get::<crate::game::player::PlayerFlags>(entity)
                .is_none()
        {
            return None;
        }
        let mut s = ecs
            .get_mut::<crate::game::player::PlayerStats>(entity)
            .expect("PlayerStats checked before crystal spend");
        if s.crystals[idx] >= amount {
            s.crystals[idx] -= amount;
            let c_data = s.crystals;
            ecs.get_mut::<crate::game::player::PlayerFlags>(entity)
                .expect("PlayerFlags checked before crystal spend")
                .dirty = true;
            send_u_packet(tx, "@B", &basket(&c_data, 1).1);
            Some(true)
        } else {
            Some(false)
        }
    });
    let Some(spent) = spent.flatten() else {
        tracing::error!(player_id = %pid, "Player stats missing for crystal spend");
        send_build_state_error(tx);
        return false;
    };
    spent
}

pub fn broadcast_cell_update(state: &Arc<GameState>, x: i32, y: i32) {
    let cell = state.world.get_cell_typed(x, y);
    let sub = hb_cell(net_u16_nonneg(x), net_u16_nonneg(y), cell.0);
    state.broadcast_hb_at(x, y, &[sub], None);
}

fn place_block(state: &Arc<GameState>, x: i32, y: i32, cell: u8) {
    state
        .world
        .set_cell_typed(x, y, crate::world::CellType(cell));
    broadcast_cell_update(state, x, y);
}

fn place_world_cell(state: &Arc<GameState>, x: i32, y: i32, cell: u8, durability: f32) {
    state.world.write_world_cell(
        x,
        y,
        crate::world::WorldCell {
            cell_type: crate::world::CellType(cell),
            durability,
        },
    );
    broadcast_cell_update(state, x, y);
}

pub const fn is_truly_empty(cell: crate::world::CellType) -> bool {
    cell.is(cell_type::NOTHING) || cell.is(cell_type::EMPTY)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use tokio::sync::mpsc::UnboundedReceiver;

    struct TestState {
        state: Arc<GameState>,
        player: crate::db::PlayerRow,
        world_name: String,
        db_path: std::path::PathBuf,
    }

    impl TestState {
        fn cleanup(&self) {
            let dir = std::env::temp_dir();
            let _ = std::fs::remove_file(&self.db_path);
            let _ = std::fs::remove_file(self.db_path.with_extension("db-wal"));
            let _ = std::fs::remove_file(self.db_path.with_extension("db-shm"));
            let _ = std::fs::remove_file(dir.join(format!("{}_v2.map", self.world_name)));
            let _ = std::fs::remove_file(dir.join(format!("{}_durability.mapb", self.world_name)));
        }
    }

    async fn make_test_state(label: &str) -> TestState {
        let dir = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_path = dir.join(format!("{label}_{}_{}.db", std::process::id(), nonce));
        let _ = std::fs::remove_file(&db_path);
        let database = crate::db::Database::open(&db_path).await.unwrap();
        let player = database
            .create_player("build-user", "p", "h")
            .await
            .unwrap();

        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();
        let world_name = format!("{label}_world_{}_{}", std::process::id(), nonce);
        let world = crate::world::World::new(&world_name, 2, 2, cell_defs, &dir).unwrap();
        let config = crate::config::Config {
            world_name: world_name.clone(),
            port: 8090,
            world_chunks_w: 2,
            world_chunks_h: 2,
            data_dir: dir.to_string_lossy().to_string(),
            logging: crate::config::LoggingConfig::default(),
            cron: crate::config::CronConfig::default(),
            gameplay: crate::config::GameplayConfig::default(),
        };
        let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config)
            .await
            .unwrap();

        TestState {
            state,
            player,
            world_name,
            db_path,
        }
    }

    fn drain_events(rx: &mut UnboundedReceiver<Vec<u8>>) -> Vec<(String, Vec<u8>)> {
        let mut events = Vec::new();
        while let Ok(frame) = rx.try_recv() {
            let mut buf = BytesMut::from(&frame[..]);
            let packet = crate::protocol::Packet::try_decode(&mut buf)
                .expect("valid packet")
                .expect("decoded packet");
            events.push((packet.event_str().to_owned(), packet.payload.to_vec()));
        }
        events
    }

    fn basket_green(payload: &[u8]) -> i64 {
        std::str::from_utf8(payload)
            .unwrap()
            .split(':')
            .next()
            .unwrap()
            .parse()
            .unwrap()
    }

    fn directed_mine_fx_amount(payload: &[u8]) -> Option<i64> {
        let mut i = 0;
        while i + 9 <= payload.len() {
            if payload[i] != b'D' {
                return None;
            }
            let fx = payload[i + 1];
            let amount = payload[i + 2];
            if fx == 2 {
                return Some(i64::from(amount));
            }
            i += 9;
        }
        None
    }

    #[tokio::test]
    async fn crystal_spend_missing_player_stats_is_explicit_error_not_insufficient_resources() {
        let test = make_test_state("crystal_spend_missing_stats").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerStats>();
        }

        assert!(!try_spend_crystal(&test.state, &tx, pid, 0, 1));

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));

        test.cleanup();
    }

    #[tokio::test]
    async fn crystal_spend_missing_player_flags_is_explicit_error_without_crystal_mutation() {
        let test = make_test_state("crystal_spend_missing_flags").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.get_mut::<crate::game::player::PlayerStats>(entity)
                .unwrap()
                .crystals[0] = 10;
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerFlags>();
        }

        assert!(!try_spend_crystal(&test.state, &tx, pid, 0, 1));

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));
        assert_eq!(player_crystal(&test.state, pid, 0), 10);

        test.cleanup();
    }

    #[tokio::test]
    async fn crystal_spend_success_marks_player_dirty() {
        let test = make_test_state("crystal_spend_dirty").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.get_mut::<crate::game::player::PlayerStats>(entity)
                .unwrap()
                .crystals[0] = 10;
            ecs.get_mut::<crate::game::player::PlayerFlags>(entity)
                .unwrap()
                .dirty = false;
        }

        assert!(try_spend_crystal(&test.state, &tx, pid, 0, 1));

        assert_eq!(player_crystal(&test.state, pid, 0), 9);
        assert!(player_dirty(&test.state, pid));
        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "@B");

        test.cleanup();
    }

    #[tokio::test]
    async fn dig_missing_player_skills_is_explicit_error_not_silent_noop() {
        let test = make_test_state("dig_missing_skills").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut cd = ecs
                .get_mut::<crate::game::player::PlayerCooldowns>(entity)
                .unwrap();
            cd.last_dig -= Duration::from_millis(500);
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerSkillsComp>();
        }

        handle_dig(&test.state, &tx, pid, 0, false);

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));

        test.cleanup();
    }

    #[tokio::test]
    async fn programmatic_dig_ignores_open_gui_but_manual_dig_does_not() {
        let test = make_test_state("programmatic_dig_window").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut pos = ecs
                .get_mut::<crate::game::player::PlayerPosition>(entity)
                .unwrap();
            pos.x = 10;
            pos.y = 10;
            pos.dir = 0;
            ecs.get_mut::<crate::game::player::PlayerUI>(entity)
                .unwrap()
                .current_window = Some("prog".to_string());
        }
        test.state.world.set_cell(10, 11, cell_type::GREEN);
        test.state.world.set_durability(10, 11, 100.0);

        handle_dig(&test.state, &tx, pid, 0, false);
        assert!(!drain_events(&mut rx).iter().any(|(event, _)| event == "@B"));

        {
            let mut ecs = test.state.ecs.write();
            ecs.get_mut::<crate::game::player::PlayerCooldowns>(entity)
                .unwrap()
                .last_dig -= Duration::from_millis(500);
        }
        handle_dig(&test.state, &tx, pid, 0, true);

        let events = drain_events(&mut rx);
        assert!(
            events.iter().any(|(event, _)| event == "@B"),
            "programmatic Dig must call Bz even while programmator GUI state is open"
        );
        assert!(
            events.iter().any(|(event, _)| event == "HB"),
            "programmatic Dig must send self HB for programmator visual sync"
        );

        test.cleanup();
    }

    #[tokio::test]
    async fn crystal_mine_fx_uses_same_final_amount_as_inventory() {
        let test = make_test_state("crystal_mine_fx_final_amount").await;
        {
            let now = crate::time::now_unix();
            test.state
                .active_events
                .write()
                .list
                .push(crate::game::ActiveEvent {
                    id: "drop_x100".to_string(),
                    title: "drop_x100".to_string(),
                    starts_at: now - 1,
                    ends_at: now + 60,
                    xp_mult: 1.0,
                    drop_mult: 100.0,
                });
        }
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut pos = ecs
                .get_mut::<crate::game::player::PlayerPosition>(entity)
                .unwrap();
            pos.x = 10;
            pos.y = 10;
            pos.dir = 0;
            ecs.get_mut::<crate::game::player::PlayerCooldowns>(entity)
                .unwrap()
                .last_dig -= Duration::from_millis(500);
        }
        test.state.world.set_cell(10, 11, cell_type::GREEN);
        test.state.world.set_durability(10, 11, 100.0);

        handle_dig(&test.state, &tx, pid, 0, false);

        let events = drain_events(&mut rx);
        let basket_amount = events
            .iter()
            .find(|(event, _)| event == "@B")
            .map(|(_, payload)| basket_green(payload))
            .expect("@B after crystal mine");
        let fx_amount = events
            .iter()
            .filter(|(event, _)| event == "HB")
            .find_map(|(_, payload)| directed_mine_fx_amount(payload))
            .expect("mine D FX after crystal mine");

        assert_eq!(fx_amount, basket_amount);
        assert!(
            basket_amount >= 100,
            "test must exercise event/drop multiplier, got {basket_amount}"
        );

        test.cleanup();
    }

    #[tokio::test]
    async fn build_turn_marks_player_dirty_even_when_build_does_not_happen() {
        let test = make_test_state("build_turn_dirty").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut pos = ecs
                .get_mut::<crate::game::player::PlayerPosition>(entity)
                .unwrap();
            pos.x = 10;
            pos.y = 10;
            pos.dir = 0;
            ecs.get_mut::<crate::game::player::PlayerFlags>(entity)
                .unwrap()
                .dirty = false;
        }
        test.state.world.set_cell(9, 10, cell_type::EMPTY);

        let bld = XbldClient {
            direction: 1,
            block_type: "G",
        };
        handle_build(&test.state, &tx, pid, &bld, false);

        let (dir, dirty) = test
            .state
            .query_player_opt(pid, |ecs, entity| {
                let pos = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
                let flags = ecs.get::<crate::game::player::PlayerFlags>(entity)?;
                Some((pos.dir, flags.dirty))
            })
            .unwrap();
        assert_eq!(dir, 1);
        assert!(dirty);

        test.cleanup();
    }

    #[tokio::test]
    async fn dig_turn_marks_player_dirty_even_without_crystal_gain() {
        let test = make_test_state("dig_turn_dirty").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut pos = ecs
                .get_mut::<crate::game::player::PlayerPosition>(entity)
                .unwrap();
            pos.x = 10;
            pos.y = 10;
            pos.dir = 0;
            ecs.get_mut::<crate::game::player::PlayerCooldowns>(entity)
                .unwrap()
                .last_dig -= Duration::from_millis(500);
            ecs.get_mut::<crate::game::player::PlayerFlags>(entity)
                .unwrap()
                .dirty = false;
        }
        test.state.world.set_cell(9, 10, cell_type::EMPTY);

        handle_dig(&test.state, &tx, pid, 1, false);

        let (dir, dirty) = test
            .state
            .query_player_opt(pid, |ecs, entity| {
                let pos = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
                let flags = ecs.get::<crate::game::player::PlayerFlags>(entity)?;
                Some((pos.dir, flags.dirty))
            })
            .unwrap();
        assert_eq!(dir, 1);
        assert!(dirty);

        test.cleanup();
    }

    #[tokio::test]
    async fn dig_missing_player_flags_is_explicit_error_before_world_damage() {
        let test = make_test_state("dig_missing_flags_no_damage").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut pos = ecs
                .get_mut::<crate::game::player::PlayerPosition>(entity)
                .unwrap();
            pos.x = 10;
            pos.y = 10;
            pos.dir = 0;
            let mut cd = ecs
                .get_mut::<crate::game::player::PlayerCooldowns>(entity)
                .unwrap();
            cd.last_dig -= Duration::from_millis(500);
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerFlags>();
        }
        test.state.world.set_cell(10, 11, cell_type::ROCK);
        test.state.world.set_durability(10, 11, 0.0);

        handle_dig(&test.state, &tx, pid, 0, false);

        assert_eq!(test.state.world.get_cell(10, 11), cell_type::ROCK);
        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));

        test.cleanup();
    }

    #[tokio::test]
    async fn dig_crystal_missing_player_flags_is_explicit_error_without_crystal_gain() {
        let test = make_test_state("dig_crystal_missing_flags").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut pos = ecs
                .get_mut::<crate::game::player::PlayerPosition>(entity)
                .unwrap();
            pos.x = 10;
            pos.y = 10;
            pos.dir = 0;
            let mut cd = ecs
                .get_mut::<crate::game::player::PlayerCooldowns>(entity)
                .unwrap();
            cd.last_dig -= Duration::from_millis(500);
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerFlags>();
        }
        test.state.world.set_cell(10, 11, cell_type::GREEN);
        test.state.world.set_durability(10, 11, 100.0);

        handle_dig(&test.state, &tx, pid, 0, false);

        assert_eq!(player_crystal(&test.state, pid, 0), 0);
        assert_eq!(test.state.world.get_cell(10, 11), cell_type::GREEN);
        let events = drain_events(&mut rx);
        assert!(events.iter().any(|(event, payload)| {
            event == "OK"
                && std::str::from_utf8(payload)
                    .is_ok_and(|message| message.contains("Состояние игрока недоступно."))
        }));
        assert!(!events.iter().any(|(event, _)| event == "@B"));

        test.cleanup();
    }

    #[tokio::test]
    async fn dig_box_missing_player_flags_keeps_box_and_sends_explicit_error() {
        let test = make_test_state("dig_box_missing_flags").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut pos = ecs
                .get_mut::<crate::game::player::PlayerPosition>(entity)
                .unwrap();
            pos.x = 10;
            pos.y = 10;
            pos.dir = 0;
            let mut cd = ecs
                .get_mut::<crate::game::player::PlayerCooldowns>(entity)
                .unwrap();
            cd.last_dig -= Duration::from_millis(500);
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerFlags>();
        }
        test.state.put_box_cell(10, 11, [3, 2, 1, 0, 0, 0]);

        handle_dig(&test.state, &tx, pid, 0, false);

        assert_eq!(player_crystal(&test.state, pid, 0), 0);
        assert_eq!(test.state.world.get_cell(10, 11), cell_type::BOX);
        assert_eq!(test.state.box_take(10, 11), Some([3, 2, 1, 0, 0, 0]));
        let events = drain_events(&mut rx);
        assert!(events.iter().any(|(event, payload)| {
            event == "OK"
                && std::str::from_utf8(payload)
                    .is_ok_and(|message| message.contains("Состояние игрока недоступно."))
        }));
        assert!(!events.iter().any(|(event, _)| event == "@B"));

        test.cleanup();
    }

    #[tokio::test]
    async fn crystal_spend_insufficient_resources_stays_quiet_false() {
        let test = make_test_state("crystal_spend_insufficient").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        assert!(!try_spend_crystal(&test.state, &tx, pid, 0, 1));

        let events = drain_events(&mut rx);
        assert!(events.is_empty());

        test.cleanup();
    }

    #[tokio::test]
    async fn build_missing_player_skills_is_explicit_error_not_blocked_fallback() {
        let test = make_test_state("build_missing_skills").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut cd = ecs
                .get_mut::<crate::game::player::PlayerCooldowns>(entity)
                .unwrap();
            cd.last_build -= Duration::from_millis(500);
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerSkillsComp>();
        }

        let bld = XbldClient {
            direction: 0,
            block_type: "G",
        };
        handle_build(&test.state, &tx, pid, &bld, false);

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));

        test.cleanup();
    }

    #[tokio::test]
    async fn build_yellow_upgrade_missing_skills_does_not_use_default_cost_or_mutate_world() {
        let test = make_test_state("build_yellow_upgrade_missing_skills").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut pos = ecs
                .get_mut::<crate::game::player::PlayerPosition>(entity)
                .unwrap();
            pos.x = 10;
            pos.y = 10;
            pos.dir = 0;
            let mut cd = ecs
                .get_mut::<crate::game::player::PlayerCooldowns>(entity)
                .unwrap();
            cd.last_build -= Duration::from_millis(500);
            let mut stats = ecs
                .get_mut::<crate::game::player::PlayerStats>(entity)
                .unwrap();
            stats.crystals[4] = 10;
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerSkillsComp>();
        }
        test.state.world.set_cell(10, 11, cell_type::GREEN_BLOCK);
        test.state.world.set_durability(10, 11, 5.0);

        let bld = XbldClient {
            direction: 0,
            block_type: "G",
        };
        handle_build(&test.state, &tx, pid, &bld, false);

        assert_eq!(player_crystal(&test.state, pid, 4), 10);
        assert_eq!(test.state.world.get_cell(10, 11), cell_type::GREEN_BLOCK);
        assert!((test.state.world.get_durability(10, 11) - 5.0).abs() < f32::EPSILON);
        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));

        test.cleanup();
    }

    #[tokio::test]
    async fn build_red_upgrade_missing_skills_does_not_use_default_cost_or_mutate_world() {
        let test = make_test_state("build_red_upgrade_missing_skills").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut pos = ecs
                .get_mut::<crate::game::player::PlayerPosition>(entity)
                .unwrap();
            pos.x = 10;
            pos.y = 10;
            pos.dir = 0;
            let mut cd = ecs
                .get_mut::<crate::game::player::PlayerCooldowns>(entity)
                .unwrap();
            cd.last_build -= Duration::from_millis(500);
            let mut stats = ecs
                .get_mut::<crate::game::player::PlayerStats>(entity)
                .unwrap();
            stats.crystals[2] = 10;
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerSkillsComp>();
        }
        test.state.world.set_cell(10, 11, cell_type::YELLOW_BLOCK);
        test.state.world.set_durability(10, 11, 7.0);

        let bld = XbldClient {
            direction: 0,
            block_type: "G",
        };
        handle_build(&test.state, &tx, pid, &bld, false);

        assert_eq!(player_crystal(&test.state, pid, 2), 10);
        assert_eq!(test.state.world.get_cell(10, 11), cell_type::YELLOW_BLOCK);
        assert!((test.state.world.get_durability(10, 11) - 7.0).abs() < f32::EPSILON);
        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));

        test.cleanup();
    }

    #[tokio::test]
    async fn build_cooldown_block_stays_quiet_noop() {
        let test = make_test_state("build_cooldown_quiet").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let bld = XbldClient {
            direction: 0,
            block_type: "G",
        };
        handle_build(&test.state, &tx, PlayerId(test.player.id), &bld, false);

        let events = drain_events(&mut rx);
        assert!(events.is_empty());

        test.cleanup();
    }

    fn player_crystal(state: &Arc<GameState>, pid: PlayerId, idx: usize) -> i64 {
        state
            .query_player_opt(pid, |ecs, entity| {
                Some(
                    ecs.get::<crate::game::player::PlayerStats>(entity)?
                        .crystals[idx],
                )
            })
            .unwrap()
    }

    fn player_dirty(state: &Arc<GameState>, pid: PlayerId) -> bool {
        state
            .query_player_opt(pid, |ecs, entity| {
                Some(ecs.get::<crate::game::player::PlayerFlags>(entity)?.dirty)
            })
            .unwrap()
    }
}
