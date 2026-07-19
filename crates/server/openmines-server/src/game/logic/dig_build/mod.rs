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
    clippy::significant_drop_tightening
)]
//! Копание клеток и установка блоков (Xdig, Xbld).
use crate::game::logic::death::hurt_player_pure;
use crate::game::skills::{
    OnBld, OnDig, OnDigCrys, PlayerSkills as SkillHooks, SkillType, get_player_skill_effect,
};

use crate::game::direction::dir_offset;
use crate::game::{GameState, PlayerId};
use crate::net::session::util::{net_u8_clamped, net_u16_nonneg};
use crate::net::session::wire::PacketSink;
use crate::net::session::wire::send_u_packet;
use crate::protocol::packets::{
    XbldClient, basket, hb_bot, hb_crystal_mine_fx, hb_dig_fx, ok_message,
};
use crate::world::WorldProvider;
use crate::world::cells::cell_type;
use std::sync::Arc;

/// Делитель силы копания (C# `Player.cs`: `digPower / 500`).
const DIG_POWER_DIVISOR: f32 = 500.0;
/// Минимальный урон за удар — не даём округлить до нуля (epsilon).
const MIN_HIT_POWER: f32 = 1.0e-6;

const fn add_crystals_like_reference(current: i64, amount: i64) -> i64 {
    match current.checked_add(amount) {
        Some(sum) if sum >= 0 => sum,
        _ => i64::MAX,
    }
}

fn send_build_state_error(tx: &dyn PacketSink) {
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
    session_id: Option<crate::game::SessionId>,
}

enum DigPlayerRead {
    Ready(DigPlayerData),
    Blocked,
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
    tx: &dyn PacketSink,
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
            let skill_hooks = SkillHooks {
                skills: &skills.states,
            };
            let dp = skill_hooks.on_dig(0.0);
            let mm = skill_hooks.on_dig_crys(0.0);
            let mine_by_crystal = [
                get_player_skill_effect(&skills.states, SkillType::MineGreen),
                get_player_skill_effect(&skills.states, SkillType::MineBlue),
                get_player_skill_effect(&skills.states, SkillType::MineRed),
                get_player_skill_effect(&skills.states, SkillType::MineViolet),
                get_player_skill_effect(&skills.states, SkillType::MineWhite),
                get_player_skill_effect(&skills.states, SkillType::MineCyan),
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
                session_id: ecs
                    .get::<crate::game::player::PlayerConnection>(entity)
                    .map(|connection| connection.session_id),
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
            Some(DigPlayerRead::Ready(data))
        })
        .flatten();
    let Some(player_read) = player_data else {
        tracing::error!(player_id = %pid, "Player entity missing for dig");
        send_build_state_error(tx);
        return;
    };
    let player_data = match player_read {
        DigPlayerRead::Ready(player_data) => player_data,
        DigPlayerRead::Blocked => return,
        DigPlayerRead::MissingState(component) => {
            tracing::error!(player_id = %pid, component, "Player component missing for dig");
            send_build_state_error(tx);
            return;
        }
    };
    // C# `Move(own, dir)` при distance 0 начисляет Movement exp + @S
    // (Player.cs:441-452) — было пропущено, Movement не качался от копания.
    match crate::game::skills::add_player_skill_exp(state, pid, ctx, "M", 1.0, true) {
        crate::game::skills::SkillExpMutation::Packet(Some(sk)) => {
            send_u_packet(tx, sk.0, &sk.1);
        }
        crate::game::skills::SkillExpMutation::Packet(None) => {}
        crate::game::skills::SkillExpMutation::MissingState(component) => {
            tracing::error!(player_id = %pid, component, "Player component missing for dig");
            send_build_state_error(tx);
            return;
        }
        crate::game::skills::SkillExpMutation::MissingEntity => {
            tracing::error!(player_id = %pid, "Player entity missing for dig");
            send_build_state_error(tx);
            return;
        }
    }
    let DigPlayerData {
        x: px,
        y: py,
        dir: actual_dir,
        dig_power,
        mine_general,
        mine_by_crystal,
        skin,
        clan_id,
        session_id,
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
    let fx = hb_dig_fx(
        net_u16_nonneg(pid),
        net_u16_nonneg(px),
        net_u16_nonneg(py),
        u16::try_from(actual_dir).unwrap_or(0),
    );
    let exclude_self = if programmatic { None } else { Some(pid) };
    state.queue_hb_at(px, py, &[fx], exclude_self);

    // BOX pickup применяется lifecycle только после persistence admission.
    if cell.0 == cell_type::BOX {
        state.request_box_pickup(crate::game::BoxPickupIntent {
            player_id: pid,
            player_pos: (px, py).into(),
            box_pos: (tgt_x, tgt_y).into(),
            source: crate::game::BoxPickupSource::Dig {
                session_id,
                direction: actual_dir,
                skin,
                clan_id,
                tail,
                exclude_self: !programmatic,
            },
        });
        return;
    }

    if !diggable {
        return;
    }

    // Fix 3: MilitaryBlock (81) special case — fixed 1.0 damage, no multiplier, no crystal/exp/FX2.
    if cell.0 == cell_type::MILITARY_BLOCK {
        let destroyed = state.world.damage_cell(tgt_x, tgt_y, 1.0);
        if destroyed {
            queue_cell_update(state, tgt_x, tgt_y);
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
        state.queue_hb_at(px, py, &[bot], exclude_self);
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

        // Add MineGeneral exp + crystals on every hit.
        // C# `Mine()`: `Skill.AddExp` sends @S before `crys.AddCrys` sends @B.
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
                let mut packets: Vec<(&str, Vec<u8>)> = Vec::new();
                {
                    let mut skills = ecs
                        .get_mut::<crate::game::player::PlayerSkillsComp>(entity)
                        .expect("PlayerSkillsComp checked before crystal mining");
                    if let Some(sk) =
                        ctx.add_skill_exp(&mut skills.states, "m", mined_yield.exp_amount)
                    {
                        packets.push((sk.0, sk.1));
                    }
                }
                {
                    let mut p_stats = ecs
                        .get_mut::<crate::game::player::PlayerStats>(entity)
                        .expect("PlayerStats checked before crystal mining");
                    p_stats.crystals[idx] = add_crystals_like_reference(
                        p_stats.crystals[idx],
                        mined_yield.final_amount,
                    );
                    let c_data = p_stats.crystals;
                    packets.push(("@B", basket(&c_data, 1).1));
                }
                {
                    let mut flags = ecs
                        .get_mut::<crate::game::player::PlayerFlags>(entity)
                        .expect("PlayerFlags checked before crystal mining");
                    flags.dirty = true;
                }
                Some(packets)
            })
            .flatten();
        if let Some(packets) = mined {
            for (event, payload) in packets {
                send_u_packet(tx, event, &payload);
            }
        } else {
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
        let mine_fx = hb_crystal_mine_fx(
            net_u16_nonneg(pid),
            net_u16_nonneg(tgt_x),
            net_u16_nonneg(tgt_y),
            mined_yield.final_amount,
            color_remapped,
        );
        // C# `SendDFToBots` шлёт через `vChunksAroundEx()` — 5×5 чанков ВКЛЮЧАЯ
        // свой (Entity.cs:43, центр входит), т.е. сам копающий тоже получает FX
        // добычи кристаллов. Раньше exclude=Some(pid) → игрок не видел анимацию
        // «сколько выкопал». Включаем себя (None).
        state.queue_hb_at(px, py, &[mine_fx], None);

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
            queue_cell_update(state, tgt_x, tgt_y);
            state.world.write_world_cell(
                bx,
                by,
                crate::world::WorldCell {
                    cell_type: cell,
                    durability: dur,
                },
            );
            queue_cell_update(state, bx, by);
            true
        } else {
            false
        }
    } else {
        false
    };

    // Fix 10: Boulder push exp.
    if pushed_boulder {
        let packets = state
            .modify_player(pid, |ecs, entity| {
                let mut skills = ecs.get_mut::<crate::game::player::PlayerSkillsComp>(entity)?;
                let mut result = Vec::new();
                if let Some(sk) = ctx.add_skill_exp(&mut skills.states, "d", 1.0) {
                    result.push((sk.0, sk.1));
                    ecs.get_mut::<crate::game::player::PlayerFlags>(entity)?
                        .dirty = true;
                }
                Some(result)
            })
            .flatten()
            .unwrap_or_default();
        for (event, payload) in packets {
            send_u_packet(tx, event, &payload);
        }
    }

    if destroyed {
        // Dig exp only on destroy (1:1 with C# OnDestroy → AddExp("d")).
        let packets = state
            .modify_player(pid, |ecs, entity| {
                let mut result = Vec::new();
                {
                    // C# `Skill.AddExp` всегда шлёт @S при изменении pct — было пропущено
                    // на dig-destroy (полоса Digging не обновлялась до след. @S-события).
                    let mut skills =
                        ecs.get_mut::<crate::game::player::PlayerSkillsComp>(entity)?;
                    if let Some(sk) = ctx.add_skill_exp(&mut skills.states, "d", 1.0) {
                        result.push((sk.0, sk.1));
                    }
                }
                {
                    let mut flags = ecs.get_mut::<crate::game::player::PlayerFlags>(entity)?;
                    flags.dirty = true;
                }
                Some(result)
            })
            .flatten()
            .unwrap_or_default();
        for (event, payload) in packets {
            send_u_packet(tx, event, &payload);
        }

        queue_cell_update(state, tgt_x, tgt_y);
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
    state.queue_hb_at(px, py, &[bot], exclude_self);
}

pub fn handle_build(
    state: &Arc<GameState>,
    tx: &dyn PacketSink,
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
            let skill_hooks = SkillHooks {
                skills: &skills.states,
            };
            let effect = skill_hooks.on_bld(skill_type, 1.0);
            let hp_effect = hp_skill_type.map_or(1.0, |hst| skill_hooks.on_bld_hp(hst, 1.0));
            let yellow_effect = skill_hooks.on_bld(SkillType::BuildYellow, 1.0);
            let yellow_hp = skill_hooks.on_bld_hp(SkillType::BuildYellow, 1.0);
            let red_effect = skill_hooks.on_bld(SkillType::BuildRed, 1.0);
            let red_hp = skill_hooks.on_bld_hp(SkillType::BuildRed, 1.0);

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
            let mut ecs = state.ecs_write_profiled("build.pending_military_conversion");
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
        let packets = state
            .modify_player(pid, |ecs, entity| {
                // C# `Skill.AddExp` всегда шлёт @S при изменении pct — было пропущено
                // на build (полоса Build* не обновлялась до след. @S-события).
                let mut skills = ecs.get_mut::<crate::game::player::PlayerSkillsComp>(entity)?;
                let mut result = Vec::new();
                if let Some(sk) = ctx.add_skill_exp(&mut skills.states, skill.code(), 1.0) {
                    result.push((sk.0.to_string(), sk.1));
                }
                Some(result)
            })
            .flatten()
            .unwrap_or_default();
        for (event, payload) in packets {
            send_u_packet(tx, &event, &payload);
        }
    }
}

pub fn try_spend_crystal(
    state: &Arc<GameState>,
    tx: &dyn PacketSink,
    pid: PlayerId,
    idx: usize,
    amount: i64,
) -> bool {
    match crate::game::logic::crystals::spend_crystal(state, pid, idx, amount) {
        crate::game::logic::crystals::CrystalSpendResult::Spent { crystals } => {
            send_u_packet(tx, "@B", &basket(&crystals, 1).1);
            true
        }
        crate::game::logic::crystals::CrystalSpendResult::Insufficient => false,
        crate::game::logic::crystals::CrystalSpendResult::MissingState(component) => {
            tracing::error!(player_id = %pid, component, "Player component missing for crystal spend");
            send_build_state_error(tx);
            false
        }
        crate::game::logic::crystals::CrystalSpendResult::MissingEntity => {
            tracing::error!(player_id = %pid, "Player entity missing for crystal spend");
            send_build_state_error(tx);
            false
        }
    }
}

fn queue_cell_update(state: &Arc<GameState>, x: i32, y: i32) {
    state.queue_cell_update(x, y);
}

fn place_block(state: &Arc<GameState>, x: i32, y: i32, cell: u8) {
    state
        .world
        .set_cell_typed(x, y, crate::world::CellType(cell));
    queue_cell_update(state, x, y);
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
    queue_cell_update(state, x, y);
}

pub const fn is_truly_empty(cell: crate::world::CellType) -> bool {
    cell.is(cell_type::NOTHING) || cell.is(cell_type::EMPTY)
}

#[cfg(test)]
pub mod tests;
