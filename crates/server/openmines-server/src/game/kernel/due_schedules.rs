use crate::game::kernel::programmator::ProgrammatorDueSchedule;
use crate::game::kernel::schedule::{CraftingDueSchedule, HazardDueSchedule};
use crate::game::mechanics::building_damage::CraftingDue;
use crate::simulation_waker::SimulationWaker;
use bevy_ecs::prelude::Entity;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Instant;

pub struct DueSchedules {
    crafting: Mutex<CraftingDueSchedule>,
    programmator: Arc<Mutex<ProgrammatorDueSchedule>>,
    hazard: Arc<Mutex<HazardDueSchedule>>,
    waker: SimulationWaker,
}

impl DueSchedules {
    pub fn new(waker: SimulationWaker) -> Self {
        Self {
            crafting: Mutex::new(CraftingDueSchedule::default()),
            programmator: Arc::new(Mutex::new(ProgrammatorDueSchedule::default())),
            hazard: Arc::new(Mutex::new(HazardDueSchedule::default())),
            waker,
        }
    }

    pub fn schedule_crafting_completion(&self, entity: Entity, end_ts: i64) {
        let mut schedule = self.crafting.lock();
        schedule.schedule(entity, end_ts);
        let depth = schedule.len();
        drop(schedule);
        crate::metrics::CRAFTING_DUE_DEPTH.set(i64::try_from(depth).unwrap_or(i64::MAX));
        self.waker.wake();
    }

    pub fn next_crafting_due_ts(&self) -> Option<i64> {
        self.crafting.lock().next_due_ts()
    }
    pub fn has_due_crafting(&self, now_ts: i64) -> bool {
        self.crafting.lock().is_due(now_ts)
    }

    pub fn take_due_crafting(
        &self,
        now_ts: i64,
        batch_budget: usize,
    ) -> (Vec<CraftingDue>, bool, usize) {
        let mut schedule = self.crafting.lock();
        let due = schedule.pop_due(now_ts, batch_budget);
        let due_remaining = schedule.is_due(now_ts);
        let depth = schedule.len();
        drop(schedule);
        (due, due_remaining, depth)
    }

    pub fn schedule_programmator(&self, entity: Entity, due_at: Instant) {
        self.programmator.lock().schedule(entity, due_at);
        self.waker.wake();
    }
    pub fn next_programmator_due_at(&self) -> Option<Instant> {
        self.programmator.lock().next_due_at()
    }
    pub fn has_due_programmator(&self, now: Instant) -> bool {
        self.programmator.lock().is_due(now)
    }
    pub fn take_due_programmators(
        &self,
        now: Instant,
        batch_budget: usize,
    ) -> Vec<(Entity, Instant)> {
        self.programmator.lock().pop_due(now, batch_budget)
    }

    pub fn schedule_hazard(&self, entity: Entity, due_at: Instant) {
        self.hazard.lock().schedule(entity, due_at);
        self.waker.wake();
    }
    pub fn next_hazard_due_at(&self) -> Option<Instant> {
        self.hazard.lock().next_due_at()
    }
    pub fn take_due_hazards(&self, now: Instant, batch_budget: usize) -> Vec<(Entity, Instant)> {
        self.hazard.lock().pop_due(now, batch_budget)
    }

    pub fn programmator_schedule(&self) -> Arc<Mutex<ProgrammatorDueSchedule>> {
        self.programmator.clone()
    }
    pub fn hazard_schedule(&self) -> Arc<Mutex<HazardDueSchedule>> {
        self.hazard.clone()
    }
}
