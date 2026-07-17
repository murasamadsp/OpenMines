use super::types::{ActionType, PAction, PFunction, ProgrammatorState, get_action_type};
use std::collections::HashMap;
use std::time::Instant;

impl ProgrammatorState {
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
                            Self::push_text_action(
                                &mut functions,
                                &current_func,
                                ActionType::Start,
                            );
                            i = after_hash + 'S'.len_utf8();
                            continue;
                        }
                        if context
                            .get(after_hash..)
                            .is_some_and(|s| s.starts_with('E'))
                        {
                            Self::push_text_action(&mut functions, &current_func, ActionType::Stop);
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
            self.current_prog = functions;
            self.function_order = order;
            self.delay = Instant::now();
            self.started_at = Instant::now();
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
}
