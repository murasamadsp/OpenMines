use bevy_ecs::prelude::{Entity, Resource};
use parking_lot::Mutex;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;
#[allow(unused_imports)]
use std::time::{Duration, Instant};

use super::super::{PlayerId, SessionId};

pub enum ProgrammatorAction {
    Move {
        pid: PlayerId,
        session_id: Option<SessionId>,
        x: i32,
        y: i32,
        dir: i32,
    },
    Dig {
        pid: PlayerId,
        session_id: Option<SessionId>,
        dir: i32,
    },
    Build {
        pid: PlayerId,
        session_id: Option<SessionId>,
        dir: i32,
        block_type: String,
    },
    Geo {
        pid: PlayerId,
        session_id: Option<SessionId>,
    },
    Heal {
        pid: PlayerId,
        session_id: Option<SessionId>,
    },
    SetAutoDig {
        pid: PlayerId,
        session_id: Option<SessionId>,
        enabled: bool,
    },
    SetAggression {
        pid: PlayerId,
        session_id: Option<SessionId>,
        enabled: bool,
    },
    SetHandMode {
        session_id: Option<SessionId>,
        enabled: bool,
    },
    FillGun {
        pid: PlayerId,
        session_id: Option<SessionId>,
        x: i32,
        y: i32,
    },
    SetProgrammatorStatus {
        session_id: Option<SessionId>,
        running: bool,
    },
    Send {
        session_id: SessionId,
        data: Vec<u8>,
    },
}

#[derive(Resource, Default)]
pub struct ProgrammatorQueue(pub Vec<ProgrammatorAction>);

#[derive(Resource, Clone)]
pub struct ProgrammatorDueQueue(Arc<Mutex<ProgrammatorDueSchedule>>);

impl ProgrammatorDueQueue {
    pub fn schedule(&self, entity: Entity, due_at: Instant) {
        self.0.lock().schedule(entity, due_at);
    }

    pub(crate) const fn new(schedule: Arc<Mutex<ProgrammatorDueSchedule>>) -> Self {
        Self(schedule)
    }
}

#[derive(Resource, Default)]
pub struct ProgrammatorDueBatch(pub Vec<(Entity, Instant)>);

#[derive(Default)]
pub struct ProgrammatorDueSchedule {
    due: BinaryHeap<Reverse<(Instant, Entity)>>,
    scheduled: HashMap<Entity, Instant>,
}

impl ProgrammatorDueSchedule {
    pub(crate) fn schedule(&mut self, entity: Entity, due_at: Instant) {
        self.scheduled.insert(entity, due_at);
        self.due.push(Reverse((due_at, entity)));
    }

    fn discard_stale_head(&mut self) {
        while let Some(&Reverse((due_at, entity))) = self.due.peek() {
            if self.scheduled.get(&entity) == Some(&due_at) {
                break;
            }
            self.due.pop();
        }
    }

    pub(crate) fn is_due(&mut self, now: Instant) -> bool {
        self.discard_stale_head();
        self.due
            .peek()
            .is_some_and(|Reverse((due_at, _))| *due_at <= now)
    }

    pub(crate) fn next_due_at(&mut self) -> Option<Instant> {
        self.discard_stale_head();
        self.due.peek().map(|Reverse((due_at, _))| *due_at)
    }

    pub(crate) fn pop_due(&mut self, now: Instant, limit: usize) -> Vec<(Entity, Instant)> {
        let mut due = Vec::with_capacity(limit.min(self.scheduled.len()));
        while due.len() < limit {
            self.discard_stale_head();
            let Some(&Reverse((due_at, entity))) = self.due.peek() else {
                break;
            };
            if due_at > now {
                break;
            }
            self.due.pop();
            if self.scheduled.remove(&entity) == Some(due_at) {
                due.push((entity, due_at));
            }
        }
        due
    }
}

#[cfg(test)]
mod programmator_due_schedule_tests {
    use super::*;

    #[test]
    fn keeps_only_the_latest_deadline_per_entity() {
        let base = Instant::now();
        let entity = Entity::from_raw_u32(1).expect("non-placeholder entity id");
        let mut schedule = ProgrammatorDueSchedule::default();

        for delay_ms in 10..=266 {
            schedule.schedule(entity, base + Duration::from_millis(delay_ms));
        }

        assert!(!schedule.is_due(base + Duration::from_millis(10)));
        assert_eq!(
            schedule.pop_due(base + Duration::from_millis(266), 256),
            vec![(entity, base + Duration::from_millis(266))]
        );
        assert!(schedule.next_due_at().is_none());
    }
}
