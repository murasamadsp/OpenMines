use crate::game::player::{
    PlayerConnection, PlayerMetadata, PlayerPosition, PlayerSkillsComp, PlayerStats,
};
use crate::game::skills::{OnMove, PlayerSkills};
use crate::game::{
    ProgrammatorAction, ProgrammatorConfigResource, ProgrammatorQueue, WorldResource,
};
use crate::world::WorldProvider;
use bevy_ecs::prelude::{Entity, Query, Res, ResMut};
use num_traits::ToPrimitive;
use std::time::{Duration, Instant};

use super::helpers::{
    ExecResult, WritableStateContext, check_cell, delay_millis, execute_writable_state,
    selected_offset, selected_world_pos,
};
use super::types::{ActionType, PAction, ProgrammatorState};

const fn direct_action_delay(timing: crate::config::ProgrammatorConfig) -> Duration {
    Duration::from_micros(timing.direct_action_delay_us)
}

// ─── Main ECS system ─────────────────────────────────────────────────────────

type ProgrammatorQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static PlayerMetadata,
        &'static PlayerPosition,
        Option<&'static PlayerConnection>,
        &'static PlayerStats,
        &'static PlayerSkillsComp,
        &'static crate::game::player::PlayerSettings,
        &'static mut crate::game::player::PlayerFlags,
        &'static mut ProgrammatorState,
        &'static crate::game::player::PlayerGeoStack,
    ),
>;

#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]
pub fn programmator_system(
    world_res: Res<WorldResource>,
    timing_res: Res<ProgrammatorConfigResource>,
    mut prog_q: ResMut<ProgrammatorQueue>,
    mut dirty_players: ResMut<crate::game::DirtyPlayers>,
    due_queue: Res<crate::game::ProgrammatorDueQueue>,
    mut due_batch: ResMut<crate::game::ProgrammatorDueBatch>,
    mut query: ProgrammatorQuery<'_, '_>,
) {
    let now = Instant::now();

    for (entity, due_at) in std::mem::take(&mut due_batch.0) {
        let Ok((_, meta, pos, conn, stats, skills, settings, mut flags, mut prog, geo)) =
            query.get_mut(entity)
        else {
            continue;
        };
        if stats.health <= 0 || !prog.running || prog.delay != due_at {
            continue;
        }
        if now < prog.delay {
            continue;
        }

        // Get current function actions count
        let (action_count, current_pos) = {
            let f = prog.current_prog.get(&prog.current_function);
            if let Some(f) = f {
                (f.actions.len(), f.current)
            } else {
                prog.running = false;
                continue;
            }
        };

        // If function exhausted, reset and move to next
        if action_count == 0 || current_pos >= action_count {
            let cf = prog.current_function.clone();
            if let Some(f) = prog.current_prog.get_mut(&cf) {
                f.reset();
            }
            prog.next_function();
            schedule_next_programmator_step(&mut prog, entity, now, None, &due_queue);
            flags.dirty = true;
            dirty_players.0.insert((entity, flags.incarnation));
            continue;
        }

        // Get next action
        let action = {
            let cf = prog.current_function.clone();
            let Some(f) = prog.current_prog.get_mut(&cf) else {
                prog.running = false;
                continue;
            };
            let a = f.actions[f.current].clone();
            f.current += 1;
            a
        };

        tracing::trace!(
            "PROGDIAG exec {:?}:{} at ({},{})",
            action.action_type,
            action.num,
            pos.x,
            pos.y
        );

        let mut delay = None;

        // Execute action and get result
        let result = execute_action(
            &action,
            &mut prog,
            pos,
            stats,
            skills,
            settings,
            &world_res.0,
            meta,
            conn,
            &mut prog_q,
            &mut delay,
            geo.0.len(),
            timing_res.0,
        );

        // Process result (matching C# `ProgrammatorData.Step()`)
        match result {
            ExecResult::Label(label) => {
                handle_label_result(&action, &label, &mut prog);
            }
            ExecResult::BoolResult(result_state) => {
                handle_bool_result(&action, result_state, &mut prog);
            }
            ExecResult::None => {
                handle_none_result(&action, &mut prog);
            }
        }

        schedule_next_programmator_step(&mut prog, entity, now, delay, &due_queue);
        flags.dirty = true;
        dirty_players.0.insert((entity, flags.incarnation));
    }
}

fn schedule_next_programmator_step(
    prog: &mut ProgrammatorState,
    entity: Entity,
    now: Instant,
    delay: Option<Duration>,
    due_queue: &crate::game::ProgrammatorDueQueue,
) {
    if !prog.running {
        return;
    }
    prog.delay = next_programmator_deadline(now, delay);
    due_queue.schedule(entity, prog.delay);
}

pub(crate) fn next_programmator_deadline(now: Instant, delay: Option<Duration>) -> Instant {
    delay
        .and_then(|delay| now.checked_add(delay))
        .unwrap_or(now)
}

#[allow(clippy::too_many_arguments)]
// 1:1 ref C# program executor (Program.cs ActionType switch). Это
// дословный порт большого switch — механический разрез на под-функции и
// substring-переименование `state`/`stats` в 1:1-логике рискуют сломать
// паритет с референсом (жёсткое требование CLAUDE.md). Точечный allow в
// той же конвенции, что db/mod.rs / skills.rs.
#[allow(clippy::too_many_lines, clippy::similar_names)]
fn execute_action(
    action: &PAction,
    prog: &mut ProgrammatorState,
    pos: &PlayerPosition,
    stats: &PlayerStats,
    skills: &PlayerSkillsComp,
    settings: &crate::game::player::PlayerSettings,
    world: &crate::world::World,
    meta: &PlayerMetadata,
    conn: Option<&PlayerConnection>,
    prog_q: &mut ProgrammatorQueue,
    delay: &mut Option<Duration>,
    geo_count: usize,
    timing: crate::config::ProgrammatorConfig,
) -> ExecResult {
    // C# Player.OnRoad: is_road клетки под игроком (для ServerPause road-бонуса).
    let on_road = world.get_cell_typed(pos.x, pos.y).is_road();
    match action.action_type {
        // ─── Movement ────────────────────────────────────────────────────
        // dir = -1 (позиционный ход, 1:1 C# `Move(x,y)` дефолт). `handle_move`
        // выводит поворот из дельты И достигает ветки автокопы (`movement.rs:129`:
        // `dir == -1 && auto_dig`). С явным dir 0-3 автокопа в программе НЕ работала.
        // Повороты (Rotate*) ниже остаются с явным dir — у них нулевая дельта.
        ActionType::MoveDown => {
            *delay = Some(delay_millis(speed_pause(skills, on_road, timing)));
            push_move(prog_q, meta, conn, pos.x, pos.y + 1, -1);
            ExecResult::None
        }
        ActionType::MoveUp => {
            *delay = Some(delay_millis(speed_pause(skills, on_road, timing)));
            push_move(prog_q, meta, conn, pos.x, pos.y - 1, -1);
            ExecResult::None
        }
        ActionType::MoveRight => {
            *delay = Some(delay_millis(speed_pause(skills, on_road, timing)));
            push_move(prog_q, meta, conn, pos.x + 1, pos.y, -1);
            ExecResult::None
        }
        ActionType::MoveLeft => {
            *delay = Some(delay_millis(speed_pause(skills, on_road, timing)));
            push_move(prog_q, meta, conn, pos.x - 1, pos.y, -1);
            ExecResult::None
        }
        ActionType::MoveForward => {
            let (dx, dy) = crate::game::direction::dir_offset(pos.dir);
            *delay = Some(delay_millis(speed_pause(skills, on_road, timing)));
            push_move(prog_q, meta, conn, pos.x + dx, pos.y + dy, -1);
            ExecResult::None
        }

        // ─── Rotation ────────────────────────────────────────────────────
        ActionType::RotateDown => {
            *delay = Some(delay_millis(speed_pause(skills, on_road, timing)));
            push_move(prog_q, meta, conn, pos.x, pos.y, 0);
            ExecResult::None
        }
        ActionType::RotateUp => {
            *delay = Some(delay_millis(speed_pause(skills, on_road, timing)));
            push_move(prog_q, meta, conn, pos.x, pos.y, 2);
            ExecResult::None
        }
        ActionType::RotateLeft => {
            *delay = Some(delay_millis(speed_pause(skills, on_road, timing)));
            push_move(prog_q, meta, conn, pos.x, pos.y, 1);
            ExecResult::None
        }
        ActionType::RotateRight => {
            *delay = Some(delay_millis(speed_pause(skills, on_road, timing)));
            push_move(prog_q, meta, conn, pos.x, pos.y, 3);
            ExecResult::None
        }
        ActionType::RotateLeftRelative => {
            *delay = Some(delay_millis(speed_pause(skills, on_road, timing)));
            let d = match pos.dir {
                0 => 3,
                2 => 1,
                3 => 2,
                // dir 1 → 0 (как и любое прочее).
                _ => 0,
            };
            push_move(prog_q, meta, conn, pos.x, pos.y, d);
            ExecResult::None
        }
        ActionType::RotateRightRelative => {
            *delay = Some(delay_millis(speed_pause(skills, on_road, timing)));
            let d = match pos.dir {
                0 => 1,
                1 => 2,
                2 => 3,
                // dir 3 → 0 (как и любое прочее).
                _ => 0,
            };
            push_move(prog_q, meta, conn, pos.x, pos.y, d);
            ExecResult::None
        }
        ActionType::RotateRandom => {
            *delay = Some(delay_millis(speed_pause(skills, on_road, timing)));
            let d = rand::random_range(0..4);
            push_move(prog_q, meta, conn, pos.x, pos.y, d);
            ExecResult::None
        }

        // ─── Dig / Build ─────────────────────────────────────────────────
        ActionType::Dig => {
            *delay = Some(direct_action_delay(timing));
            prog_q.0.push(ProgrammatorAction::Dig {
                pid: meta.id,
                session_id: conn.map(|c| c.session_id),
                dir: pos.dir,
            });
            ExecResult::None
        }
        // MacrosBuild (id 142) намеренно НЕ здесь: C# `PAction.Execute` не имеет
        // для него case → no-op (падает в `_ => None`). 1:1 с референсом.
        ActionType::BuildBlock => {
            *delay = Some(direct_action_delay(timing));
            prog_q.0.push(ProgrammatorAction::Build {
                pid: meta.id,
                session_id: conn.map(|c| c.session_id),
                dir: pos.dir,
                block_type: "G".to_string(),
            });
            ExecResult::None
        }
        ActionType::BuildPillar => {
            *delay = Some(direct_action_delay(timing));
            prog_q.0.push(ProgrammatorAction::Build {
                pid: meta.id,
                session_id: conn.map(|c| c.session_id),
                dir: pos.dir,
                block_type: "O".to_string(),
            });
            ExecResult::None
        }
        ActionType::BuildRoad => {
            *delay = Some(direct_action_delay(timing));
            prog_q.0.push(ProgrammatorAction::Build {
                pid: meta.id,
                session_id: conn.map(|c| c.session_id),
                dir: pos.dir,
                block_type: "R".to_string(),
            });
            ExecResult::None
        }
        ActionType::BuildMilitaryBlock => {
            *delay = Some(direct_action_delay(timing));
            prog_q.0.push(ProgrammatorAction::Build {
                pid: meta.id,
                session_id: conn.map(|c| c.session_id),
                dir: pos.dir,
                block_type: "V".to_string(),
            });
            ExecResult::None
        }
        ActionType::Geology => {
            *delay = Some(direct_action_delay(timing));
            prog_q.0.push(ProgrammatorAction::Geo {
                pid: meta.id,
                session_id: conn.map(|c| c.session_id),
            });
            ExecResult::None
        }
        ActionType::Heal => {
            prog_q.0.push(ProgrammatorAction::Heal {
                pid: meta.id,
                session_id: conn.map(|c| c.session_id),
            });
            *delay = Some(direct_action_delay(timing));
            ExecResult::None
        }
        ActionType::Stop => {
            prog.stop_program();
            prog_q.0.push(ProgrammatorAction::SetProgrammatorStatus {
                session_id: conn.map(|c| c.session_id),
                running: false,
            });
            prog_q.0.push(ProgrammatorAction::SetHandMode {
                session_id: conn.map(|c| c.session_id),
                enabled: false,
            });
            ExecResult::None
        }

        // ─── Shift / Check direction ────────────────────────────────────
        ActionType::ShiftUp => {
            prog.shift_y -= 1;
            ExecResult::None
        }
        ActionType::ShiftDown => {
            prog.shift_y += 1;
            ExecResult::None
        }
        ActionType::ShiftRight => {
            prog.shift_x += 1;
            ExecResult::None
        }
        ActionType::ShiftLeft => {
            prog.shift_x -= 1;
            ExecResult::None
        }
        ActionType::ShiftForward => {
            prog.shift_x += match pos.dir {
                1 => -1,
                3 => 1,
                _ => 0,
            };
            prog.shift_y += match pos.dir {
                0 => -1,
                2 => 1,
                _ => 0,
            };
            ExecResult::None
        }
        ActionType::CheckUp => {
            prog.check_x = 0;
            prog.check_y = -1;
            ExecResult::None
        }
        ActionType::CheckDown => {
            prog.check_x = 0;
            prog.check_y = 1;
            ExecResult::None
        }
        ActionType::CheckRight => {
            prog.check_x = 1;
            prog.check_y = 0;
            ExecResult::None
        }
        ActionType::CheckLeft => {
            prog.check_x = -1;
            prog.check_y = 0;
            ExecResult::None
        }
        ActionType::CheckUpLeft => {
            prog.check_x = -1;
            prog.check_y = -1;
            ExecResult::None
        }
        ActionType::CheckUpRight => {
            prog.check_x = 1;
            prog.check_y = -1;
            ExecResult::None
        }
        ActionType::CheckDownLeft => {
            prog.check_x = -1;
            prog.check_y = 1;
            ExecResult::None
        }
        ActionType::CheckDownRight => {
            prog.check_x = 1;
            prog.check_y = 1;
            ExecResult::None
        }
        ActionType::CheckForward => {
            prog.check_x = match pos.dir {
                1 => -1,
                3 => 1,
                _ => 0,
            };
            prog.check_y = match pos.dir {
                0 => 1,
                2 => -1,
                _ => 0,
            };
            ExecResult::None
        }
        ActionType::CheckLeftRelative => {
            prog.check_x = match pos.dir {
                0 => -1,
                2 => 1,
                _ => 0,
            };
            prog.check_y = match pos.dir {
                1 => 1,
                3 => -1,
                _ => 0,
            };
            ExecResult::None
        }
        ActionType::CheckRightRelative => {
            prog.check_x = match pos.dir {
                0 => 1,
                2 => -1,
                _ => 0,
            };
            prog.check_y = match pos.dir {
                1 => -1,
                3 => 1,
                _ => 0,
            };
            ExecResult::None
        }

        // ─── Cell condition checks ──────────────────────────────────────
        ActionType::IsEmpty => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                w.cell_defs()
                    .get_typed(w.get_cell_typed(x, y))
                    .cell_is_empty()
            });
            ExecResult::None
        }
        ActionType::IsNotEmpty => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                !w.cell_defs()
                    .get_typed(w.get_cell_typed(x, y))
                    .cell_is_empty()
            });
            ExecResult::None
        }
        ActionType::IsCrystal => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                w.get_cell_typed(x, y).is_crystal()
            });
            ExecResult::None
        }
        ActionType::IsBoulder => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                w.cell_defs()
                    .get_typed(w.get_cell_typed(x, y))
                    .nature
                    .is_boulder
            });
            ExecResult::None
        }
        ActionType::IsSand => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                w.cell_defs().get_typed(w.get_cell_typed(x, y)).is_sand()
            });
            ExecResult::None
        }
        ActionType::IsBreakableRock => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                w.cell_defs()
                    .get_typed(w.get_cell_typed(x, y))
                    .is_diggable()
            });
            ExecResult::None
        }
        ActionType::IsUnbreakable => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                let defs = w.cell_defs();
                let def = defs.get_typed(w.get_cell_typed(x, y));
                !def.cell_is_empty() && !def.is_diggable()
            });
            ExecResult::None
        }
        ActionType::IsFalling => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                let defs = w.cell_defs();
                let def = defs.get_typed(w.get_cell_typed(x, y));
                def.is_sand() || def.nature.is_boulder
            });
            ExecResult::None
        }
        ActionType::IsRoad => {
            check_cell(&mut *prog, pos, world, |x, y, w| w.get_road_cell(x, y) != 0);
            ExecResult::None
        }
        ActionType::IsBox => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                w.get_cell_typed(x, y)
                    .is(crate::world::cells::cell_type::BOX)
            });
            ExecResult::None
        }
        ActionType::IsGreenBlock => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                w.get_cell_typed(x, y)
                    .is(crate::world::cells::cell_type::GREEN_BLOCK)
            });
            ExecResult::None
        }
        ActionType::IsYellowBlock => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                w.get_cell_typed(x, y)
                    .is(crate::world::cells::cell_type::YELLOW_BLOCK)
            });
            ExecResult::None
        }
        ActionType::IsRedBlock => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                w.get_cell_typed(x, y)
                    .is(crate::world::cells::cell_type::RED_BLOCK)
            });
            ExecResult::None
        }
        ActionType::IsPillar => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                w.get_cell_typed(x, y)
                    .is(crate::world::cells::cell_type::SUPPORT)
            });
            ExecResult::None
        }
        ActionType::IsQuadBlock => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                w.get_cell_typed(x, y)
                    .is(crate::world::cells::cell_type::QUAD_BLOCK)
            });
            ExecResult::None
        }
        ActionType::IsRedRock => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                w.get_cell_typed(x, y)
                    .is(crate::world::cells::cell_type::RED_ROCK)
            });
            ExecResult::None
        }
        ActionType::IsBlackRock => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                w.get_cell_typed(x, y)
                    .is(crate::world::cells::cell_type::BLACK_ROCK)
            });
            ExecResult::None
        }
        ActionType::IsAcid => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                w.get_cell_typed(x, y).is_acid()
            });
            ExecResult::None
        }
        ActionType::IsSlime => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                w.get_cell_typed(x, y).is_slime()
            });
            ExecResult::None
        }
        ActionType::IsInGun => {
            check_cell(&mut *prog, pos, world, |_x, _y, _w| {
                false // stub: будет подключено через world/game state
            });
            ExecResult::None
        }
        ActionType::IsLivingCrystal => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                w.get_cell_typed(x, y).is_living_crystal()
            });
            ExecResult::None
        }
        ActionType::IsHpLower100 => {
            let result = stats.health < stats.max_health;
            if let Some(f) = prog.current_prog.get_mut(&prog.current_function) {
                match f.last_state_action {
                    Some(ActionType::Or) => f.state = Some(f.state.unwrap_or(false) || result),
                    Some(ActionType::And) => f.state = Some(f.state.unwrap_or(true) && result),
                    // None и прочее → прямое значение result.
                    _ => f.state = Some(result),
                }
            }
            ExecResult::None
        }
        ActionType::IsHpLower50 => {
            let result = stats.health < stats.max_health / 2;
            if let Some(f) = prog.current_prog.get_mut(&prog.current_function) {
                match f.last_state_action {
                    Some(ActionType::Or) => f.state = Some(f.state.unwrap_or(false) || result),
                    Some(ActionType::And) => f.state = Some(f.state.unwrap_or(true) && result),
                    // None и прочее → прямое значение result.
                    _ => f.state = Some(result),
                }
            }
            ExecResult::None
        }

        // ─── Flow control ───────────────────────────────────────────────
        ActionType::GoTo
        | ActionType::RunSub
        | ActionType::RunFunction
        | ActionType::RunState
        | ActionType::RunOnRespawn => ExecResult::Label(action.label.clone()),
        ActionType::RunIfTrue => {
            let state_val = prog
                .current_prog
                .get(&prog.current_function)
                .and_then(|f| f.state);
            if state_val == Some(false) {
                // C#/JS preserve state on the no-jump path.
                ExecResult::None
            } else {
                if let Some(f) = prog.current_prog.get_mut(&prog.current_function) {
                    f.state = None;
                }
                ExecResult::Label(action.label.clone())
            }
        }
        ActionType::RunIfFalse => {
            let state_val = prog
                .current_prog
                .get(&prog.current_function)
                .and_then(|f| f.state);
            if state_val == Some(true) {
                // C#/JS preserve state on the no-jump path.
                ExecResult::None
            } else {
                if let Some(f) = prog.current_prog.get_mut(&prog.current_function) {
                    f.state = None;
                }
                ExecResult::Label(action.label.clone())
            }
        }
        ActionType::ReturnFunction => {
            let state_val = prog
                .current_prog
                .get(&prog.current_function)
                .and_then(|f| f.state);
            ExecResult::BoolResult(state_val.unwrap_or(false))
        }

        // ─── Logic operators ────────────────────────────────────────────
        ActionType::Or => {
            if let Some(f) = prog.current_prog.get_mut(&prog.current_function) {
                f.last_state_action = Some(ActionType::Or);
            }
            ExecResult::None
        }
        ActionType::And => {
            if let Some(f) = prog.current_prog.get_mut(&prog.current_function) {
                f.last_state_action = Some(ActionType::And);
            }
            ExecResult::None
        }

        // Control (Start/Stop/Return/ReturnState/Flip) → ExecResult::None
        // через общий wildcard ниже.
        ActionType::Beep => {
            let (event, payload) = crate::protocol::packets::bibika();
            let pkt = crate::protocol::u_packet(event, &payload);
            let mut buf = bytes::BytesMut::with_capacity(pkt.wire_len());
            if pkt.encode(&mut buf).is_ok()
                && let Some(session_id) = conn.map(|connection| connection.session_id)
            {
                prog_q.0.push(ProgrammatorAction::Send {
                    session_id,
                    data: buf.to_vec(),
                });
            }
            ExecResult::None
        }

        ActionType::EnableAutoDig => {
            prog_q.0.push(ProgrammatorAction::SetAutoDig {
                pid: meta.id,
                session_id: conn.map(|c| c.session_id),
                enabled: true,
            });
            ExecResult::None
        }
        ActionType::DisableAutoDig => {
            prog_q.0.push(ProgrammatorAction::SetAutoDig {
                pid: meta.id,
                session_id: conn.map(|c| c.session_id),
                enabled: false,
            });
            ExecResult::None
        }
        ActionType::EnableAgression => {
            prog_q.0.push(ProgrammatorAction::SetAggression {
                pid: meta.id,
                session_id: conn.map(|c| c.session_id),
                enabled: true,
            });
            ExecResult::None
        }
        ActionType::DisableAgression => {
            prog_q.0.push(ProgrammatorAction::SetAggression {
                pid: meta.id,
                session_id: conn.map(|c| c.session_id),
                enabled: false,
            });
            ExecResult::None
        }
        ActionType::HandModeOn => {
            prog.hand_mode_active = true;
            prog_q.0.push(ProgrammatorAction::SetHandMode {
                session_id: conn.map(|c| c.session_id),
                enabled: true,
            });
            ExecResult::None
        }
        ActionType::HandModeOff => {
            prog.hand_mode_active = false;
            prog_q.0.push(ProgrammatorAction::SetHandMode {
                session_id: conn.map(|c| c.session_id),
                enabled: false,
            });
            ExecResult::None
        }
        ActionType::DebugMessage | ActionType::DebugPause => {
            // JS reference ExecutorList: DBG_MSG/DBG_PAUSE only call dbgL("28"/"29").
            // No wire packet, delay or state mutation is expected.
            tracing::debug!(
                player_id = %meta.id,
                action = ?action.action_type,
                label = %action.label,
                "programmator debug action"
            );
            ExecResult::None
        }

        // ─── Macros (simplified) ────────────────────────────────────────
        ActionType::MacrosDig => {
            let (dx, dy) = crate::game::direction::dir_offset(pos.dir);
            let tx = pos.x + dx;
            let ty = pos.y + dy;
            {
                let diggable = world
                    .cell_defs()
                    .get_typed(world.get_cell_typed(tx, ty))
                    .is_diggable();
                if diggable {
                    *delay = Some(direct_action_delay(timing));
                    prog_q.0.push(ProgrammatorAction::Dig {
                        pid: meta.id,
                        session_id: conn.map(|c| c.session_id),
                        dir: pos.dir,
                    });
                    return ExecResult::BoolResult(true);
                }
            }
            ExecResult::None
        }
        ActionType::MacrosHeal => {
            // C# PAction.cs:122-131: требует Red-кристалл (`crys[Red] > 0`) перед Heal.
            // Red — индекс 2 (Green0 Blue1 Red2 Violet3 White4 Cyan5).
            if stats.crystals[2] > 0 && stats.health < stats.max_health {
                prog_q.0.push(ProgrammatorAction::Heal {
                    pid: meta.id,
                    session_id: conn.map(|c| c.session_id),
                });
                *delay = Some(direct_action_delay(timing));
                return ExecResult::BoolResult(true);
            }
            ExecResult::None
        }
        ActionType::MacrosMine => {
            // dirz: dir→offset (C# {0:(0,1),1:(-1,0),2:(0,-1),3:(1,0)}).
            const DIRZ: [(i32, (i32, i32)); 4] =
                [(0, (0, 1)), (1, (-1, 0)), (2, (0, -1)), (3, (1, 0))];
            // C# PAction.cs:90-121. Fast-path: если уже копаем в направлении (template)
            // и там всё ещё кристалл — копаем дальше.
            if prog.macros_template.is_some() {
                let (dx, dy) = crate::game::direction::dir_offset(pos.dir);
                if world.get_cell_typed(pos.x + dx, pos.y + dy).is_crystal() {
                    *delay = Some(direct_action_delay(timing));
                    prog_q.0.push(ProgrammatorAction::Dig {
                        pid: meta.id,
                        session_id: conn.map(|c| c.session_id),
                        dir: pos.dir,
                    });
                    return ExecResult::BoolResult(true);
                }
            }
            // Скан 4 направлений. Первый кристалл: если смотрим на него — копаем
            // (и фиксируем template), иначе поворачиваемся к нему.
            for (dir_key, (dx, dy)) in DIRZ {
                if world.get_cell_typed(pos.x + dx, pos.y + dy).is_crystal() {
                    if pos.dir == dir_key {
                        *delay = Some(direct_action_delay(timing));
                        prog.macros_template = Some(dir_key);
                        prog_q.0.push(ProgrammatorAction::Dig {
                            pid: meta.id,
                            session_id: conn.map(|c| c.session_id),
                            dir: pos.dir,
                        });
                    } else {
                        *delay = Some(delay_millis(speed_pause(skills, on_road, timing)));
                        push_move(prog_q, meta, conn, pos.x, pos.y, dir_key);
                    }
                    return ExecResult::BoolResult(true);
                }
            }
            prog.macros_template = None;
            ExecResult::None
        }
        ActionType::MacrosGun => {
            // JS ref MACROS_GUN: charge the gun at facing cell.
            let (dx, dy) = crate::game::direction::dir_offset(pos.dir);
            let gx = pos.x + dx;
            let gy = pos.y + dy;
            *delay = Some(direct_action_delay(timing));
            prog_q.0.push(ProgrammatorAction::FillGun {
                pid: meta.id,
                session_id: conn.map(|c| c.session_id),
                x: gx,
                y: gy,
            });
            ExecResult::BoolResult(true)
        }
        ActionType::MacrosDigAround => {
            // JS ref MACROS_DIGG_AROUND: save rotation, scan left/right/ahead
            // for crystals, dig if found. Rotations: left=(d+3)%4, right=(d+1)%4.
            let left_dir = (pos.dir + 3) % 4;
            let right_dir = (pos.dir + 1) % 4;
            let check = [(left_dir, 0), (right_dir, 1), (pos.dir, 2)];
            let mut found = false;
            for &(check_dir, _) in &check {
                let (dx, dy) = crate::game::direction::dir_offset(check_dir);
                let cx = pos.x + dx;
                let cy = pos.y + dy;
                if world.get_cell_typed(cx, cy).is_crystal() {
                    if pos.dir == check_dir {
                        *delay = Some(direct_action_delay(timing));
                        prog_q.0.push(ProgrammatorAction::Dig {
                            pid: meta.id,
                            session_id: conn.map(|c| c.session_id),
                            dir: pos.dir,
                        });
                    } else {
                        *delay = Some(delay_millis(speed_pause(skills, on_road, timing)));
                        push_move(prog_q, meta, conn, pos.x, pos.y, check_dir);
                    }
                    found = true;
                    break;
                }
            }
            if found {
                ExecResult::BoolResult(true)
            } else {
                ExecResult::None
            }
        }

        // ─── Writable state / other ─────────────────────────────────────
        ActionType::WritableState
        | ActionType::WritableStateLower
        | ActionType::WritableStateMore => execute_writable_state(
            action,
            prog,
            WritableStateContext {
                pos,
                stats,
                settings,
                world,
                geo_count,
            },
            delay,
        ),

        _ => ExecResult::None,
    }
}

/// Ветка `GoTo` из `handle_label_result` (вынесена — лимит строк).
fn handle_goto_label(label: &str, prog: &mut ProgrammatorState) {
    if prog.current_prog.contains_key(label) {
        if let Some(f) = prog.current_prog.get_mut(&prog.current_function) {
            f.reset();
        }
        if label.is_empty() {
            let sp_name = prog.startpoint.0.clone();
            let sp_pos = prog.startpoint.1;
            prog.current_function = sp_name;
            if let Some(f) = prog.current_prog.get_mut(&prog.current_function) {
                f.current = sp_pos;
            }
        } else {
            prog.current_function = label.to_string();
        }
    } else {
        let sp_name = prog.startpoint.0.clone();
        let sp_pos = prog.startpoint.1;
        prog.current_function = sp_name;
        if let Some(f) = prog.current_prog.get_mut(&prog.current_function) {
            f.current = sp_pos;
        }
    }
}

fn handle_label_result(action: &PAction, label: &str, prog: &mut ProgrammatorState) {
    match action.action_type {
        ActionType::GoTo => handle_goto_label(label, prog),
        ActionType::RunSub => {
            if prog.current_prog.contains_key(label) {
                let cf = prog.current_function.clone();
                if let Some(f) = prog.current_prog.get_mut(label) {
                    f.called_from = Some(cf);
                }
                prog.current_function = label.to_string();
            }
        }
        ActionType::RunFunction => {
            if prog.current_prog.contains_key(label) {
                let cf = prog.current_function.clone();
                let has_offset = prog.shift_x != 0
                    || prog.shift_y != 0
                    || prog.check_x != 0
                    || prog.check_y != 0;
                if has_offset {
                    let offset = (prog.shift_x + prog.check_x, prog.shift_y + prog.check_y);
                    if let Some(f) = prog.current_prog.get_mut(label) {
                        f.startoffset = offset;
                    }
                }
                if let Some(f) = prog.current_prog.get_mut(label) {
                    f.called_from = Some(cf);
                }
                prog.current_function = label.to_string();
            }
        }
        ActionType::RunState => {
            if prog.current_prog.contains_key(label) {
                let cf = prog.current_function.clone();
                let (state_val, last_state) = prog
                    .current_prog
                    .get(&cf)
                    .map_or((None, None), |f| (f.state, f.last_state_action));
                let has_offset = prog.shift_x != 0
                    || prog.shift_y != 0
                    || prog.check_x != 0
                    || prog.check_y != 0;
                if has_offset {
                    let offset = (prog.shift_x + prog.check_x, prog.shift_y + prog.check_y);
                    if let Some(f) = prog.current_prog.get_mut(label) {
                        f.startoffset = offset;
                    }
                }
                if let Some(f) = prog.current_prog.get_mut(label) {
                    f.state = state_val;
                    f.last_state_action = last_state;
                    f.called_from = Some(cf);
                }
                prog.current_function = label.to_string();
            }
        }
        ActionType::RunIfTrue | ActionType::RunIfFalse => {
            if prog.current_prog.contains_key(label) {
                if let Some(f) = prog.current_prog.get_mut(&prog.current_function) {
                    f.reset();
                }
                if label.is_empty() {
                    let sp_name = prog.startpoint.0.clone();
                    let sp_pos = prog.startpoint.1;
                    prog.current_function = sp_name;
                    if let Some(f) = prog.current_prog.get_mut(&prog.current_function) {
                        f.current = sp_pos;
                    }
                } else {
                    let called_from = prog
                        .current_prog
                        .get(&prog.current_function)
                        .and_then(|f| f.called_from.clone());
                    if let Some(f) = prog.current_prog.get_mut(label) {
                        f.called_from = called_from;
                    }
                    prog.current_function = label.to_string();
                }
            }
        }
        ActionType::RunOnRespawn if prog.current_prog.contains_key(label) => {
            prog.goto_death = Some(label.to_string());
        }
        _ => {}
    }
}

fn handle_bool_result(action: &PAction, state: bool, prog: &mut ProgrammatorState) {
    match action.action_type {
        ActionType::ReturnFunction => {
            let cf = prog.current_function.clone();
            if let Some(f) = prog.current_prog.get_mut(&cf) {
                f.reset();
                f.startoffset = (0, 0);
            }
            let called_from = prog
                .current_prog
                .get(&cf)
                .and_then(|f| f.called_from.clone());
            if let Some(caller) = called_from {
                prog.current_function.clone_from(&caller);
                if let Some(f) = prog.current_prog.get_mut(&caller) {
                    f.state = Some(state);
                    f.startoffset = (0, 0);
                }
            }
        }
        ActionType::MacrosDig
        | ActionType::MacrosHeal
        | ActionType::MacrosMine
        | ActionType::MacrosGun
        | ActionType::MacrosDigAround => {
            // Repeat action: decrement current
            let cf = prog.current_function.clone();
            if state
                && let Some(f) = prog.current_prog.get_mut(&cf)
                && f.current > 0
            {
                f.current -= 1;
            }
        }
        _ => {}
    }
}

fn handle_none_result(action: &PAction, prog: &mut ProgrammatorState) {
    match action.action_type {
        ActionType::CheckDown
        | ActionType::CheckUp
        | ActionType::CheckRight
        | ActionType::CheckLeft
        | ActionType::CheckDownLeft
        | ActionType::CheckDownRight
        | ActionType::CheckUpLeft
        | ActionType::CheckUpRight
        | ActionType::ShiftUp
        | ActionType::ShiftLeft
        | ActionType::ShiftDown
        | ActionType::ShiftRight
        | ActionType::ShiftForward => {
            let cf = prog.current_function.clone();
            if let Some(f) = prog.current_prog.get_mut(&cf)
                && f.startoffset != (0, 0)
            {
                f.startoffset = (0, 0);
            }
        }
        ActionType::Return => {
            let cf = prog.current_function.clone();
            if let Some(f) = prog.current_prog.get_mut(&cf) {
                f.reset();
            }
            let called_from = prog
                .current_prog
                .get(&cf)
                .and_then(|f| f.called_from.clone());
            if let Some(caller) = called_from {
                prog.current_function = caller;
            }
        }
        ActionType::ReturnState => {
            let cf = prog.current_function.clone();
            if let Some(f) = prog.current_prog.get_mut(&cf) {
                f.reset();
            }
            let (state_val, last_state, called_from) =
                prog.current_prog.get(&cf).map_or((None, None, None), |f| {
                    (f.state, f.last_state_action, f.called_from.clone())
                });
            if let Some(caller) = called_from {
                let has_offset = prog.shift_x != 0
                    || prog.shift_y != 0
                    || prog.check_x != 0
                    || prog.check_y != 0;
                if has_offset {
                    let offset = (prog.shift_x + prog.check_x, prog.shift_y + prog.check_y);
                    if let Some(f) = prog.current_prog.get_mut(&caller) {
                        f.startoffset = offset;
                    }
                }
                if let Some(f) = prog.current_prog.get_mut(&caller) {
                    f.state = state_val;
                    f.last_state_action = last_state;
                }
                prog.current_function = caller;
            }
        }
        ActionType::Start => {
            let cf = prog.current_function.clone();
            let pos = prog.current_prog.get(&cf).map_or(0, |f| f.current);
            prog.startpoint = (cf, pos);
        }
        ActionType::Flip => {
            prog.flip_state = !prog.flip_state;
        }
        _ => {}
    }
}

fn push_move(
    prog_q: &mut ProgrammatorQueue,
    meta: &PlayerMetadata,
    conn: Option<&PlayerConnection>,
    x: i32,
    y: i32,
    dir: i32,
) {
    prog_q.0.push(ProgrammatorAction::Move {
        pid: meta.id,
        session_id: conn.map(|c| c.session_id),
        x,
        y,
        dir,
    });
}

fn speed_pause(
    skills: &PlayerSkillsComp,
    on_road: bool,
    timing: crate::config::ProgrammatorConfig,
) -> u64 {
    let move_effect = PlayerSkills {
        skills: &skills.states,
    }
    .on_move(0.0);
    // 1:1 ref Player.cs:155: ServerPause = (OnRoad ? pause*5*0.80 : pause*5) * 1.4 / 1000.
    // pause = move_effect * 100. move_effect — f32 из get_player_skill_effect
    // (1:1 с C#, нельзя в int без потери паритета); каст намеренный,
    // move_effect ≥ 0. Та же конвенция, что skills.rs.
    let pause_units = (move_effect * 100.0).to_u64().unwrap_or(0);
    // off-road: pause*5*1.4 = pause*7; on-road (×0.80): pause*5.6 = pause*56/10000.
    let server_pause_ms = if on_road {
        pause_units * 56 / 10000
    } else {
        pause_units * 7 / 1000
    };
    server_pause_ms.max(timing.min_move_delay_ms)
}
