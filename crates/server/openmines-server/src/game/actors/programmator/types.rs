use bevy_ecs::prelude::Component;
use std::collections::HashMap;
use std::time::Instant;

// ─── ActionType — 1:1 with C# reference ─────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    IsSlime,
    IsInGun,
    HandModeOn,
    HandModeOff,
    MacrosGun,
    MacrosDigAround,
    OnlineGeo,
    OnlineZz,
    OnlineC190,
    OnlinePoly,
    OnlineUp,
    OnlineCraft,
    OnlineNano,
    OnlineRem,
    InventoryUp,
    InventoryLeft,
    InventoryDown,
    InventoryRight,
    DebugMessage,
    DebugPause,
    RestartRow,
}

pub(crate) const fn get_action_type(id: u8) -> ActionType {
    match id {
        162 => ActionType::BuildBlock,
        163 => ActionType::BuildPillar,
        164 => ActionType::BuildRoad,
        165 => ActionType::BuildMilitaryBlock,
        166 => ActionType::RunOnRespawn,
        167 => ActionType::OnlineGeo,
        168 => ActionType::OnlineZz,
        169 => ActionType::OnlineC190,
        170 => ActionType::OnlinePoly,
        171 => ActionType::OnlineUp,
        172 => ActionType::OnlineCraft,
        173 => ActionType::OnlineNano,
        174 => ActionType::OnlineRem,
        175 => ActionType::InventoryUp,
        176 => ActionType::InventoryLeft,
        177 => ActionType::InventoryDown,
        178 => ActionType::InventoryRight,
        179 => ActionType::HandModeOn,
        180 => ActionType::HandModeOff,
        181 => ActionType::DebugMessage,
        182 => ActionType::DebugPause,
        200 => ActionType::RestartRow,
        _ => get_legacy_action_type(id),
    }
}

const fn get_legacy_action_type(id: u8) -> ActionType {
    match id {
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
        98 => ActionType::IsSlime,
        106 => ActionType::IsInGun,
        _ => ActionType::None,
    }
}

// ─── PAction / PFunction ─────────────────────────────────────────────────────

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PAction {
    pub action_type: ActionType,
    pub label: String,
    pub num: i32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PFunction {
    pub actions: Vec<PAction>,
    pub current: usize,
    pub state: Option<bool>,
    pub last_state_action: Option<ActionType>,
    pub startoffset: (i32, i32),
    pub called_from: Option<String>,
}

impl PFunction {
    pub(crate) const fn new() -> Self {
        Self {
            actions: Vec::new(),
            current: 0,
            state: None,
            last_state_action: None,
            startoffset: (0, 0),
            called_from: None,
        }
    }

    pub const fn reset(&mut self) {
        self.current = 0;
        self.startoffset = (0, 0);
    }
}

// ─── ProgrammatorState — ECS component ──────────────────────────────────────

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ProgrammatorSnapshot {
    pub running: bool,
    pub current_prog: HashMap<String, PFunction>,
    pub function_order: Vec<String>,
    pub current_function: String,
    pub selected_id: Option<i32>,
    pub selected_data: Option<String>,
    pub shift_x: i32,
    pub shift_y: i32,
    pub check_x: i32,
    pub check_y: i32,
    pub flip_state: bool,
    pub startpoint: (String, usize),
    pub goto_death: Option<String>,
    pub macros_template: Option<i32>,
    pub hand_mode_active: bool,
    pub user_variables: HashMap<String, i32>,
    pub last_variables: LastVariables,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct LastVariables {
    pub(crate) younger: Option<String>,
    pub(crate) older: Option<String>,
}

impl LastVariables {
    pub(crate) fn set(&mut self, name: &str) {
        self.older = self.younger.replace(name.to_string());
    }

    pub(crate) fn younger(&self) -> Option<&str> {
        self.younger.as_deref()
    }

    pub(crate) fn older(&self) -> Option<&str> {
        self.older.as_deref()
    }
}

#[derive(Component, Debug)]
pub struct ProgrammatorState {
    pub running: bool,
    pub current_prog: HashMap<String, PFunction>,
    pub function_order: Vec<String>,
    pub current_function: String,
    pub delay: Instant,
    pub started_at: Instant,
    pub selected_id: Option<i32>,
    pub selected_data: Option<String>,
    pub shift_x: i32,
    pub shift_y: i32,
    pub check_x: i32,
    pub check_y: i32,
    pub flip_state: bool,
    pub startpoint: (String, usize),
    pub goto_death: Option<String>,
    pub macros_template: Option<i32>,
    pub hand_mode_active: bool,
    pub user_variables: HashMap<String, i32>,
    pub last_variables: LastVariables,
}

impl ProgrammatorState {
    pub fn new() -> Self {
        Self {
            running: false,
            current_prog: HashMap::new(),
            function_order: Vec::new(),
            current_function: String::new(),
            delay: Instant::now(),
            started_at: Instant::now(),
            selected_id: None,
            selected_data: None,
            shift_x: 0,
            shift_y: 0,
            check_x: 0,
            check_y: 0,
            flip_state: false,
            startpoint: (String::new(), 0),
            goto_death: None,
            macros_template: None,
            hand_mode_active: false,
            user_variables: HashMap::new(),
            last_variables: LastVariables::default(),
        }
    }

    #[inline]
    #[must_use]
    pub const fn is_manual_control_allowed(&self) -> bool {
        if self.running {
            self.hand_mode_active
        } else {
            true
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> ProgrammatorSnapshot {
        ProgrammatorSnapshot {
            running: self.running,
            current_prog: self.current_prog.clone(),
            function_order: self.function_order.clone(),
            current_function: self.current_function.clone(),
            selected_id: self.selected_id,
            selected_data: self.selected_data.clone(),
            shift_x: self.shift_x,
            shift_y: self.shift_y,
            check_x: self.check_x,
            check_y: self.check_y,
            flip_state: self.flip_state,
            startpoint: self.startpoint.clone(),
            goto_death: self.goto_death.clone(),
            macros_template: self.macros_template,
            hand_mode_active: self.hand_mode_active,
            user_variables: self.user_variables.clone(),
            last_variables: self.last_variables.clone(),
        }
    }

    pub fn restore_snapshot(&mut self, snapshot: ProgrammatorSnapshot) {
        self.running = snapshot.running;
        self.current_prog = snapshot.current_prog;
        self.function_order = snapshot.function_order;
        self.current_function = snapshot.current_function;
        self.delay = Instant::now();
        self.selected_id = snapshot.selected_id;
        self.selected_data = snapshot.selected_data;
        self.shift_x = snapshot.shift_x;
        self.shift_y = snapshot.shift_y;
        self.check_x = snapshot.check_x;
        self.check_y = snapshot.check_y;
        self.flip_state = snapshot.flip_state;
        self.startpoint = snapshot.startpoint;
        self.goto_death = snapshot.goto_death;
        self.macros_template = snapshot.macros_template;
        self.hand_mode_active = snapshot.hand_mode_active;
        self.user_variables = snapshot.user_variables;
        self.last_variables = snapshot.last_variables;
    }

    pub(crate) fn drop_state(&mut self) {
        self.startpoint = (String::new(), 0);
        self.goto_death = None;
        self.current_function = String::new();
        self.check_x = 0;
        self.check_y = 0;
        self.shift_x = 0;
        self.shift_y = 0;
        self.flip_state = false;
        self.hand_mode_active = false;
        self.user_variables.clear();
        self.last_variables = LastVariables::default();
        for f in self.current_prog.values_mut() {
            f.reset();
        }
    }

    pub(crate) fn next_function(&mut self) {
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
