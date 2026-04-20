use crate::game::player::{PlayerConnection, PlayerMetadata, PlayerPosition};
use bevy_ecs::prelude::Component;
use std::collections::HashMap;
use std::time::Instant;

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

#[derive(Component, Debug)]
pub struct ProgrammatorState {
    pub scripts: HashMap<String, Vec<Command>>,
    pub current_script: Option<String>,
    pub pc: usize,
    pub running: bool,
    pub last_tick: Instant,
}

impl ProgrammatorState {
    pub fn new() -> Self {
        Self {
            scripts: HashMap::new(),
            current_script: None,
            pc: 0,
            running: false,
            last_tick: Instant::now(),
        }
    }

    pub fn fetch_next(&mut self) -> Option<Command> {
        let name = self.current_script.as_ref()?;
        let script = self.scripts.get(name)?;
        if self.pc >= script.len() {
            return None;
        }
        let cmd = script[self.pc].clone();
        self.pc += 1;
        Some(cmd)
    }
}

pub struct Parser<'a> {
    tokens: Vec<&'a str>,
    pos: usize,
}

impl<'a> Parser<'a> {
    pub fn new(input: &'a str) -> Self {
        let tokens = input.split_whitespace().collect();
        Self { tokens, pos: 0 }
    }

    fn next(&mut self) -> Option<&'a str> {
        if self.pos < self.tokens.len() {
            let t = self.tokens[self.pos];
            self.pos += 1;
            Some(t)
        } else {
            None
        }
    }

    pub fn parse_script(&mut self) -> Vec<Command> {
        let mut script = Vec::new();
        while let Some(cmd) = self.parse_command() {
            script.push(cmd);
        }
        script
    }

    fn parse_command(&mut self) -> Option<Command> {
        let t = self.next()?;
        match t {
            "move" => {
                let dir = self.next()?.parse().ok()?;
                Some(Command::Move(dir))
            }
            "dig" => {
                let dir = self.next()?.parse().ok()?;
                Some(Command::Dig(dir))
            }
            "if" => {
                let cond_t = self.next()?;
                let cond_dir = self.next()?.parse().ok()?;
                let cond = match cond_t {
                    "empty" => Condition::Empty(cond_dir),
                    "can_move" => Condition::CanMove(cond_dir),
                    _ => return None,
                };
                let then_body = self.parse_block()?;
                let else_body = if self.tokens.get(self.pos) == Some(&"else") {
                    self.pos += 1;
                    self.parse_block()?
                } else {
                    Vec::new()
                };
                Some(Command::If(cond, then_body, else_body))
            }
            "loop" => {
                let body = self.parse_block()?;
                Some(Command::Loop(body))
            }
            "{" | "}" => None,
            name => Some(Command::Call(name.to_string())),
        }
    }

    fn parse_block(&mut self) -> Option<Vec<Command>> {
        if self.next() != Some("{") {
            return None;
        }
        let mut block = Vec::new();
        while self.tokens.get(self.pos) != Some(&"}") {
            if let Some(cmd) = self.parse_command() {
                block.push(cmd);
            } else {
                break;
            }
        }
        self.pos += 1; // skip }
        Some(block)
    }
}

use crate::game::{GameStateResource, ProgrammatorAction, ProgrammatorQueue};
use bevy_ecs::prelude::{Query, Res, ResMut};

#[allow(clippy::needless_pass_by_value)]
pub fn programmator_system(
    _state_res: Res<GameStateResource>,
    mut prog_q: ResMut<ProgrammatorQueue>,
    mut query: Query<(
        &PlayerMetadata,
        &PlayerPosition,
        &PlayerConnection,
        &mut ProgrammatorState,
    )>,
) {
    for (meta, pos, conn, mut prog) in &mut query {
        if prog.running {
            if let Some(cmd) = prog.fetch_next() {
                match cmd {
                    Command::Move(dir) => {
                        let (dx, dy) = crate::game::direction::dir_offset(dir);
                        prog_q.0.push(ProgrammatorAction::Move {
                            pid: meta.id,
                            tx: conn.tx.clone(),
                            x: pos.x + dx,
                            y: pos.y + dy,
                            dir,
                        });
                    }
                    Command::Dig(dir) => {
                        prog_q.0.push(ProgrammatorAction::Dig {
                            pid: meta.id,
                            tx: conn.tx.clone(),
                            dir,
                        });
                    }
                    _ => {}
                }
            } else {
                prog.running = false;
            }
        }
    }
}
