use bevy_ecs::prelude::{Entity, Resource, Schedule};
use parking_lot::{Mutex, RwLock};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::super::PlayerId;
use super::super::mechanics::{building_damage, combat};
use super::box_pickups::BoxPickupQueue;

pub struct GameSchedule {
    pub name: String,
    pub activity: ScheduleActivity,
    pub schedule: RwLock<Schedule>,
    pub interval_ms: std::sync::atomic::AtomicU64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScheduleActivity {
    Always,
    OnlinePlayers,
    DueCrafting,
    DueGuns,
    DueProgrammator,
    DueHazards,
    ActiveGranular,
    ActiveAlive,
}

#[derive(Clone, Copy, Debug)]
pub struct BotsRenderPlayer {
    pub x: i32,
    pub y: i32,
    pub dir: i32,
    pub skin: i32,
    pub clan_id: i32,
    pub tail: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BotsRenderDue {
    pub due_at: Instant,
    pub player_id: PlayerId,
    pub session_token: u64,
}

#[derive(Default)]
pub struct BotsRenderSchedule {
    due: BinaryHeap<Reverse<(Instant, PlayerId, u64)>>,
}

impl BotsRenderSchedule {
    pub(crate) fn schedule(&mut self, due: BotsRenderDue) {
        self.due
            .push(Reverse((due.due_at, due.player_id, due.session_token)));
    }

    pub(crate) fn pop_due(&mut self, now: Instant) -> Option<BotsRenderDue> {
        let Reverse((due_at, player_id, session_token)) = *self.due.peek()?;
        if due_at > now {
            return None;
        }
        self.due.pop();
        Some(BotsRenderDue {
            due_at,
            player_id,
            session_token,
        })
    }

    pub(crate) fn next_due_at(&self) -> Option<Instant> {
        self.due.peek().map(|Reverse((due_at, _, _))| *due_at)
    }
}

#[derive(Default)]
pub struct CraftingDueSchedule {
    due: BinaryHeap<Reverse<(i64, Entity)>>,
}

impl CraftingDueSchedule {
    pub(crate) fn schedule(&mut self, entity: Entity, end_ts: i64) {
        if end_ts > 0 {
            self.due.push(Reverse((end_ts, entity)));
        }
    }

    pub(crate) fn is_due(&self, now_ts: i64) -> bool {
        self.due
            .peek()
            .is_some_and(|Reverse((end_ts, _))| *end_ts <= now_ts)
    }

    pub(crate) fn next_due_ts(&self) -> Option<i64> {
        self.due.peek().map(|Reverse((end_ts, _))| *end_ts)
    }

    pub(crate) fn pop_due(
        &mut self,
        now_ts: i64,
        limit: usize,
    ) -> Vec<building_damage::CraftingDue> {
        let mut due = Vec::with_capacity(limit.min(self.due.len()));
        while due.len() < limit {
            let Some(&Reverse((end_ts, entity))) = self.due.peek() else {
                break;
            };
            if end_ts > now_ts {
                break;
            }
            self.due.pop();
            due.push(building_damage::CraftingDue { entity, end_ts });
        }
        due
    }

    pub(crate) fn len(&self) -> usize {
        self.due.len()
    }
}

#[derive(Resource, Clone)]
pub struct HazardDueQueue(Arc<Mutex<HazardDueSchedule>>);

impl HazardDueQueue {
    #[allow(dead_code)]
    pub fn schedule(&self, entity: Entity, due_at: Instant) {
        self.0.lock().schedule(entity, due_at);
    }

    #[allow(dead_code)]
    pub fn pop_due(&self, now: Instant, limit: usize) -> Vec<(Entity, Instant)> {
        self.0.lock().pop_due(now, limit)
    }

    pub(crate) const fn new(schedule: Arc<Mutex<HazardDueSchedule>>) -> Self {
        Self(schedule)
    }
}

#[derive(Resource, Default)]
pub struct HazardDueBatch(pub Vec<(Entity, Instant)>);

#[derive(Resource, Clone)]
pub struct StandingCellHazardContext {
    pub box_pickups: BoxPickupQueue,
    pub death_queue: combat::DeathQueue,
    pub due_queue: HazardDueQueue,
    pub interval: Duration,
    pub slow_threshold: Duration,
}

#[derive(Default)]
pub struct HazardDueSchedule {
    due: BinaryHeap<Reverse<(Instant, Entity)>>,
    scheduled: HashMap<Entity, Instant>,
}

impl HazardDueSchedule {
    pub(crate) fn schedule(&mut self, entity: Entity, due_at: Instant) {
        let Some(previous) = self.scheduled.insert(entity, due_at) else {
            self.due.push(Reverse((due_at, entity)));
            return;
        };
        if due_at < previous {
            self.due.push(Reverse((due_at, entity)));
        } else {
            self.scheduled.insert(entity, previous);
        }
    }

    fn discard_stale_head(&mut self) {
        while let Some(&Reverse((due_at, entity))) = self.due.peek() {
            if self
                .scheduled
                .get(&entity)
                .is_some_and(|current| *current == due_at)
            {
                break;
            }
            self.due.pop();
        }
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
            self.scheduled.remove(&entity);
            due.push((entity, due_at));
        }
        due
    }
}

#[cfg(test)]
mod hazard_due_schedule_tests {
    use super::*;

    #[test]
    fn keeps_one_earliest_deadline_per_entity() {
        let base = Instant::now();
        let entity = Entity::from_raw_u32(1).expect("non-placeholder entity id");
        let mut schedule = HazardDueSchedule::default();

        schedule.schedule(entity, base + Duration::from_millis(20));
        schedule.schedule(entity, base + Duration::from_millis(10));
        schedule.schedule(entity, base + Duration::from_millis(30));

        assert_eq!(
            schedule.next_due_at(),
            Some(base + Duration::from_millis(10))
        );
        assert_eq!(
            schedule.pop_due(base + Duration::from_millis(10), 1),
            vec![(entity, base + Duration::from_millis(10))]
        );
        assert!(schedule.next_due_at().is_none());
    }
}
