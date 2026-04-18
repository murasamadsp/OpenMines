use bevy_ecs::prelude::Component;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum Command {
    Move(i32),
    Dig(i32),
    If(Condition, Vec<Self>, Vec<Self>),
    Loop(Vec<Self>),
    Call(String),
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum Condition {
    Empty(i32),
    CanMove(i32),
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct StackFrame {
    pub commands: Vec<Command>,
    pub pc: usize,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Component)]
pub struct ProgrammatorState {
    pub running: bool,
    pub main_commands: Vec<Command>,
    pub pc: usize,
    pub last_tick: Instant,
    pub delay: Duration,
    pub functions: HashMap<String, Vec<Command>>,
    pub stack: Vec<StackFrame>,
}

#[allow(dead_code)]
impl ProgrammatorState {
    pub fn new() -> Self {
        Self {
            running: false,
            main_commands: Vec::new(),
            pc: 0,
            last_tick: Instant::now(),
            delay: Duration::from_millis(150), // Default delay
            functions: HashMap::new(),
            stack: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.pc = 0;
        self.stack.clear();
        self.last_tick = Instant::now();
    }
}

#[allow(dead_code)]
pub struct Parser<'a> {
    tokens: Vec<&'a str>,
    pos: usize,
}

#[allow(dead_code)]
impl<'a> Parser<'a> {
    pub fn new(code: &'a str) -> Self {
        let tokens = code
            .split_whitespace()
            .flat_map(|s| {
                // Split symbols like { } from words
                let mut res = Vec::new();
                let mut start = 0;
                for (i, c) in s.char_indices() {
                    if c == '{' || c == '}' {
                        if start < i {
                            res.push(&s[start..i]);
                        }
                        res.push(&s[i..=i]);
                        start = i + 1;
                    }
                }
                if start < s.len() {
                    res.push(&s[start..]);
                }
                res
            })
            .collect();
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&'a str> {
        self.tokens.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<&'a str> {
        let res = self.peek();
        if res.is_some() {
            self.pos += 1;
        }
        res
    }

    pub fn parse(&mut self) -> (Vec<Command>, HashMap<String, Vec<Command>>) {
        let mut main = Vec::new();
        let mut functions = HashMap::new();

        while let Some(token) = self.next() {
            if token == "fn" {
                if let Some(name) = self.next() {
                    let body = self.parse_block();
                    functions.insert(name.to_string(), body);
                }
            } else {
                self.pos -= 1;
                if let Some(cmd) = self.parse_command() {
                    main.push(cmd);
                } else {
                    self.pos += 1; // skip unknown
                }
            }
        }

        (main, functions)
    }

    fn parse_block(&mut self) -> Vec<Command> {
        let mut block = Vec::new();
        if self.peek() == Some("{") {
            self.next();
            while let Some(token) = self.peek() {
                if token == "}" {
                    self.next();
                    break;
                }
                if let Some(cmd) = self.parse_command() {
                    block.push(cmd);
                } else {
                    self.next(); // skip
                }
            }
        }
        block
    }

    fn parse_command(&mut self) -> Option<Command> {
        let token = self.next()?;
        match token {
            "move" => {
                let dir = self.next()?.parse().ok()?;
                Some(Command::Move(dir))
            }
            "dig" => {
                let dir = self.next()?.parse().ok()?;
                Some(Command::Dig(dir))
            }
            "if" => {
                let cond_str = self.next()?;
                let cond_dir = self.next()?.parse().ok()?;
                let cond = match cond_str {
                    "empty" => Condition::Empty(cond_dir),
                    "can_move" => Condition::CanMove(cond_dir),
                    _ => return None,
                };
                let then_block = self.parse_block();
                let else_block = if self.peek() == Some("else") {
                    self.next();
                    self.parse_block()
                } else {
                    Vec::new()
                };
                Some(Command::If(cond, then_block, else_block))
            }
            "loop" => {
                let body = self.parse_block();
                Some(Command::Loop(body))
            }
            "{" | "}" => None,
            name => Some(Command::Call(name.to_string())),
        }
    }
}

use crate::game::{GameStateResource, PlayerComponent};
use crate::net::session::play::dig_build::handle_dig;
use crate::net::session::play::movement::handle_move;
use bevy_ecs::prelude::{Query, Res};

#[allow(clippy::needless_pass_by_value)]
pub fn programmator_system(
    state_res: Res<GameStateResource>,
    mut query: Query<(&PlayerComponent, &mut ProgrammatorState)>,
) {
    let state = &state_res.0;
    let now = Instant::now();

    for (player, mut prog) in &mut query {
        if !prog.running || prog.main_commands.is_empty() {
            continue;
        }

        if now.duration_since(prog.last_tick) < prog.delay {
            continue;
        }

        prog.last_tick = now;

        // basic execution of one command
        if prog.pc >= prog.main_commands.len() {
            prog.pc = 0; // auto-loop for now or stop
        }

        let cmd = prog.main_commands[prog.pc].clone();
        prog.pc += 1;

        match cmd {
            Command::Move(dir) => {
                let (px, py) = {
                    let p = state.active_players.get(&player.pid).unwrap();
                    (p.data.x, p.data.y)
                };
                let (dx, dy) = crate::game::direction::dir_offset(dir);
                // Call handle_move as if client sent it
                // We need tx, but we can get it from active_players
                if let Some(p) = state.active_players.get(&player.pid) {
                    handle_move(state, &p.tx, player.pid, px + dx, py + dy, dir);
                }
            }
            Command::Dig(dir) => {
                if let Some(p) = state.active_players.get(&player.pid) {
                    handle_dig(state, &p.tx, player.pid, dir);
                }
            }
            _ => {} // implement if/loop later
        }
    }
}
