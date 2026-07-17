use super::types::{ActionType, PAction, ProgrammatorState};
use crate::game::player::{PlayerPosition, PlayerStats};
use crate::world::WorldProvider;
use std::time::Duration;

pub(crate) const fn delay_millis(ms: u64) -> Duration {
    Duration::from_millis(ms)
}

pub(crate) fn check_cell(
    prog: &mut ProgrammatorState,
    pos: &PlayerPosition,
    world: &crate::world::World,
    predicate: impl FnOnce(i32, i32, &crate::world::World) -> bool,
) {
    let (sx, sy) = {
        let f = prog.current_prog.get(&prog.current_function);
        if let Some(f) = f {
            if f.startoffset == (0, 0) {
                (prog.shift_x + prog.check_x, prog.shift_y + prog.check_y)
            } else {
                f.startoffset
            }
        } else {
            (prog.shift_x + prog.check_x, prog.shift_y + prog.check_y)
        }
    };

    let x = if prog.flip_state {
        pos.x - sx
    } else {
        pos.x + sx
    };
    let y = if prog.flip_state {
        pos.y - sy
    } else {
        pos.y + sy
    };

    prog.check_x = 0;
    prog.check_y = 0;
    prog.shift_x = 0;
    prog.shift_y = 0;

    let result = predicate(x, y, world);

    let func = prog.current_prog.get_mut(&prog.current_function);
    if let Some(f) = func {
        match f.last_state_action {
            Some(ActionType::Or) => f.state = Some(f.state.unwrap_or(false) || result),
            Some(ActionType::And) => f.state = Some(f.state.unwrap_or(true) && result),
            _ => f.state = Some(result),
        }
    }
}

pub(crate) fn set_condition_state(prog: &mut ProgrammatorState, result: bool) {
    if let Some(f) = prog.current_prog.get_mut(&prog.current_function) {
        match f.last_state_action {
            Some(ActionType::Or) => f.state = Some(f.state.unwrap_or(false) || result),
            Some(ActionType::And) => f.state = Some(f.state.unwrap_or(true) && result),
            _ => f.state = Some(result),
        }
    }
}

pub(crate) fn selected_offset(prog: &ProgrammatorState) -> (i32, i32) {
    let f = prog.current_prog.get(&prog.current_function);
    if let Some(f) = f
        && f.startoffset != (0, 0)
    {
        return f.startoffset;
    }
    (prog.shift_x + prog.check_x, prog.shift_y + prog.check_y)
}

pub(crate) fn selected_world_pos(prog: &ProgrammatorState, pos: &PlayerPosition) -> (i32, i32) {
    let (sx, sy) = selected_offset(prog);
    if prog.flip_state {
        (pos.x - sx, pos.y - sy)
    } else {
        (pos.x + sx, pos.y + sy)
    }
}

pub(crate) const fn reset_view_offsets(prog: &mut ProgrammatorState) {
    prog.check_x = 0;
    prog.check_y = 0;
    prog.shift_x = 0;
    prog.shift_y = 0;
}

pub(crate) const fn compare_value(action_type: ActionType, actual: i32, expected: i32) -> bool {
    match action_type {
        ActionType::WritableStateLower => actual < expected,
        ActionType::WritableStateMore => actual > expected,
        _ => actual == expected,
    }
}

pub(crate) fn clamp_i64_to_i32(value: i64) -> i32 {
    i32::try_from(value).unwrap_or_else(|_| {
        if value.is_negative() {
            i32::MIN
        } else {
            i32::MAX
        }
    })
}

pub(crate) fn load_percent(stats: &PlayerStats) -> i32 {
    let total = stats.crystals.iter().copied().sum::<i64>();
    clamp_i64_to_i32(total.saturating_mul(100))
}

pub(crate) fn programmator_call_depth(prog: &ProgrammatorState) -> i32 {
    let mut depth = 0_i32;
    let mut current = prog.current_function.as_str();
    let mut guard = 0_usize;
    while guard < prog.current_prog.len() {
        guard += 1;
        let Some(called_from) = prog
            .current_prog
            .get(current)
            .and_then(|f| f.called_from.as_deref())
        else {
            break;
        };
        depth = depth.saturating_add(1);
        current = called_from;
    }
    depth
}

pub(crate) fn programmator_logic_mode(prog: &ProgrammatorState) -> i32 {
    prog.current_prog
        .get(&prog.current_function)
        .and_then(|f| f.last_state_action)
        .map_or(0, |action| match action {
            ActionType::And => 1,
            ActionType::Or => 2,
            _ => 0,
        })
}

pub(crate) fn readonly_programmator_value(
    label: &str,
    prog: &mut ProgrammatorState,
    pos: &PlayerPosition,
    stats: &PlayerStats,
    settings: &crate::game::player::PlayerSettings,
    world: &crate::world::World,
    geo_count: usize,
) -> Option<i32> {
    let key = label.to_ascii_uppercase();
    if let Some((kind, percent)) =
        crate::game::logic::crystals::CrystalKind::from_programmator_variable(&key)
    {
        let value = stats.crystals[kind.index()];
        return Some(clamp_i64_to_i32(if percent {
            value.saturating_mul(100)
        } else {
            value
        }));
    }
    let value = match key.as_str() {
        "AUT" => i32::from(settings.auto_dig),
        "AGR" => i32::from(settings.aggression),
        "HND" => i32::from(prog.hand_mode_active),
        "DBG" => 0,
        "STK" => programmator_call_depth(prog),
        "DIR" => match pos.dir {
            1 => 3,
            3 => 1,
            d => d,
        },
        "X" => pos.x,
        "Y" => pos.y,
        "CEL" => {
            let (x, y) = selected_world_pos(prog, pos);
            let cell = i32::from(world.get_cell(x, y));
            reset_view_offsets(prog);
            cell
        }
        "HP" => stats.health,
        "HPP" => {
            if stats.max_health <= 0 {
                0
            } else {
                stats.health.saturating_mul(100) / stats.max_health
            }
        }
        "TIM" => i32::try_from(prog.started_at.elapsed().as_secs()).unwrap_or(i32::MAX),
        "GEO" => i32::try_from(geo_count).unwrap_or(i32::MAX),
        "GEP" => i32::try_from(geo_count.saturating_mul(100)).unwrap_or(i32::MAX),
        "LOA" => load_percent(stats),
        "RND" => rand::random_range(0..1000),
        "FLP" => i32::from(prog.flip_state),
        "BOO" => programmator_logic_mode(prog),
        "AX" => selected_offset(prog).0.abs(),
        "AY" => selected_offset(prog).1.abs(),
        "DX" => 100 + selected_offset(prog).0,
        "DY" => 100 + selected_offset(prog).1,
        _ => return None,
    };
    Some(value)
}

pub(crate) fn command_variable_value(
    name: &str,
    prog: &mut ProgrammatorState,
    ctx: WritableStateContext<'_>,
) -> Option<i32> {
    prog.user_variables.get(name).copied().or_else(|| {
        readonly_programmator_value(
            name,
            prog,
            ctx.pos,
            ctx.stats,
            ctx.settings,
            ctx.world,
            ctx.geo_count,
        )
    })
}

pub(crate) const fn js_divisor(value: i32) -> i32 {
    if value == 0 { 1 } else { value }
}

pub(crate) fn run_programmator_command(
    prog: &mut ProgrammatorState,
    command: &str,
    num: i32,
    ctx: WritableStateContext<'_>,
) -> bool {
    let key = command.to_ascii_uppercase();
    match key.as_str() {
        "SET" | "ADD" | "MUL" | "DIV" | "SUB" | "MOD" => {
            let Some(name) = prog.last_variables.younger().map(str::to_string) else {
                return false;
            };
            let Some(current) = prog.user_variables.get(&name).copied() else {
                return false;
            };
            let next = match key.as_str() {
                "SET" => num,
                "ADD" => current.saturating_add(num),
                "MUL" => current.saturating_mul(num),
                "DIV" => current / js_divisor(num),
                "SUB" => current.saturating_sub(num),
                "MOD" => {
                    if num == 0 {
                        return false;
                    }
                    current % num
                }
                _ => unreachable!(),
            };
            prog.user_variables.insert(name, next);
            true
        }
        "AD2" | "MU2" | "DI2" | "SU2" => {
            let Some(younger_name) = prog.last_variables.younger().map(str::to_string) else {
                return false;
            };
            let Some(older_name) = prog.last_variables.older().map(str::to_string) else {
                return false;
            };
            let Some(younger) = command_variable_value(&younger_name, prog, ctx) else {
                return false;
            };
            let Some(older) = prog.user_variables.get(&older_name).copied() else {
                return false;
            };
            let next = match key.as_str() {
                "AD2" => older.saturating_add(younger),
                "MU2" => older.saturating_mul(younger),
                "DI2" => older / js_divisor(younger),
                "SU2" => older.saturating_sub(younger),
                _ => unreachable!(),
            };
            prog.user_variables.insert(older_name, next);
            true
        }
        _ => false,
    }
}

#[derive(Clone, Copy)]
pub(crate) struct WritableStateContext<'a> {
    pub(crate) pos: &'a PlayerPosition,
    pub(crate) stats: &'a PlayerStats,
    pub(crate) settings: &'a crate::game::player::PlayerSettings,
    pub(crate) world: &'a crate::world::World,
    pub(crate) geo_count: usize,
}

pub(crate) fn execute_writable_state(
    action: &PAction,
    prog: &mut ProgrammatorState,
    ctx: WritableStateContext<'_>,
    delay: &mut Option<Duration>,
) -> ExecResult {
    if action.label.eq_ignore_ascii_case("del") {
        *delay = Some(delay_millis(u64::try_from(action.num).unwrap_or(0)));
        return ExecResult::None;
    }
    if run_programmator_command(prog, &action.label, action.num, ctx) {
        return ExecResult::None;
    }
    if let Some(value) = readonly_programmator_value(
        &action.label,
        prog,
        ctx.pos,
        ctx.stats,
        ctx.settings,
        ctx.world,
        ctx.geo_count,
    ) {
        let result = compare_value(action.action_type, value, action.num);
        set_condition_state(prog, result);
        prog.last_variables.set(&action.label);
        return ExecResult::BoolResult(result);
    }
    let result = if let Some(value) = prog.user_variables.get(&action.label).copied() {
        compare_value(action.action_type, value, action.num)
    } else if action.action_type == ActionType::WritableState {
        prog.user_variables.insert(action.label.clone(), action.num);
        true
    } else {
        false
    };
    prog.last_variables.set(&action.label);
    set_condition_state(prog, result);
    ExecResult::BoolResult(result)
}

pub(crate) enum ExecResult {
    None,
    Label(String),
    BoolResult(bool),
}
