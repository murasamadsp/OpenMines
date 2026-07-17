use bevy_ecs::prelude::Resource;
use parking_lot::Mutex;
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::super::WorldPos;
use super::types::{BoxPickupIntent, BroadcastEffect, PendingConversion, ProgrammatorAction};

#[derive(Default)]
struct BoxPickupQueueState {
    queue: VecDeque<BoxPickupIntent>,
    players: HashSet<super::super::PlayerId>,
}

#[derive(Resource, Clone, Default)]
pub struct BoxPickupQueue(Arc<Mutex<BoxPickupQueueState>>);

impl BoxPickupQueue {
    pub fn push(&self, intent: BoxPickupIntent) {
        let mut state = self.0.lock();
        if state.players.insert(intent.player_id) {
            state.queue.push_back(intent);
        }
    }

    pub fn drain(&self) -> Vec<BoxPickupIntent> {
        let mut state = self.0.lock();
        state.players.clear();
        state.queue.drain(..).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.0.lock().queue.is_empty()
    }
}

#[derive(Default)]
struct GranularWakeState {
    points: HashSet<WorldPos>,
    region_seeds: HashSet<WorldPos>,
}

#[derive(Resource, Clone)]
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

#[derive(Resource, Default)]
pub struct BroadcastQueue(pub Vec<BroadcastEffect>);

#[derive(Resource, Default)]
pub struct ProgrammatorQueue(pub Vec<ProgrammatorAction>);

#[derive(Resource, Default)]
pub struct PendingCellConversions(pub Vec<PendingConversion>);

/// Координаты зданий, которым нужен HB O re-broadcast после обнуления charge (C# `ResendPack`).
#[derive(Resource, Default)]
pub struct PackResendQueue(pub Vec<(i32, i32)>);
