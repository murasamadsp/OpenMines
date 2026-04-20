use crate::game::player::{
    PlayerConnection, PlayerMetadata, PlayerPosition, PlayerSkills, PlayerStats,
};
use crate::game::skills::{SkillType, get_player_skill_effect};
use crate::game::{GameStateResource, ProgrammatorAction, ProgrammatorQueue};
use crate::world::WorldProvider;
use bevy_ecs::prelude::{Component, Query, Res, ResMut};
use std::collections::HashMap;
use std::time::Instant;

// ─── ActionType — 1:1 with C# reference ─────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ActionType {
    None,
    MoveUp,
    MoveLeft,
    MoveDown,
    MoveRight,
    MoveForward,
    RotateUp,
    RotateLeft,
    RotateDown,
    RotateRight,
    RotateLeftRelative,
    RotateRightRelative,
    RotateRandom,
    Dig,
    BuildBlock,
    BuildPillar,
    BuildRoad,
    BuildMilitaryBlock,
    Geology,
    Heal,
    NextRow,
    CreateFunction,
    GoTo,
    WritableStateMore,
    WritableStateLower,
    WritableState,
    RunSub,
    RunFunction,
    RunState,
    RunOnRespawn,
    RunIfTrue,
    RunIfFalse,
    Return,
    ReturnFunction,
    ReturnState,
    Start,
    Stop,
    Beep,
    CheckUp,
    CheckLeft,
    CheckDown,
    CheckRight,
    CheckUpLeft,
    CheckUpRight,
    CheckDownLeft,
    CheckDownRight,
    CheckForward,
    CheckForwardLeft,
    CheckForwardRight,
    CheckLeftRelative,
    CheckRightRelative,
    ShiftUp,
    ShiftLeft,
    ShiftDown,
    ShiftRight,
    ShiftForward,
    EnableAgression,
    DisableAgression,
    EnableAutoDig,
    DisableAutoDig,
    Flip,
    MacrosDig,
    MacrosBuild,
    MacrosHeal,
    MacrosMine,
    Or,
    And,
    IsHpLower100,
    IsHpLower50,
    IsNotEmpty,
    IsEmpty,
    IsFalling,
    IsCrystal,
    IsLivingCrystal,
    IsBoulder,
    IsSand,
    IsBreakableRock,
    IsUnbreakable,
    IsAcid,
    IsRedRock,
    IsBlackRock,
    IsGreenBlock,
    IsYellowBlock,
    IsRedBlock,
    IsPillar,
    IsQuadBlock,
    IsRoad,
    IsBox,
    CheckGun,
    FillGun,
}

fn get_action_type(id: u8) -> ActionType {
    match id {
        0 => ActionType::None,
        1 => ActionType::NextRow,
        2 => ActionType::Start,
        3 => ActionType::Stop,
        4 => ActionType::MoveUp,
        5 => ActionType::MoveLeft,
        6 => ActionType::MoveDown,
        7 => ActionType::MoveRight,
        8 => ActionType::Dig,
        9 => ActionType::RotateUp,
        10 => ActionType::RotateLeft,
        11 => ActionType::RotateDown,
        12 => ActionType::RotateRight,
        14 => ActionType::MoveForward,
        15 => ActionType::RotateLeftRelative,
        16 => ActionType::RotateRightRelative,
        17 => ActionType::BuildBlock,
        18 => ActionType::Geology,
        19 => ActionType::BuildRoad,
        20 => ActionType::Heal,
        21 => ActionType::BuildPillar,
        22 => ActionType::RotateRandom,
        23 => ActionType::Beep,
        24 => ActionType::GoTo,
        25 => ActionType::RunSub,
        26 => ActionType::RunFunction,
        27 => ActionType::Return,
        28 => ActionType::ReturnFunction,
        29 => ActionType::CheckUpLeft,
        30 => ActionType::CheckDownRight,
        31 => ActionType::CheckUp,
        32 => ActionType::CheckUpRight,
        33 => ActionType::CheckLeft,
        35 => ActionType::CheckRight,
        36 => ActionType::CheckDownLeft,
        37 => ActionType::CheckDown,
        38 => ActionType::Or,
        39 => ActionType::And,
        40 => ActionType::CreateFunction,
        43 => ActionType::IsNotEmpty,
        44 => ActionType::IsEmpty,
        45 => ActionType::IsFalling,
        46 => ActionType::IsCrystal,
        47 => ActionType::IsLivingCrystal,
        48 => ActionType::IsBoulder,
        49 => ActionType::IsSand,
        50 => ActionType::IsBreakableRock,
        51 => ActionType::IsUnbreakable,
        52 => ActionType::IsRedRock,
        53 => ActionType::IsBlackRock,
        54 => ActionType::IsAcid,
        57 => ActionType::IsQuadBlock,
        58 => ActionType::IsRoad,
        59 => ActionType::IsRedBlock,
        60 => ActionType::IsYellowBlock,
        74 => ActionType::IsBox,
        76 => ActionType::IsPillar,
        77 => ActionType::IsGreenBlock,
        119 => ActionType::WritableStateMore,
        120 => ActionType::WritableStateLower,
        123 => ActionType::WritableState,
        131 => ActionType::ShiftUp,
        132 => ActionType::ShiftLeft,
        133 => ActionType::ShiftDown,
        134 => ActionType::ShiftRight,
        135 => ActionType::CheckForward,
        136 => ActionType::ShiftForward,
        137 => ActionType::RunState,
        138 => ActionType::ReturnState,
        139 => ActionType::RunIfFalse,
        140 => ActionType::RunIfTrue,
        141 => ActionType::MacrosDig,
        142 => ActionType::MacrosBuild,
        143 => ActionType::MacrosHeal,
        144 => ActionType::Flip,
        145 => ActionType::MacrosMine,
        146 => ActionType::CheckGun,
        147 => ActionType::FillGun,
        148 => ActionType::IsHpLower100,
        149 => ActionType::IsHpLower50,
        156 => ActionType::CheckForwardLeft,
        157 => ActionType::CheckForwardRight,
        158 => ActionType::EnableAutoDig,
        159 => ActionType::DisableAutoDig,
        160 => ActionType::EnableAgression,
        161 => ActionType::DisableAgression,
        166 => ActionType::RunOnRespawn,
        _ => ActionType::None,
    }
}

// ─── PAction / PFunction ─────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct PAction {
    pub action_type: ActionType,
    pub label: String,
    pub num: i32,
}

#[derive(Clone, Debug)]
pub struct PFunction {
    pub actions: Vec<PAction>,
    pub current: usize,
    pub state: Option<bool>,
    pub last_state_action: Option<ActionType>,
    pub startoffset: (i32, i32),
    pub called_from: Option<String>,
}

impl PFunction {
    fn new() -> Self {
        Self {
            actions: Vec::new(),
            current: 0,
            state: None,
            last_state_action: None,
            startoffset: (0, 0),
            called_from: None,
        }
    }

    fn reset(&mut self) {
        self.current = 0;
        self.startoffset = (0, 0);
    }
}

// ─── ProgrammatorState — ECS component ──────────────────────────────────────

#[derive(Component, Debug)]
pub struct ProgrammatorState {
    pub running: bool,
    pub current_prog: HashMap<String, PFunction>,
    pub function_order: Vec<String>,
    pub current_function: String,
    pub delay: Instant,
    pub selected_id: Option<i32>,
    pub selected_data: Option<String>,
    pub shift_x: i32,
    pub shift_y: i32,
    pub check_x: i32,
    pub check_y: i32,
    pub flip_state: bool,
    pub startpoint: (String, usize),
    pub goto_death: Option<String>,
}

impl ProgrammatorState {
    pub fn new() -> Self {
        Self {
            running: false,
            current_prog: HashMap::new(),
            function_order: Vec::new(),
            current_function: String::new(),
            delay: Instant::now(),
            selected_id: None,
            selected_data: None,
            shift_x: 0,
            shift_y: 0,
            check_x: 0,
            check_y: 0,
            flip_state: false,
            startpoint: (String::new(), 0),
            goto_death: None,
        }
    }

    /// Parse PROG payload from client: [4B len i32 LE][4B id i32 LE][...][UTF-8 source]
    /// Returns (id, source) or None on failure.
    pub fn decode_prog_packet(payload: &[u8]) -> Option<(i32, String)> {
        if payload.len() < 8 {
            return None;
        }
        let len = i32::from_le_bytes(payload[0..4].try_into().ok()?) as usize;
        let id = i32::from_le_bytes(payload[4..8].try_into().ok()?);
        // Source is UTF-8 after the header + len bytes
        let source_start = 8 + len;
        if source_start > payload.len() {
            // Source might be empty
            return Some((id, String::new()));
        }
        let source = String::from_utf8_lossy(&payload[source_start..]).to_string();
        Some((id, source))
    }

    /// Parse script from base64-encoded LZMA data (the "normal" format from C# `parseNormal`).
    pub fn parse_normal(data: &str) -> Option<(HashMap<String, PFunction>, Vec<String>)> {
        if data.is_empty() {
            return None;
        }

        let decoded =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data).ok()?;

        // LZMA decompress
        let mut decompressed = Vec::new();
        let mut reader = std::io::Cursor::new(&decoded);
        lzma_rs::lzma_decompress(&mut reader, &mut decompressed).ok()?;

        if decompressed.len() < 4 {
            return None;
        }

        let num = i32::from_le_bytes(decompressed[0..4].try_into().ok()?) as usize;
        if decompressed.len() < 4 + num {
            return None;
        }

        // Action bytes start at offset 4, labels UTF-8 after 4+num
        let labels_str = if decompressed.len() > 4 + num {
            String::from_utf8_lossy(&decompressed[4 + num..]).to_string()
        } else {
            String::new()
        };
        let labels: Vec<&str> = labels_str.split(':').collect();

        let mut functions: HashMap<String, PFunction> = HashMap::new();
        let mut function_order: Vec<String> = Vec::new();
        functions.insert(String::new(), PFunction::new());
        function_order.push(String::new());
        let mut current_func = String::new();
        let mut contains_nextrow = false;
        let mut index = 0;

        for i in 0..num {
            let atype = get_action_type(decompressed[i + 4]);

            let mut name = "0".to_string();
            let mut number = 0i32;
            if i < labels.len() {
                let lbl = labels[i];
                if let Some(at_pos) = lbl.find('@') {
                    name = lbl[..at_pos].to_string();
                    if let Ok(n) = lbl[at_pos + 1..].parse::<i32>() {
                        number = n;
                    }
                } else {
                    name = lbl.to_string();
                }
            }

            match atype {
                ActionType::NextRow => {
                    contains_nextrow = true;
                }
                ActionType::CreateFunction => {
                    functions.insert(name.clone(), PFunction::new());
                    function_order.push(name.clone());
                    current_func = name;
                    index = 0;
                }
                ActionType::WritableState
                | ActionType::WritableStateLower
                | ActionType::WritableStateMore => {
                    if let Some(f) = functions.get_mut(&current_func) {
                        f.actions.push(PAction {
                            action_type: atype,
                            label: name,
                            num: number,
                        });
                    }
                }
                ActionType::RunFunction
                | ActionType::RunIfFalse
                | ActionType::RunIfTrue
                | ActionType::RunOnRespawn
                | ActionType::RunState
                | ActionType::RunSub
                | ActionType::GoTo => {
                    if let Some(f) = functions.get_mut(&current_func) {
                        f.actions.push(PAction {
                            action_type: atype,
                            label: name,
                            num: 0,
                        });
                    }
                }
                ActionType::None => {}
                _ => {
                    if let Some(f) = functions.get_mut(&current_func) {
                        f.actions.push(PAction {
                            action_type: atype,
                            label: String::new(),
                            num: 0,
                        });
                    }
                }
            }

            if index > 0 && index % 15 == 0 {
                if let Some(f) = functions.get_mut(&current_func) {
                    let should_add_goto = !f.actions.is_empty()
                        && f.actions
                            .last()
                            .is_none_or(|a| a.action_type != ActionType::GoTo)
                        && !contains_nextrow;
                    if should_add_goto {
                        f.actions.push(PAction {
                            action_type: ActionType::GoTo,
                            label: String::new(),
                            num: 0,
                        });
                    }
                }
                index = 0;
                contains_nextrow = false;
            }
            index += 1;
        }

        Some((functions, function_order))
    }

    /// Start running a program (equivalent to C# `ProgrammatorData.Run(Program p)`).
    pub fn run_program(&mut self, data: &str) {
        if let Some((functions, order)) = Self::parse_normal(data) {
            self.current_prog = functions;
            self.function_order = order;
            self.delay = Instant::now();
            self.drop_state();
            self.running = true;
        }
    }

    /// Toggle run/stop (equivalent to C# `ProgrammatorData.Run()` no-arg).
    pub fn toggle_run(&mut self) {
        if self.running || self.selected_data.is_none() {
            self.running = false;
            return;
        }
        if let Some(data) = self.selected_data.clone() {
            self.run_program(&data);
        }
    }

    fn drop_state(&mut self) {
        self.startpoint = (String::new(), 0);
        self.goto_death = None;
        self.current_function = String::new();
        self.check_x = 0;
        self.check_y = 0;
        self.shift_x = 0;
        self.shift_y = 0;
        self.flip_state = false;
        for f in self.current_prog.values_mut() {
            f.reset();
        }
    }

    fn next_function(&mut self) {
        let idx = self
            .function_order
            .iter()
            .position(|k| k == &self.current_function);
        if let Some(i) = idx {
            if i + 1 < self.function_order.len() {
                self.current_function = self.function_order[i + 1].clone();
            } else {
                self.current_function = self.function_order[0].clone();
            }
        } else if let Some(first) = self.function_order.first() {
            self.current_function = first.clone();
        }
    }
}

// ─── Execution result ────────────────────────────────────────────────────────

enum ExecResult {
    None,
    Label(String),
    BoolResult(bool),
}

// ─── Cell check helper ───────────────────────────────────────────────────────

fn check_cell(
    prog: &mut ProgrammatorState,
    pos: &PlayerPosition,
    world: &crate::world::World,
    predicate: impl FnOnce(i32, i32, &crate::world::World) -> bool,
) {
    let (sx, sy) = {
        let f = prog.current_prog.get(&prog.current_function);
        if let Some(f) = f {
            if f.startoffset != (0, 0) {
                f.startoffset
            } else {
                (prog.shift_x + prog.check_x, prog.shift_y + prog.check_y)
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
            None => f.state = Some(result),
            Some(ActionType::Or) => f.state = Some(f.state.unwrap_or(false) || result),
            Some(ActionType::And) => f.state = Some(f.state.unwrap_or(true) && result),
            _ => f.state = Some(result),
        }
    }
}

// ─── Main ECS system ─────────────────────────────────────────────────────────

#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]
pub fn programmator_system(
    state_res: Res<GameStateResource>,
    mut prog_q: ResMut<ProgrammatorQueue>,
    mut query: Query<(
        &PlayerMetadata,
        &PlayerPosition,
        &PlayerConnection,
        &PlayerStats,
        &PlayerSkills,
        &mut ProgrammatorState,
    )>,
) {
    let now = Instant::now();

    for (meta, pos, conn, stats, skills, mut prog) in &mut query {
        if !prog.running {
            continue;
        }
        if now < prog.delay {
            continue;
        }

        // Get current function actions count
        let (action_count, current_pos) = {
            let f = prog.current_prog.get(&prog.current_function);
            match f {
                Some(f) => (f.actions.len(), f.current),
                None => {
                    prog.running = false;
                    continue;
                }
            }
        };

        // If function exhausted, reset and move to next
        if action_count == 0 || current_pos >= action_count {
            let cf = prog.current_function.clone();
            if let Some(f) = prog.current_prog.get_mut(&cf) {
                f.reset();
            }
            prog.next_function();
            continue;
        }

        // Get next action
        let action = {
            let cf = prog.current_function.clone();
            let f = prog.current_prog.get_mut(&cf).unwrap();
            let a = f.actions[f.current].clone();
            f.current += 1;
            a
        };

        let mut delay_ms: u64 = 0;

        // Execute action and get result
        let result = execute_action(
            &action,
            &mut prog,
            pos,
            stats,
            skills,
            &state_res.0,
            meta,
            conn,
            &mut prog_q,
            &mut delay_ms,
        );

        // Process result (matching C# `ProgrammatorData.Step()`)
        match result {
            ExecResult::Label(label) => {
                handle_label_result(&action, &label, &mut prog);
            }
            ExecResult::BoolResult(state) => {
                handle_bool_result(&action, state, &mut prog);
            }
            ExecResult::None => {
                handle_none_result(&action, &mut prog);
            }
        }

        // Set delay
        if delay_ms > 0 {
            prog.delay = now + std::time::Duration::from_millis(delay_ms);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_action(
    action: &PAction,
    prog: &mut ProgrammatorState,
    pos: &PlayerPosition,
    stats: &PlayerStats,
    skills: &PlayerSkills,
    state: &std::sync::Arc<crate::game::GameState>,
    meta: &PlayerMetadata,
    conn: &PlayerConnection,
    prog_q: &mut ProgrammatorQueue,
    delay_ms: &mut u64,
) -> ExecResult {
    match action.action_type {
        // ─── Movement ────────────────────────────────────────────────────
        ActionType::MoveDown => {
            *delay_ms = speed_pause(skills);
            push_move(prog_q, meta, conn, pos.x, pos.y + 1, 0);
            ExecResult::None
        }
        ActionType::MoveUp => {
            *delay_ms = speed_pause(skills);
            push_move(prog_q, meta, conn, pos.x, pos.y - 1, 2);
            ExecResult::None
        }
        ActionType::MoveRight => {
            *delay_ms = speed_pause(skills);
            push_move(prog_q, meta, conn, pos.x + 1, pos.y, 3);
            ExecResult::None
        }
        ActionType::MoveLeft => {
            *delay_ms = speed_pause(skills);
            push_move(prog_q, meta, conn, pos.x - 1, pos.y, 1);
            ExecResult::None
        }
        ActionType::MoveForward => {
            *delay_ms = speed_pause(skills);
            let (dx, dy) = crate::game::direction::dir_offset(pos.dir);
            push_move(prog_q, meta, conn, pos.x + dx, pos.y + dy, pos.dir);
            ExecResult::None
        }

        // ─── Rotation ────────────────────────────────────────────────────
        ActionType::RotateDown => {
            *delay_ms = speed_pause(skills);
            push_move(prog_q, meta, conn, pos.x, pos.y, 0);
            ExecResult::None
        }
        ActionType::RotateUp => {
            *delay_ms = speed_pause(skills);
            push_move(prog_q, meta, conn, pos.x, pos.y, 2);
            ExecResult::None
        }
        ActionType::RotateLeft => {
            *delay_ms = speed_pause(skills);
            push_move(prog_q, meta, conn, pos.x, pos.y, 1);
            ExecResult::None
        }
        ActionType::RotateRight => {
            *delay_ms = speed_pause(skills);
            push_move(prog_q, meta, conn, pos.x, pos.y, 3);
            ExecResult::None
        }
        ActionType::RotateLeftRelative => {
            *delay_ms = speed_pause(skills);
            let d = match pos.dir {
                0 => 3,
                1 => 0,
                2 => 1,
                3 => 2,
                _ => 0,
            };
            push_move(prog_q, meta, conn, pos.x, pos.y, d);
            ExecResult::None
        }
        ActionType::RotateRightRelative => {
            *delay_ms = speed_pause(skills);
            let d = match pos.dir {
                0 => 1,
                1 => 2,
                2 => 3,
                3 => 0,
                _ => 0,
            };
            push_move(prog_q, meta, conn, pos.x, pos.y, d);
            ExecResult::None
        }
        ActionType::RotateRandom => {
            *delay_ms = speed_pause(skills);
            let d = rand::random_range(0..4);
            push_move(prog_q, meta, conn, pos.x, pos.y, d);
            ExecResult::None
        }

        // ─── Dig / Build ─────────────────────────────────────────────────
        ActionType::Dig => {
            *delay_ms = 100;
            prog_q.0.push(ProgrammatorAction::Dig {
                pid: meta.id,
                tx: conn.tx.clone(),
                dir: pos.dir,
            });
            ExecResult::None
        }
        ActionType::BuildBlock => {
            *delay_ms = 100;
            prog_q.0.push(ProgrammatorAction::Build {
                pid: meta.id,
                tx: conn.tx.clone(),
                dir: pos.dir,
                block_type: "G".to_string(),
            });
            ExecResult::None
        }
        ActionType::BuildPillar => {
            *delay_ms = 100;
            prog_q.0.push(ProgrammatorAction::Build {
                pid: meta.id,
                tx: conn.tx.clone(),
                dir: pos.dir,
                block_type: "O".to_string(),
            });
            ExecResult::None
        }
        ActionType::BuildRoad => {
            *delay_ms = 100;
            prog_q.0.push(ProgrammatorAction::Build {
                pid: meta.id,
                tx: conn.tx.clone(),
                dir: pos.dir,
                block_type: "R".to_string(),
            });
            ExecResult::None
        }
        ActionType::BuildMilitaryBlock => {
            *delay_ms = 100;
            prog_q.0.push(ProgrammatorAction::Build {
                pid: meta.id,
                tx: conn.tx.clone(),
                dir: pos.dir,
                block_type: "V".to_string(),
            });
            ExecResult::None
        }
        ActionType::Geology => {
            *delay_ms = 100;
            prog_q.0.push(ProgrammatorAction::Geo {
                pid: meta.id,
                tx: conn.tx.clone(),
            });
            ExecResult::None
        }
        ActionType::Heal => {
            prog_q.0.push(ProgrammatorAction::Heal {
                pid: meta.id,
                tx: conn.tx.clone(),
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
            check_cell(&mut *prog, pos, &state.world, |x, y, w| {
                w.cell_defs().get(w.get_cell(x, y)).cell_is_empty()
            });
            ExecResult::None
        }
        ActionType::IsNotEmpty => {
            check_cell(&mut *prog, pos, &state.world, |x, y, w| {
                !w.cell_defs().get(w.get_cell(x, y)).cell_is_empty()
            });
            ExecResult::None
        }
        ActionType::IsCrystal => {
            check_cell(&mut *prog, pos, &state.world, |x, y, w| {
                let cell = w.get_cell(x, y);
                crate::world::cells::is_crystal(cell)
            });
            ExecResult::None
        }
        ActionType::IsBoulder => {
            check_cell(&mut *prog, pos, &state.world, |x, y, w| {
                w.cell_defs().get(w.get_cell(x, y)).nature.is_boulder
            });
            ExecResult::None
        }
        ActionType::IsSand => {
            check_cell(&mut *prog, pos, &state.world, |x, y, w| {
                w.cell_defs().get(w.get_cell(x, y)).is_sand()
            });
            ExecResult::None
        }
        ActionType::IsBreakableRock => {
            check_cell(&mut *prog, pos, &state.world, |x, y, w| {
                w.cell_defs().get(w.get_cell(x, y)).is_diggable()
            });
            ExecResult::None
        }
        ActionType::IsUnbreakable => {
            check_cell(&mut *prog, pos, &state.world, |x, y, w| {
                let defs = w.cell_defs();
                let def = defs.get(w.get_cell(x, y));
                !def.cell_is_empty() && !def.is_diggable()
            });
            ExecResult::None
        }
        ActionType::IsFalling => {
            check_cell(&mut *prog, pos, &state.world, |x, y, w| {
                let defs = w.cell_defs();
                let def = defs.get(w.get_cell(x, y));
                def.is_sand() || def.nature.is_boulder
            });
            ExecResult::None
        }
        ActionType::IsRoad => {
            check_cell(&mut *prog, pos, &state.world, |x, y, w| {
                w.get_road_cell(x, y) != 0
            });
            ExecResult::None
        }
        ActionType::IsBox => {
            check_cell(&mut *prog, pos, &state.world, |x, y, w| {
                w.get_cell(x, y) == crate::world::cells::cell_type::BOX
            });
            ExecResult::None
        }
        ActionType::IsGreenBlock => {
            check_cell(&mut *prog, pos, &state.world, |x, y, w| {
                w.get_cell(x, y) == crate::world::cells::cell_type::GREEN_BLOCK
            });
            ExecResult::None
        }
        ActionType::IsYellowBlock => {
            check_cell(&mut *prog, pos, &state.world, |x, y, w| {
                w.get_cell(x, y) == crate::world::cells::cell_type::YELLOW_BLOCK
            });
            ExecResult::None
        }
        ActionType::IsRedBlock => {
            check_cell(&mut *prog, pos, &state.world, |x, y, w| {
                w.get_cell(x, y) == crate::world::cells::cell_type::RED_BLOCK
            });
            ExecResult::None
        }
        ActionType::IsPillar => {
            check_cell(&mut *prog, pos, &state.world, |x, y, w| {
                w.get_cell(x, y) == crate::world::cells::cell_type::SUPPORT
            });
            ExecResult::None
        }
        ActionType::IsQuadBlock => {
            check_cell(&mut *prog, pos, &state.world, |x, y, w| {
                w.get_cell(x, y) == crate::world::cells::cell_type::QUAD_BLOCK
            });
            ExecResult::None
        }
        ActionType::IsRedRock => {
            check_cell(&mut *prog, pos, &state.world, |x, y, w| {
                w.get_cell(x, y) == crate::world::cells::cell_type::RED_ROCK
            });
            ExecResult::None
        }
        ActionType::IsBlackRock => {
            check_cell(&mut *prog, pos, &state.world, |x, y, w| {
                w.get_cell(x, y) == crate::world::cells::cell_type::BLACK_ROCK
            });
            ExecResult::None
        }
        ActionType::IsAcid => {
            check_cell(&mut *prog, pos, &state.world, |x, y, w| {
                let c = w.get_cell(x, y);
                // C# IsAcid: GrayAcid(66), PurpleAcid(67), PassiveAcid(86),
                // LivingActiveAcid(95), CorrosiveActiveAcid(96), AcidRock(118)
                matches!(c, 66 | 67 | 86 | 95 | 96 | 118)
            });
            ExecResult::None
        }
        ActionType::IsLivingCrystal => {
            check_cell(&mut *prog, pos, &state.world, |x, y, w| {
                let c = w.get_cell(x, y);
                // C# isAlive: AliveCyan(50)..AliveRainbow(55), AliveBlue(116)
                matches!(c, 50..=55 | 116)
            });
            ExecResult::None
        }
        ActionType::IsHpLower100 => {
            let result = stats.health < stats.max_health;
            if let Some(f) = prog.current_prog.get_mut(&prog.current_function) {
                match f.last_state_action {
                    None => f.state = Some(result),
                    Some(ActionType::Or) => f.state = Some(f.state.unwrap_or(false) || result),
                    Some(ActionType::And) => f.state = Some(f.state.unwrap_or(true) && result),
                    _ => f.state = Some(result),
                }
            }
            ExecResult::None
        }
        ActionType::IsHpLower50 => {
            let result = stats.health < stats.max_health / 2;
            if let Some(f) = prog.current_prog.get_mut(&prog.current_function) {
                match f.last_state_action {
                    None => f.state = Some(result),
                    Some(ActionType::Or) => f.state = Some(f.state.unwrap_or(false) || result),
                    Some(ActionType::And) => f.state = Some(f.state.unwrap_or(true) && result),
                    _ => f.state = Some(result),
                }
            }
            ExecResult::None
        }

        // ─── Flow control ───────────────────────────────────────────────
        ActionType::GoTo => ExecResult::Label(action.label.clone()),
        ActionType::RunSub
        | ActionType::RunFunction
        | ActionType::RunState
        | ActionType::RunOnRespawn => ExecResult::Label(action.label.clone()),
        ActionType::RunIfTrue => {
            let state_val = prog
                .current_prog
                .get(&prog.current_function)
                .and_then(|f| f.state);
            if let Some(false) = state_val {
                // Condition is false, don't jump
                if let Some(f) = prog.current_prog.get_mut(&prog.current_function) {
                    f.state = None;
                }
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
            if let Some(true) = state_val {
                // Condition is true, don't jump
                if let Some(f) = prog.current_prog.get_mut(&prog.current_function) {
                    f.state = None;
                }
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

        // ─── Control ────────────────────────────────────────────────────
        ActionType::Start
        | ActionType::Stop
        | ActionType::Return
        | ActionType::ReturnState
        | ActionType::Flip => ExecResult::None,

        ActionType::Beep => {
            // Send BB (beep sound) to player via protocol encoding
            let pkt = crate::protocol::u_packet("BB", &[]);
            let mut buf = bytes::BytesMut::with_capacity(pkt.wire_len());
            if pkt.encode(&mut buf).is_ok() {
                let _ = conn.tx.send(buf.to_vec());
            }
            ExecResult::None
        }

        ActionType::EnableAutoDig => {
            prog_q.0.push(ProgrammatorAction::SetAutoDig {
                pid: meta.id,
                tx: conn.tx.clone(),
                enabled: true,
            });
            ExecResult::None
        }
        ActionType::DisableAutoDig => {
            prog_q.0.push(ProgrammatorAction::SetAutoDig {
                pid: meta.id,
                tx: conn.tx.clone(),
                enabled: false,
            });
            ExecResult::None
        }

        // ─── Macros (simplified) ────────────────────────────────────────
        ActionType::MacrosDig => {
            let (dx, dy) = crate::game::direction::dir_offset(pos.dir);
            let tx = pos.x + dx;
            let ty = pos.y + dy;
            {
                let diggable = state
                    .world
                    .cell_defs()
                    .get(state.world.get_cell(tx, ty))
                    .is_diggable();
                if diggable {
                    *delay_ms = 200;
                    prog_q.0.push(ProgrammatorAction::Dig {
                        pid: meta.id,
                        tx: conn.tx.clone(),
                        dir: pos.dir,
                    });
                    return ExecResult::BoolResult(true);
                }
            }
            ExecResult::None
        }
        ActionType::MacrosHeal => {
            if stats.health < stats.max_health {
                prog_q.0.push(ProgrammatorAction::Heal {
                    pid: meta.id,
                    tx: conn.tx.clone(),
                });
                *delay_ms = 200;
                return ExecResult::BoolResult(true);
            }
            ExecResult::None
        }
        ActionType::MacrosBuild => {
            *delay_ms = 100;
            prog_q.0.push(ProgrammatorAction::Build {
                pid: meta.id,
                tx: conn.tx.clone(),
                dir: pos.dir,
                block_type: "G".to_string(),
            });
            ExecResult::None
        }
        ActionType::MacrosMine => {
            // Simplified: dig in current direction if crystal
            let (dx, dy) = crate::game::direction::dir_offset(pos.dir);
            let tx = pos.x + dx;
            let ty = pos.y + dy;
            let cell = state.world.get_cell(tx, ty);
            if crate::world::cells::is_crystal(cell) {
                *delay_ms = 200;
                prog_q.0.push(ProgrammatorAction::Dig {
                    pid: meta.id,
                    tx: conn.tx.clone(),
                    dir: pos.dir,
                });
                return ExecResult::BoolResult(true);
            }
            ExecResult::None
        }

        // ─── Writable state / other ─────────────────────────────────────
        ActionType::WritableState
        | ActionType::WritableStateLower
        | ActionType::WritableStateMore => {
            // WritableState with "del" label = set delay
            if action.label.eq_ignore_ascii_case("del") {
                *delay_ms = action.num as u64;
            }
            ExecResult::None
        }

        _ => ExecResult::None,
    }
}

fn handle_label_result(action: &PAction, label: &str, prog: &mut ProgrammatorState) {
    match action.action_type {
        ActionType::GoTo => {
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
                    .map(|f| (f.state, f.last_state_action))
                    .unwrap_or((None, None));
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
        ActionType::RunOnRespawn => {
            if prog.current_prog.contains_key(label) {
                prog.goto_death = Some(label.to_string());
            }
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
                prog.current_function = caller.clone();
                if let Some(f) = prog.current_prog.get_mut(&caller) {
                    f.state = Some(state);
                    f.startoffset = (0, 0);
                }
            }
        }
        ActionType::MacrosDig | ActionType::MacrosHeal | ActionType::MacrosMine => {
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
            let (state_val, last_state, called_from) = prog
                .current_prog
                .get(&cf)
                .map(|f| (f.state, f.last_state_action, f.called_from.clone()))
                .unwrap_or((None, None, None));
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
        ActionType::Stop => {
            prog.toggle_run();
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
    conn: &PlayerConnection,
    x: i32,
    y: i32,
    dir: i32,
) {
    prog_q.0.push(ProgrammatorAction::Move {
        pid: meta.id,
        tx: conn.tx.clone(),
        x,
        y,
        dir,
    });
}

/// Calculate movement pause in ms from skills (C# `Player.ServerPause` = `pause / 10`).
/// `pause = (int)(Movement.Effect * 100)`, `ServerPause = pause / 10`.
fn speed_pause(skills: &PlayerSkills) -> u64 {
    let move_effect = get_player_skill_effect(&skills.states, SkillType::Movement);
    let pause = (move_effect * 100.0) as u64;
    // ServerPause = pause / 10; minimum 50ms
    (pause / 10).max(50)
}
