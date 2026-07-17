use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::super::WorldPos;

#[derive(Default)]
struct GranularWakeState {
    points: HashSet<WorldPos>,
    region_seeds: HashSet<WorldPos>,
}

#[derive(bevy_ecs::prelude::Resource, Clone)]
pub struct GranularWakeQueue {
    state: Arc<Mutex<GranularWakeState>>,
    active: Arc<AtomicBool>,
}

impl Default for GranularWakeQueue {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(GranularWakeState::default())),
            active: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl GranularWakeQueue {
    pub fn wake_neighborhood(&self, x: i32, y: i32) {
        self.state.lock().points.insert((x, y).into());
    }

    pub fn seed_region(&self, x: i32, y: i32) {
        self.state.lock().region_seeds.insert((x, y).into());
    }

    pub fn take(&self) -> (Vec<WorldPos>, Vec<WorldPos>) {
        let mut state = self.state.lock();
        (
            std::mem::take(&mut state.points).into_iter().collect(),
            std::mem::take(&mut state.region_seeds)
                .into_iter()
                .collect(),
        )
    }

    pub fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::Release);
    }

    pub fn has_work(&self) -> bool {
        self.active.load(Ordering::Acquire) || {
            let state = self.state.lock();
            !state.points.is_empty() || !state.region_seeds.is_empty()
        }
    }
}
