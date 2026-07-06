use crate::game::player::{
    PlayerConnection, PlayerMetadata, PlayerPosition, PlayerSkillsComp, PlayerStats,
};
use crate::game::skills::{SkillType, get_player_skill_effect};
use crate::game::{ProgrammatorAction, ProgrammatorQueue, WorldResource};
use crate::world::WorldProvider;
use bevy_ecs::prelude::{Component, Query, Res, ResMut};
use num_traits::ToPrimitive;
use std::collections::HashMap;
use std::time::{Duration, Instant};

const ACTION_DELAY: Duration = Duration::from_micros(333_333);

const fn delay_millis(ms: u64) -> Duration {
    Duration::from_millis(ms)
}

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

const fn get_action_type(id: u8) -> ActionType {
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
        // 0 → None (как и любой неизвестный id, см. wildcard).
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
    const fn new() -> Self {
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
}

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
    /// C# `ProgrammatorData.temp` — состояние `MacrosMine` между тиками: направление,
    /// в котором бот сейчас копает (fast-path), либо None (нужен скан 4 направлений).
    pub macros_template: Option<i32>,
    /// JS ref `HAND_MODE`: когда `true` — программа не блокирует ручное управление
    /// (ботом можно двигать WASD даже при запущенной программе).
    pub hand_mode_active: bool,
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
            macros_template: None,
            hand_mode_active: false,
        }
    }

    /// Проверяет, разрешено ли ручное управление.
    /// Если программатор не запущен, ручной ход всегда разрешен.
    /// Если программатор запущен, ручной ход разрешен только при активном `hand_mode`.
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
    }

    /// Parse PROG payload from client: [4B len i32 LE][4B id i32 LE][...][UTF-8 source]
    /// Returns (id, source) or None on failure.
    pub fn decode_prog_packet(payload: &[u8]) -> Option<(i32, String)> {
        if payload.len() < 8 {
            return None;
        }
        let len = usize::try_from(i32::from_le_bytes(payload[0..4].try_into().ok()?))
            .unwrap_or(usize::MAX);
        let id = i32::from_le_bytes(payload[4..8].try_into().ok()?);
        // Source is UTF-8 after the header + len bytes
        let source_start = 8 + len;
        if source_start > payload.len() {
            return None;
        }
        let source = String::from_utf8_lossy(&payload[source_start..]).to_string();
        Some((id, source))
    }

    /// Декодировать base64+LZMA полезную нагрузку `parseNormal` →
    /// (распакованные байты, число действий, метки). Вынесено из
    /// `parse_normal` (лимит строк).
    fn decode_normal_payload(data: &str) -> Option<(Vec<u8>, usize, Vec<String>)> {
        if data.is_empty() {
            return None;
        }
        let decoded =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data).ok()?;
        let mut decompressed = Vec::new();
        let mut reader = std::io::Cursor::new(&decoded);
        lzma_rs::lzma_decompress(&mut reader, &mut decompressed).ok()?;
        if decompressed.len() < 4 {
            return None;
        }
        let num = usize::try_from(i32::from_le_bytes(decompressed[0..4].try_into().ok()?))
            .unwrap_or(usize::MAX);
        if decompressed.len() < 4 + num {
            return None;
        }
        // Action bytes start at offset 4, labels UTF-8 after 4+num
        let labels_str = if decompressed.len() > 4 + num {
            String::from_utf8_lossy(&decompressed[4 + num..]).to_string()
        } else {
            String::new()
        };
        let labels: Vec<String> = labels_str.split(':').map(str::to_string).collect();
        Some((decompressed, num, labels))
    }

    /// Parse script from base64-encoded LZMA data (the "normal" format from C# `parseNormal`).
    pub fn parse_normal(data: &str) -> Option<(HashMap<String, PFunction>, Vec<String>)> {
        let (decompressed, num, labels) = Self::decode_normal_payload(data)?;

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
                let lbl = labels[i].as_str();
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

    fn parse_label_until(context: &str, start: usize, delimiter: char) -> Option<(&str, usize)> {
        let rest = context.get(start..)?;
        let end = rest.find(delimiter)?;
        Some((&rest[..end], start + end + delimiter.len_utf8()))
    }

    fn push_text_action(
        functions: &mut HashMap<String, PFunction>,
        current_func: &str,
        action_type: ActionType,
    ) {
        if let Some(f) = functions.get_mut(current_func) {
            f.actions.push(PAction {
                action_type,
                label: String::new(),
                num: 0,
            });
        }
    }

    fn push_text_label_action(
        functions: &mut HashMap<String, PFunction>,
        current_func: &str,
        action_type: ActionType,
        label: &str,
    ) {
        if let Some(f) = functions.get_mut(current_func) {
            f.actions.push(PAction {
                action_type,
                label: label.to_string(),
                num: 0,
            });
        }
    }

    fn push_text_state_action(
        functions: &mut HashMap<String, PFunction>,
        current_func: &str,
        action_type: ActionType,
        label: &str,
        num: i32,
    ) {
        if let Some(f) = functions.get_mut(current_func) {
            f.actions.push(PAction {
                action_type,
                label: label.to_string(),
                num,
            });
        }
    }

    /// Parse current Unity text format from `ProgrammerView.SaveToStringNew()`.
    #[allow(clippy::too_many_lines)]
    pub fn parse_text(data: &str) -> Option<(HashMap<String, PFunction>, Vec<String>)> {
        let context = data.strip_prefix('$')?;
        let mut functions: HashMap<String, PFunction> = HashMap::new();
        let mut function_order = vec![String::new()];
        functions.insert(String::new(), PFunction::new());
        let mut current_func = String::new();
        let mut i = 0;

        while i < context.len() {
            let current = context.get(i..)?;
            if current.starts_with("CCW;") {
                Self::push_text_action(
                    &mut functions,
                    &current_func,
                    ActionType::RotateLeftRelative,
                );
                i += 4;
            } else if current.starts_with("CW;") {
                Self::push_text_action(
                    &mut functions,
                    &current_func,
                    ActionType::RotateRightRelative,
                );
                i += 3;
            } else if current.starts_with("RAND;") {
                Self::push_text_action(&mut functions, &current_func, ActionType::RotateRandom);
                i += 5;
            } else if current.starts_with("DIGG;") {
                Self::push_text_action(&mut functions, &current_func, ActionType::MacrosDig);
                i += 5;
            } else if current.starts_with("BUILD;") {
                Self::push_text_action(&mut functions, &current_func, ActionType::MacrosBuild);
                i += 6;
            } else if current.starts_with("HEAL;") {
                Self::push_text_action(&mut functions, &current_func, ActionType::MacrosHeal);
                i += 5;
            } else if current.starts_with("MINE;") {
                Self::push_text_action(&mut functions, &current_func, ActionType::MacrosMine);
                i += 5;
            } else if current.starts_with("FLIP;") {
                Self::push_text_action(&mut functions, &current_func, ActionType::Flip);
                i += 5;
            } else if current.starts_with("BEEP;") {
                Self::push_text_action(&mut functions, &current_func, ActionType::Beep);
                i += 5;
            } else if current.starts_with("AUT+") {
                Self::push_text_action(&mut functions, &current_func, ActionType::EnableAutoDig);
                i += 4;
            } else if current.starts_with("AUT-") {
                Self::push_text_action(&mut functions, &current_func, ActionType::DisableAutoDig);
                i += 4;
            } else if current.starts_with("AGR+") || current.starts_with("ARG+") {
                Self::push_text_action(&mut functions, &current_func, ActionType::EnableAgression);
                i += 4;
            } else if current.starts_with("AGR-") || current.starts_with("ARG-") {
                Self::push_text_action(&mut functions, &current_func, ActionType::DisableAgression);
                i += 4;
            } else if current.starts_with("=hp50") {
                Self::push_text_action(&mut functions, &current_func, ActionType::IsHpLower50);
                i += 5;
            } else if current.starts_with("=hp-") {
                Self::push_text_action(&mut functions, &current_func, ActionType::IsHpLower100);
                i += 4;
            } else if current.starts_with("B1;") {
                Self::push_text_action(&mut functions, &current_func, ActionType::BuildBlock);
                i += 3;
            } else if current.starts_with("B2;") {
                Self::push_text_action(&mut functions, &current_func, ActionType::BuildPillar);
                i += 3;
            } else if current.starts_with("B3;") {
                Self::push_text_action(&mut functions, &current_func, ActionType::BuildRoad);
                i += 3;
            } else if current.starts_with("VB;") {
                Self::push_text_action(
                    &mut functions,
                    &current_func,
                    ActionType::BuildMilitaryBlock,
                );
                i += 3;
            } else if current.starts_with("GEO;") {
                Self::push_text_action(&mut functions, &current_func, ActionType::OnlineGeo);
                i += 4;
            } else if current.starts_with("ZZ;") {
                Self::push_text_action(&mut functions, &current_func, ActionType::OnlineZz);
                i += 3;
            } else if current.starts_with("C190;") {
                Self::push_text_action(&mut functions, &current_func, ActionType::OnlineC190);
                i += 5;
            } else if current.starts_with("POLY;") {
                Self::push_text_action(&mut functions, &current_func, ActionType::OnlinePoly);
                i += 5;
            } else if current.starts_with("UP;") {
                Self::push_text_action(&mut functions, &current_func, ActionType::OnlineUp);
                i += 3;
            } else if current.starts_with("CRAFT;") {
                Self::push_text_action(&mut functions, &current_func, ActionType::OnlineCraft);
                i += 6;
            } else if current.starts_with("NANO;") {
                Self::push_text_action(&mut functions, &current_func, ActionType::OnlineNano);
                i += 5;
            } else if current.starts_with("REM;") {
                Self::push_text_action(&mut functions, &current_func, ActionType::OnlineRem);
                i += 4;
            } else if current.starts_with("iw") {
                Self::push_text_action(&mut functions, &current_func, ActionType::InventoryUp);
                i += 2;
            } else if current.starts_with("ia") {
                Self::push_text_action(&mut functions, &current_func, ActionType::InventoryLeft);
                i += 2;
            } else if current.starts_with("is") {
                Self::push_text_action(&mut functions, &current_func, ActionType::InventoryDown);
                i += 2;
            } else if current.starts_with("id") {
                Self::push_text_action(&mut functions, &current_func, ActionType::InventoryRight);
                i += 2;
            } else if current.starts_with("Hand+") {
                Self::push_text_action(&mut functions, &current_func, ActionType::HandModeOn);
                i += 5;
            } else if current.starts_with("Hand-") {
                Self::push_text_action(&mut functions, &current_func, ActionType::HandModeOff);
                i += 5;
            } else if current.starts_with("RESTART;") {
                Self::push_text_action(&mut functions, &current_func, ActionType::RestartRow);
                i += 8;
            } else if current.starts_with("OR") {
                Self::push_text_action(&mut functions, &current_func, ActionType::Or);
                i += 2;
            } else if current.starts_with("AND") {
                Self::push_text_action(&mut functions, &current_func, ActionType::And);
                i += 3;
            } else if let Some(ch) = current.chars().next() {
                match ch {
                    'w' => {
                        Self::push_text_action(&mut functions, &current_func, ActionType::RotateUp);
                    }
                    'a' => Self::push_text_action(
                        &mut functions,
                        &current_func,
                        ActionType::RotateLeft,
                    ),
                    's' => Self::push_text_action(
                        &mut functions,
                        &current_func,
                        ActionType::RotateDown,
                    ),
                    'd' => Self::push_text_action(
                        &mut functions,
                        &current_func,
                        ActionType::RotateRight,
                    ),
                    'z' => {
                        Self::push_text_action(&mut functions, &current_func, ActionType::Dig);
                    }
                    'b' => Self::push_text_action(
                        &mut functions,
                        &current_func,
                        ActionType::BuildBlock,
                    ),
                    'q' => Self::push_text_action(
                        &mut functions,
                        &current_func,
                        ActionType::BuildPillar,
                    ),
                    'r' => {
                        Self::push_text_action(
                            &mut functions,
                            &current_func,
                            ActionType::BuildRoad,
                        );
                    }
                    'g' => {
                        Self::push_text_action(&mut functions, &current_func, ActionType::Geology);
                    }
                    'h' => Self::push_text_action(&mut functions, &current_func, ActionType::Heal),
                    ',' => {
                        Self::push_text_action(&mut functions, &current_func, ActionType::NextRow);
                    }
                    '?' => {
                        if let Some((label, next)) =
                            Self::parse_label_until(context, i + ch.len_utf8(), '<')
                        {
                            Self::push_text_label_action(
                                &mut functions,
                                &current_func,
                                ActionType::RunIfFalse,
                                label,
                            );
                            i = next;
                            continue;
                        }
                    }
                    '(' => {
                        if let Some((expr, next)) =
                            Self::parse_label_until(context, i + ch.len_utf8(), ')')
                        {
                            if let Some((label, num)) = expr.split_once('=') {
                                if let Ok(num) = num.parse::<i32>() {
                                    Self::push_text_state_action(
                                        &mut functions,
                                        &current_func,
                                        ActionType::WritableState,
                                        label,
                                        num,
                                    );
                                }
                            } else if let Some((label, num)) = expr.split_once('<') {
                                if let Ok(num) = num.parse::<i32>() {
                                    Self::push_text_state_action(
                                        &mut functions,
                                        &current_func,
                                        ActionType::WritableStateLower,
                                        label,
                                        num,
                                    );
                                }
                            } else if let Some((label, num)) = expr.split_once('>')
                                && let Ok(num) = num.parse::<i32>()
                            {
                                Self::push_text_state_action(
                                    &mut functions,
                                    &current_func,
                                    ActionType::WritableStateMore,
                                    label,
                                    num,
                                );
                            }
                            i = next;
                            continue;
                        }
                    }
                    '!' => {
                        let after_bang = i + ch.len_utf8();
                        if context
                            .get(after_bang..)
                            .is_some_and(|s| s.starts_with('?'))
                        {
                            if let Some((label, next)) =
                                Self::parse_label_until(context, after_bang + '?'.len_utf8(), '<')
                            {
                                Self::push_text_label_action(
                                    &mut functions,
                                    &current_func,
                                    ActionType::RunIfTrue,
                                    label,
                                );
                                i = next;
                                continue;
                            }
                        } else if context
                            .get(after_bang..)
                            .is_some_and(|s| s.starts_with('{'))
                            && let Some((label, next)) =
                                Self::parse_label_until(context, after_bang + '{'.len_utf8(), '}')
                        {
                            Self::push_text_label_action(
                                &mut functions,
                                &current_func,
                                ActionType::DebugMessage,
                                label,
                            );
                            i = next;
                            continue;
                        }
                    }
                    '[' => {
                        if let Some((option, next)) =
                            Self::parse_label_until(context, i + ch.len_utf8(), ']')
                        {
                            let action = match option {
                                "W" => Some(ActionType::CheckUp),
                                "A" => Some(ActionType::CheckLeft),
                                "S" => Some(ActionType::CheckDown),
                                "D" => Some(ActionType::CheckRight),
                                "w" => Some(ActionType::ShiftUp),
                                "a" => Some(ActionType::ShiftLeft),
                                "s" => Some(ActionType::ShiftDown),
                                "d" => Some(ActionType::ShiftRight),
                                "AS" => Some(ActionType::CheckDownLeft),
                                "WA" => Some(ActionType::CheckUpLeft),
                                "DW" => Some(ActionType::CheckUpRight),
                                "SD" => Some(ActionType::CheckDownRight),
                                "F" => Some(ActionType::CheckForward),
                                "f" => Some(ActionType::ShiftForward),
                                "r" => Some(ActionType::CheckRightRelative),
                                "l" => Some(ActionType::CheckLeftRelative),
                                _ => None,
                            };
                            if let Some(action) = action {
                                Self::push_text_action(&mut functions, &current_func, action);
                            }
                            i = next;
                            continue;
                        }
                    }
                    '#' => {
                        let after_hash = i + ch.len_utf8();
                        if context
                            .get(after_hash..)
                            .is_some_and(|s| s.starts_with('S'))
                        {
                            Self::push_text_action(&mut functions, &current_func, ActionType::Stop);
                            i = after_hash + 'S'.len_utf8();
                            continue;
                        }
                        if context
                            .get(after_hash..)
                            .is_some_and(|s| s.starts_with('E'))
                        {
                            Self::push_text_action(
                                &mut functions,
                                &current_func,
                                ActionType::Start,
                            );
                            i = after_hash + 'E'.len_utf8();
                            continue;
                        }
                        if context
                            .get(after_hash..)
                            .is_some_and(|s| s.starts_with('R'))
                            && let Some((label, next)) =
                                Self::parse_label_until(context, after_hash + 'R'.len_utf8(), '<')
                        {
                            Self::push_text_label_action(
                                &mut functions,
                                &current_func,
                                ActionType::RunOnRespawn,
                                label,
                            );
                            i = next;
                            continue;
                        }
                    }
                    ':' => {
                        let after_colon = i + ch.len_utf8();
                        if context
                            .get(after_colon..)
                            .is_some_and(|s| s.starts_with('>'))
                            && let Some((label, next)) =
                                Self::parse_label_until(context, after_colon + '>'.len_utf8(), '>')
                        {
                            Self::push_text_label_action(
                                &mut functions,
                                &current_func,
                                ActionType::RunSub,
                                label,
                            );
                            i = next;
                            continue;
                        }
                    }
                    '-' => {
                        let after_dash = i + ch.len_utf8();
                        if context
                            .get(after_dash..)
                            .is_some_and(|s| s.starts_with('>'))
                            && let Some((label, next)) =
                                Self::parse_label_until(context, after_dash + '>'.len_utf8(), '>')
                        {
                            Self::push_text_label_action(
                                &mut functions,
                                &current_func,
                                ActionType::RunFunction,
                                label,
                            );
                            i = next;
                            continue;
                        }
                    }
                    '=' => {
                        let after_eq = i + ch.len_utf8();
                        if context.get(after_eq..).is_some_and(|s| s.starts_with('>'))
                            && let Some((label, next)) =
                                Self::parse_label_until(context, after_eq + '>'.len_utf8(), '>')
                        {
                            Self::push_text_label_action(
                                &mut functions,
                                &current_func,
                                ActionType::RunState,
                                label,
                            );
                            i = next;
                            continue;
                        }
                        if let Some(kind) = context.get(after_eq..).and_then(|s| s.chars().next()) {
                            let action = match kind {
                                'n' => Some(ActionType::IsNotEmpty),
                                'e' => Some(ActionType::IsEmpty),
                                'f' => Some(ActionType::IsFalling),
                                'c' => Some(ActionType::IsCrystal),
                                'a' => Some(ActionType::IsLivingCrystal),
                                'b' => Some(ActionType::IsBoulder),
                                's' => Some(ActionType::IsSand),
                                'k' => Some(ActionType::IsBreakableRock),
                                'd' => Some(ActionType::IsUnbreakable),
                                'A' => Some(ActionType::IsAcid),
                                'B' => Some(ActionType::IsRedRock),
                                'K' => Some(ActionType::IsBlackRock),
                                'g' => Some(ActionType::IsGreenBlock),
                                'y' => Some(ActionType::IsYellowBlock),
                                'r' => Some(ActionType::IsRedBlock),
                                'o' => Some(ActionType::IsPillar),
                                'q' => Some(ActionType::IsQuadBlock),
                                'R' => Some(ActionType::IsRoad),
                                'x' => Some(ActionType::IsBox),
                                'G' => Some(ActionType::CheckGun),
                                _ => None,
                            };
                            if let Some(action) = action {
                                Self::push_text_action(&mut functions, &current_func, action);
                                i = after_eq + kind.len_utf8();
                                continue;
                            }
                        }
                    }
                    '>' => {
                        if let Some((label, next)) =
                            Self::parse_label_until(context, i + ch.len_utf8(), '|')
                        {
                            Self::push_text_label_action(
                                &mut functions,
                                &current_func,
                                ActionType::GoTo,
                                label,
                            );
                            i = next;
                            continue;
                        }
                    }
                    '|' => {
                        if let Some((label, next)) =
                            Self::parse_label_until(context, i + ch.len_utf8(), ':')
                        {
                            current_func = label.to_string();
                            if !functions.contains_key(&current_func) {
                                functions.insert(current_func.clone(), PFunction::new());
                                function_order.push(current_func.clone());
                            }
                            i = next;
                            continue;
                        }
                    }
                    '<' => {
                        let after_lt = i + ch.len_utf8();
                        if context.get(after_lt..).is_some_and(|s| s.starts_with('|')) {
                            Self::push_text_action(
                                &mut functions,
                                &current_func,
                                ActionType::Return,
                            );
                            i = after_lt + '|'.len_utf8();
                            continue;
                        }
                        if context.get(after_lt..).is_some_and(|s| s.starts_with("-|")) {
                            Self::push_text_action(
                                &mut functions,
                                &current_func,
                                ActionType::ReturnFunction,
                            );
                            i = after_lt + "-|".len();
                            continue;
                        }
                        if context.get(after_lt..).is_some_and(|s| s.starts_with("=|")) {
                            Self::push_text_action(
                                &mut functions,
                                &current_func,
                                ActionType::ReturnState,
                            );
                            i = after_lt + "=|".len();
                            continue;
                        }
                    }
                    '^' => {
                        let after_caret = i + ch.len_utf8();
                        if let Some(kind) =
                            context.get(after_caret..).and_then(|s| s.chars().next())
                        {
                            let action = match kind {
                                'W' => Some(ActionType::MoveUp),
                                'A' => Some(ActionType::MoveLeft),
                                'S' => Some(ActionType::MoveDown),
                                'D' => Some(ActionType::MoveRight),
                                'F' => Some(ActionType::MoveForward),
                                _ => None,
                            };
                            if let Some(action) = action {
                                Self::push_text_action(&mut functions, &current_func, action);
                                i = after_caret + kind.len_utf8();
                                continue;
                            }
                        }
                    }
                    '{' => {
                        if let Some((label, next)) =
                            Self::parse_label_until(context, i + ch.len_utf8(), '}')
                        {
                            Self::push_text_label_action(
                                &mut functions,
                                &current_func,
                                ActionType::DebugPause,
                                label,
                            );
                            i = next;
                            continue;
                        }
                    }
                    _ => {}
                }
                i += ch.len_utf8();
            } else {
                break;
            }
        }

        Some((functions, function_order))
    }

    /// Start running a program (equivalent to C# `ProgrammatorData.Run(Program p)`).
    pub fn run_program(&mut self, data: &str) -> bool {
        self.running = false;
        self.current_prog.clear();
        self.function_order.clear();
        self.current_function.clear();
        let parsed = if data.starts_with('$') {
            Self::parse_text(data)
        } else {
            Self::parse_normal(data)
        };
        if let Some((functions, order)) = parsed {
            let total_actions: usize = functions.values().map(|f| f.actions.len()).sum();
            tracing::info!(
                "PROGDIAG run_program: parse OK funcs={} order={} actions={total_actions}",
                functions.len(),
                order.len()
            );
            // Дамп распарсенной последовательности — видно, ВЕРНЫЕ ли действия
            // произвёл парсер (move/dig/if/...) или мусор.
            for (fname, f) in &functions {
                let seq: Vec<String> = f
                    .actions
                    .iter()
                    .map(|a| format!("{:?}:{}", a.action_type, a.num))
                    .collect();
                tracing::info!("PROGDIAG parsed fn={fname:?} actions={seq:?}");
            }
            self.current_prog = functions;
            self.function_order = order;
            self.delay = Instant::now();
            self.drop_state();
            self.running = true;
            true
        } else {
            tracing::warn!(
                "PROGDIAG run_program: parse_normal FAILED data_len={}",
                data.len()
            );
            self.drop_state();
            false
        }
    }

    pub fn stop_program(&mut self) {
        self.running = false;
        self.drop_state();
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
        self.hand_mode_active = false;
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
            // None и прочее → прямое значение result.
            _ => f.state = Some(result),
        }
    }
}

// ─── Main ECS system ─────────────────────────────────────────────────────────

type ProgrammatorQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static PlayerMetadata,
        &'static PlayerPosition,
        &'static PlayerConnection,
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
    mut prog_q: ResMut<ProgrammatorQueue>,
    mut query: ProgrammatorQuery<'_, '_>,
) {
    let now = Instant::now();

    for (meta, pos, conn, stats, skills, settings, mut flags, mut prog, geo) in &mut query {
        if !prog.running {
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

        tracing::info!(
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
        if let Some(delay) = delay {
            prog.delay = now + delay;
        }
        flags.dirty = true;
    }
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
    conn: &PlayerConnection,
    prog_q: &mut ProgrammatorQueue,
    delay: &mut Option<Duration>,
    geo_count: usize,
) -> ExecResult {
    // C# Player.OnRoad: is_road клетки под игроком (для ServerPause road-бонуса).
    let on_road = crate::world::cells::is_road(world.get_cell(pos.x, pos.y));
    match action.action_type {
        // ─── Movement ────────────────────────────────────────────────────
        // dir = -1 (позиционный ход, 1:1 C# `Move(x,y)` дефолт). `handle_move`
        // выводит поворот из дельты И достигает ветки автокопы (`movement.rs:129`:
        // `dir == -1 && auto_dig`). С явным dir 0-3 автокопа в программе НЕ работала.
        // Повороты (Rotate*) ниже остаются с явным dir — у них нулевая дельта.
        ActionType::MoveDown => {
            *delay = Some(delay_millis(
                speed_pause(skills, on_road) + move_block_penalty(world, pos.x, pos.y + 1),
            ));
            push_move(prog_q, meta, conn, pos.x, pos.y + 1, -1);
            ExecResult::None
        }
        ActionType::MoveUp => {
            *delay = Some(delay_millis(
                speed_pause(skills, on_road) + move_block_penalty(world, pos.x, pos.y - 1),
            ));
            push_move(prog_q, meta, conn, pos.x, pos.y - 1, -1);
            ExecResult::None
        }
        ActionType::MoveRight => {
            *delay = Some(delay_millis(
                speed_pause(skills, on_road) + move_block_penalty(world, pos.x + 1, pos.y),
            ));
            push_move(prog_q, meta, conn, pos.x + 1, pos.y, -1);
            ExecResult::None
        }
        ActionType::MoveLeft => {
            *delay = Some(delay_millis(
                speed_pause(skills, on_road) + move_block_penalty(world, pos.x - 1, pos.y),
            ));
            push_move(prog_q, meta, conn, pos.x - 1, pos.y, -1);
            ExecResult::None
        }
        ActionType::MoveForward => {
            let (dx, dy) = crate::game::direction::dir_offset(pos.dir);
            *delay = Some(delay_millis(
                speed_pause(skills, on_road) + move_block_penalty(world, pos.x + dx, pos.y + dy),
            ));
            push_move(prog_q, meta, conn, pos.x + dx, pos.y + dy, -1);
            ExecResult::None
        }

        // ─── Rotation ────────────────────────────────────────────────────
        ActionType::RotateDown => {
            *delay = Some(delay_millis(speed_pause(skills, on_road)));
            push_move(prog_q, meta, conn, pos.x, pos.y, 0);
            ExecResult::None
        }
        ActionType::RotateUp => {
            *delay = Some(delay_millis(speed_pause(skills, on_road)));
            push_move(prog_q, meta, conn, pos.x, pos.y, 2);
            ExecResult::None
        }
        ActionType::RotateLeft => {
            *delay = Some(delay_millis(speed_pause(skills, on_road)));
            push_move(prog_q, meta, conn, pos.x, pos.y, 1);
            ExecResult::None
        }
        ActionType::RotateRight => {
            *delay = Some(delay_millis(speed_pause(skills, on_road)));
            push_move(prog_q, meta, conn, pos.x, pos.y, 3);
            ExecResult::None
        }
        ActionType::RotateLeftRelative => {
            *delay = Some(delay_millis(speed_pause(skills, on_road)));
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
            *delay = Some(delay_millis(speed_pause(skills, on_road)));
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
            *delay = Some(delay_millis(speed_pause(skills, on_road)));
            let d = rand::random_range(0..4);
            push_move(prog_q, meta, conn, pos.x, pos.y, d);
            ExecResult::None
        }

        // ─── Dig / Build ─────────────────────────────────────────────────
        ActionType::Dig => {
            *delay = Some(ACTION_DELAY);
            prog_q.0.push(ProgrammatorAction::Dig {
                pid: meta.id,
                tx: conn.tx.clone(),
                dir: pos.dir,
            });
            ExecResult::None
        }
        // MacrosBuild (id 142) намеренно НЕ здесь: C# `PAction.Execute` не имеет
        // для него case → no-op (падает в `_ => None`). 1:1 с референсом.
        ActionType::BuildBlock => {
            *delay = Some(ACTION_DELAY);
            prog_q.0.push(ProgrammatorAction::Build {
                pid: meta.id,
                tx: conn.tx.clone(),
                dir: pos.dir,
                block_type: "G".to_string(),
            });
            ExecResult::None
        }
        ActionType::BuildPillar => {
            *delay = Some(ACTION_DELAY);
            prog_q.0.push(ProgrammatorAction::Build {
                pid: meta.id,
                tx: conn.tx.clone(),
                dir: pos.dir,
                block_type: "O".to_string(),
            });
            ExecResult::None
        }
        ActionType::BuildRoad => {
            *delay = Some(ACTION_DELAY);
            prog_q.0.push(ProgrammatorAction::Build {
                pid: meta.id,
                tx: conn.tx.clone(),
                dir: pos.dir,
                block_type: "R".to_string(),
            });
            ExecResult::None
        }
        ActionType::BuildMilitaryBlock => {
            *delay = Some(ACTION_DELAY);
            prog_q.0.push(ProgrammatorAction::Build {
                pid: meta.id,
                tx: conn.tx.clone(),
                dir: pos.dir,
                block_type: "V".to_string(),
            });
            ExecResult::None
        }
        ActionType::Geology => {
            *delay = Some(ACTION_DELAY);
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
            *delay = Some(ACTION_DELAY);
            ExecResult::None
        }
        ActionType::Stop => {
            prog.stop_program();
            prog_q.0.push(ProgrammatorAction::SetProgrammatorStatus {
                tx: conn.tx.clone(),
                running: false,
            });
            prog_q.0.push(ProgrammatorAction::SetHandMode {
                tx: conn.tx.clone(),
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
                w.cell_defs().get(w.get_cell(x, y)).cell_is_empty()
            });
            ExecResult::None
        }
        ActionType::IsNotEmpty => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                !w.cell_defs().get(w.get_cell(x, y)).cell_is_empty()
            });
            ExecResult::None
        }
        ActionType::IsCrystal => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                let cell = w.get_cell(x, y);
                crate::world::cells::is_crystal(cell)
            });
            ExecResult::None
        }
        ActionType::IsBoulder => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                w.cell_defs().get(w.get_cell(x, y)).nature.is_boulder
            });
            ExecResult::None
        }
        ActionType::IsSand => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                w.cell_defs().get(w.get_cell(x, y)).is_sand()
            });
            ExecResult::None
        }
        ActionType::IsBreakableRock => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                w.cell_defs().get(w.get_cell(x, y)).is_diggable()
            });
            ExecResult::None
        }
        ActionType::IsUnbreakable => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                let defs = w.cell_defs();
                let def = defs.get(w.get_cell(x, y));
                !def.cell_is_empty() && !def.is_diggable()
            });
            ExecResult::None
        }
        ActionType::IsFalling => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                let defs = w.cell_defs();
                let def = defs.get(w.get_cell(x, y));
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
                w.get_cell(x, y) == crate::world::cells::cell_type::BOX
            });
            ExecResult::None
        }
        ActionType::IsGreenBlock => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                w.get_cell(x, y) == crate::world::cells::cell_type::GREEN_BLOCK
            });
            ExecResult::None
        }
        ActionType::IsYellowBlock => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                w.get_cell(x, y) == crate::world::cells::cell_type::YELLOW_BLOCK
            });
            ExecResult::None
        }
        ActionType::IsRedBlock => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                w.get_cell(x, y) == crate::world::cells::cell_type::RED_BLOCK
            });
            ExecResult::None
        }
        ActionType::IsPillar => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                w.get_cell(x, y) == crate::world::cells::cell_type::SUPPORT
            });
            ExecResult::None
        }
        ActionType::IsQuadBlock => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                w.get_cell(x, y) == crate::world::cells::cell_type::QUAD_BLOCK
            });
            ExecResult::None
        }
        ActionType::IsRedRock => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                w.get_cell(x, y) == crate::world::cells::cell_type::RED_ROCK
            });
            ExecResult::None
        }
        ActionType::IsBlackRock => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                w.get_cell(x, y) == crate::world::cells::cell_type::BLACK_ROCK
            });
            ExecResult::None
        }
        ActionType::IsAcid => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                let c = w.get_cell(x, y);
                // C# IsAcid: GrayAcid(66), PurpleAcid(67), PassiveAcid(86),
                // LivingActiveAcid(95), CorrosiveActiveAcid(96), AcidRock(118)
                matches!(c, 66 | 67 | 86 | 95 | 96 | 118)
            });
            ExecResult::None
        }
        ActionType::IsSlime => {
            check_cell(&mut *prog, pos, world, |x, y, w| {
                let c = w.get_cell(x, y);
                // JS ref is_slime: YellowSlime(86), WhiteSlime(66), VioletSlime(67),
                // Pearl(68), WarSlime(82), AcidRock(118). Server cells: GRAY_ACID(66),
                // PURPLE_ACID(67), PEARL(68), PASSIVE_ACID(86), LIVING_ACTIVE_ACID(95),
                // CORROSIVE_ACTIVE_ACID(96), ACID_ROCK(118).
                matches!(c, 66 | 67 | 68 | 82 | 86 | 95 | 96 | 118)
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
            if let Some(f) = prog.current_prog.get_mut(&prog.current_function) {
                f.state = None;
            }
            if state_val == Some(false) {
                // Condition is false, don't jump
                ExecResult::None
            } else {
                ExecResult::Label(action.label.clone())
            }
        }
        ActionType::RunIfFalse => {
            let state_val = prog
                .current_prog
                .get(&prog.current_function)
                .and_then(|f| f.state);
            if let Some(f) = prog.current_prog.get_mut(&prog.current_function) {
                f.state = None;
            }
            if state_val == Some(true) {
                // Condition is true, don't jump
                ExecResult::None
            } else {
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
            // Send BB (beep sound) to player via protocol encoding
            let pkt = crate::protocol::u_packet("BB", &[]);
            let mut buf = bytes::BytesMut::with_capacity(pkt.wire_len());
            if pkt.encode(&mut buf).is_ok() {
                conn.send_or_log(buf.to_vec());
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
        ActionType::EnableAgression => {
            prog_q.0.push(ProgrammatorAction::SetAggression {
                pid: meta.id,
                tx: conn.tx.clone(),
                enabled: true,
            });
            ExecResult::None
        }
        ActionType::DisableAgression => {
            prog_q.0.push(ProgrammatorAction::SetAggression {
                pid: meta.id,
                tx: conn.tx.clone(),
                enabled: false,
            });
            ExecResult::None
        }
        ActionType::HandModeOn => {
            prog.hand_mode_active = true;
            prog_q.0.push(ProgrammatorAction::SetHandMode {
                tx: conn.tx.clone(),
                enabled: true,
            });
            ExecResult::None
        }
        ActionType::HandModeOff => {
            prog.hand_mode_active = false;
            prog_q.0.push(ProgrammatorAction::SetHandMode {
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
                let diggable = world.cell_defs().get(world.get_cell(tx, ty)).is_diggable();
                if diggable {
                    *delay = Some(ACTION_DELAY);
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
            // C# PAction.cs:122-131: требует Red-кристалл (`crys[Red] > 0`) перед Heal.
            // Red — индекс 2 (Green0 Blue1 Red2 Violet3 White4 Cyan5).
            if stats.crystals[2] > 0 && stats.health < stats.max_health {
                prog_q.0.push(ProgrammatorAction::Heal {
                    pid: meta.id,
                    tx: conn.tx.clone(),
                });
                *delay = Some(ACTION_DELAY);
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
                if crate::world::cells::is_crystal(world.get_cell(pos.x + dx, pos.y + dy)) {
                    *delay = Some(ACTION_DELAY);
                    prog_q.0.push(ProgrammatorAction::Dig {
                        pid: meta.id,
                        tx: conn.tx.clone(),
                        dir: pos.dir,
                    });
                    return ExecResult::BoolResult(true);
                }
            }
            // Скан 4 направлений. Первый кристалл: если смотрим на него — копаем
            // (и фиксируем template), иначе поворачиваемся к нему.
            for (dir_key, (dx, dy)) in DIRZ {
                if crate::world::cells::is_crystal(world.get_cell(pos.x + dx, pos.y + dy)) {
                    if pos.dir == dir_key {
                        *delay = Some(ACTION_DELAY);
                        prog.macros_template = Some(dir_key);
                        prog_q.0.push(ProgrammatorAction::Dig {
                            pid: meta.id,
                            tx: conn.tx.clone(),
                            dir: pos.dir,
                        });
                    } else {
                        *delay = Some(delay_millis(speed_pause(skills, on_road)));
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
            *delay = Some(ACTION_DELAY);
            prog_q.0.push(ProgrammatorAction::FillGun {
                pid: meta.id,
                tx: conn.tx.clone(),
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
                if crate::world::cells::is_crystal(world.get_cell(cx, cy)) {
                    if pos.dir == check_dir {
                        *delay = Some(ACTION_DELAY);
                        prog_q.0.push(ProgrammatorAction::Dig {
                            pid: meta.id,
                            tx: conn.tx.clone(),
                            dir: pos.dir,
                        });
                    } else {
                        *delay = Some(delay_millis(speed_pause(skills, on_road)));
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
        | ActionType::WritableStateMore => {
            // C# PAction.CallWSAction: "del"→задержка (null, без state);
            // "geo"→сравнение geo.Count с num; прочее→false.
            let res: Option<bool> = if action.label.eq_ignore_ascii_case("del") {
                *delay = Some(delay_millis(u64::try_from(action.num).unwrap_or(0)));
                None
            } else if action.label.eq_ignore_ascii_case("geo") {
                let count = i32::try_from(geo_count).unwrap_or(i32::MAX);
                Some(match action.action_type {
                    ActionType::WritableStateLower => count < action.num,
                    ActionType::WritableStateMore => count > action.num,
                    _ => count == action.num,
                })
            } else if action.label.eq_ignore_ascii_case("aut") {
                let val = i32::from(settings.auto_dig);
                Some(match action.action_type {
                    ActionType::WritableStateLower => val < action.num,
                    ActionType::WritableStateMore => val > action.num,
                    _ => val == action.num,
                })
            } else if action.label.eq_ignore_ascii_case("agr") {
                let val = i32::from(settings.aggression);
                Some(match action.action_type {
                    ActionType::WritableStateLower => val < action.num,
                    ActionType::WritableStateMore => val > action.num,
                    _ => val == action.num,
                })
            } else if action.label.eq_ignore_ascii_case("hnd") {
                let val = i32::from(prog.hand_mode_active);
                Some(match action.action_type {
                    ActionType::WritableStateLower => val < action.num,
                    ActionType::WritableStateMore => val > action.num,
                    _ => val == action.num,
                })
            } else {
                Some(false)
            };
            // C# Execute: if (res != null) { Check(p, (_,_) => res); return res; }
            // Check пишет father.state с учётом And/Or — иначе RunIfTrue/False ломаются.
            if let Some(r) = res {
                check_cell(prog, pos, world, |_, _, _| r);
                return ExecResult::BoolResult(r);
            }
            ExecResult::None
        }

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
/// C# `Player.Move` возвращает `true` (→ `delay += 200ms`) когда ход заблокирован
/// непустой клеткой (ветка `dir==-1`, PAction.cs:143-176). Это чистый read-only
/// предикат от состояния целевой клетки — мутацию делает очередь хода, дублировать
/// её не нужно. Клетка ворот (тип 30) пустая → штрафа нет (как и C# `return false`).
fn move_block_penalty(world: &crate::world::World, tx: i32, ty: i32) -> u64 {
    if world.valid_coord(tx, ty) && !world.is_empty(tx, ty) {
        200 //TODO: что за 200?
    } else {
        0
    }
}

fn speed_pause(skills: &PlayerSkillsComp, on_road: bool) -> u64 {
    let move_effect = get_player_skill_effect(&skills.states, SkillType::Movement);
    // 1:1 ref Player.cs:155: ServerPause = (OnRoad ? pause*5*0.80 : pause*5) * 1.4 / 1000.
    // pause = move_effect * 100. move_effect — f32 из get_player_skill_effect
    // (1:1 с C#, нельзя в int без потери паритета); каст намеренный,
    // move_effect ≥ 0. Та же конвенция, что skills.rs.
    let pause_units = (move_effect * 100.0).to_u64().unwrap_or(0);
    // off-road: pause*5*1.4 = pause*7; on-road (×0.80): pause*5.6 = pause*56/10000.
    let server_pause_ms = if on_road {
        pause_units * 56 / 10000 //TODO: что за числа? лол
    } else {
        pause_units * 7 / 1000
    };
    // Minimum 20ms to prevent infinite loops / CPU stall (намеренная девиация: C# без пола)
    server_pause_ms.max(20) //TODO: почему именно 20 мс? у нас +есть своя система задержок schedule
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::SkillSlots;
    use std::collections::HashMap;

    fn empty_skills() -> PlayerSkillsComp {
        PlayerSkillsComp {
            states: SkillSlots {
                skills: HashMap::new(),
                total_slots: 20,
            },
        }
    }

    #[test]
    fn speed_pause_road_bonus_is_faster() {
        // C# Player.cs:155: на дороге ServerPause ×0.80 → меньше пауза.
        // Movement lvl0 effect=70 → pause_units=7000; off=49ms, on=39ms.
        let skills = empty_skills();
        let off = speed_pause(&skills, false);
        let on = speed_pause(&skills, true);
        assert_eq!(off, 49);
        assert_eq!(on, 39);
        assert!(on < off, "on-road должно быть быстрее off-road");
    }

    #[test]
    fn decode_prog_packet_rejects_truncated_compiled_block() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&10_i32.to_le_bytes());
        payload.extend_from_slice(&42_i32.to_le_bytes());
        payload.extend_from_slice(&[1, 2, 3]);

        assert!(ProgrammatorState::decode_prog_packet(&payload).is_none());
    }

    #[test]
    fn decode_prog_packet_accepts_empty_source_when_compiled_block_is_complete() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&3_i32.to_le_bytes());
        payload.extend_from_slice(&42_i32.to_le_bytes());
        payload.extend_from_slice(&[1, 2, 3]);

        assert_eq!(
            ProgrammatorState::decode_prog_packet(&payload),
            Some((42, String::new()))
        );
    }

    #[test]
    fn parse_text_format_maps_basic_programmator_actions() {
        let (functions, order) = ProgrammatorState::parse_text("$zghAGR+AGR-^W").unwrap();
        assert_eq!(order, vec![String::new()]);
        let actions: Vec<ActionType> = functions[""]
            .actions
            .iter()
            .map(|a| a.action_type)
            .collect();

        assert_eq!(
            actions,
            vec![
                ActionType::Dig,
                ActionType::Geology,
                ActionType::Heal,
                ActionType::EnableAgression,
                ActionType::DisableAgression,
                ActionType::MoveUp
            ]
        );
    }

    #[test]
    fn unity_hand_mode_bytecodes_map_to_hand_mode_actions() {
        assert_eq!(get_action_type(179), ActionType::HandModeOn);
        assert_eq!(get_action_type(180), ActionType::HandModeOff);
        assert_eq!(get_action_type(162), ActionType::BuildBlock);
        assert_eq!(get_action_type(163), ActionType::BuildPillar);
        assert_eq!(get_action_type(164), ActionType::BuildRoad);
        assert_eq!(get_action_type(165), ActionType::BuildMilitaryBlock);
    }

    #[test]
    fn unity_programmator_extension_bytecodes_are_named() {
        assert_eq!(get_action_type(167), ActionType::OnlineGeo);
        assert_eq!(get_action_type(168), ActionType::OnlineZz);
        assert_eq!(get_action_type(169), ActionType::OnlineC190);
        assert_eq!(get_action_type(170), ActionType::OnlinePoly);
        assert_eq!(get_action_type(171), ActionType::OnlineUp);
        assert_eq!(get_action_type(172), ActionType::OnlineCraft);
        assert_eq!(get_action_type(173), ActionType::OnlineNano);
        assert_eq!(get_action_type(174), ActionType::OnlineRem);
        assert_eq!(get_action_type(175), ActionType::InventoryUp);
        assert_eq!(get_action_type(176), ActionType::InventoryLeft);
        assert_eq!(get_action_type(177), ActionType::InventoryDown);
        assert_eq!(get_action_type(178), ActionType::InventoryRight);
        assert_eq!(get_action_type(181), ActionType::DebugMessage);
        assert_eq!(get_action_type(182), ActionType::DebugPause);
        assert_eq!(get_action_type(200), ActionType::RestartRow);
    }

    #[test]
    fn parse_text_format_maps_all_current_unity_extension_tokens() {
        let (functions, _) = ProgrammatorState::parse_text(
            "$B1;B2;B3;VB;GEO;ZZ;C190;POLY;UP;CRAFT;NANO;REM;iwiaisidHand+Hand-!{dbg}{pause}RESTART;",
        )
        .unwrap();
        let actions: Vec<ActionType> = functions[""]
            .actions
            .iter()
            .map(|a| a.action_type)
            .collect();

        assert_eq!(
            actions,
            vec![
                ActionType::BuildBlock,
                ActionType::BuildPillar,
                ActionType::BuildRoad,
                ActionType::BuildMilitaryBlock,
                ActionType::OnlineGeo,
                ActionType::OnlineZz,
                ActionType::OnlineC190,
                ActionType::OnlinePoly,
                ActionType::OnlineUp,
                ActionType::OnlineCraft,
                ActionType::OnlineNano,
                ActionType::OnlineRem,
                ActionType::InventoryUp,
                ActionType::InventoryLeft,
                ActionType::InventoryDown,
                ActionType::InventoryRight,
                ActionType::HandModeOn,
                ActionType::HandModeOff,
                ActionType::DebugMessage,
                ActionType::DebugPause,
                ActionType::RestartRow,
            ]
        );

        assert_eq!(functions[""].actions[18].label, "dbg");
        assert_eq!(functions[""].actions[19].label, "pause");
    }

    #[test]
    fn run_program_accepts_current_unity_text_format() {
        let mut state = ProgrammatorState::new();

        assert!(state.run_program("$z"));
        assert!(state.running);
        assert_eq!(
            state.current_prog[""].actions[0].action_type,
            ActionType::Dig
        );
    }

    #[test]
    fn programmator_snapshot_roundtrips_runtime_state() {
        let mut state = ProgrammatorState::new();
        assert!(state.run_program("$zg"));
        state.current_prog.get_mut("").unwrap().current = 1;
        state.shift_x = 2;
        state.check_y = -1;
        state.hand_mode_active = true;
        state.selected_id = Some(7);
        state.selected_data = Some("$zg".to_string());

        let encoded = serde_json::to_string(&state.snapshot()).unwrap();
        let snapshot = serde_json::from_str(&encoded).unwrap();
        let mut restored = ProgrammatorState::new();
        restored.restore_snapshot(snapshot);

        assert!(restored.running);
        assert_eq!(restored.current_prog[""].current, 1);
        assert_eq!(
            restored.current_prog[""].actions[1].action_type,
            ActionType::Geology
        );
        assert_eq!(restored.shift_x, 2);
        assert_eq!(restored.check_y, -1);
        assert!(restored.hand_mode_active);
        assert_eq!(restored.selected_id, Some(7));
        assert_eq!(restored.selected_data.as_deref(), Some("$zg"));
    }

    #[test]
    fn invalid_program_source_stops_previous_run() {
        let mut state = ProgrammatorState::new();
        state.running = true;
        state
            .current_prog
            .insert("stale".to_string(), PFunction::new());
        state.function_order.push("stale".to_string());
        state.current_function = "stale".to_string();

        assert!(!state.run_program("not valid base64/lzma"));
        assert!(!state.running);
        assert!(state.current_prog.is_empty());
        assert!(state.function_order.is_empty());
        assert!(state.current_function.is_empty());
    }
}
