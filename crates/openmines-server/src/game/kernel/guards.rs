use bevy_ecs::prelude::World as EcsWorld;
use parking_lot::{RwLockReadGuard, RwLockWriteGuard};
use std::ops::{Deref, DerefMut};
use std::time::{Duration, Instant};

pub const ECS_LOCK_PROFILE_THRESHOLD: Duration = Duration::from_millis(25);

pub struct ProfiledEcsReadGuard<'a> {
    pub(crate) label: &'static str,
    pub(crate) acquired_at: Instant,
    pub(crate) guard: Option<RwLockReadGuard<'a, EcsWorld>>,
}

impl<'a> ProfiledEcsReadGuard<'a> {
    #[allow(dead_code)]
    pub const fn new(
        label: &'static str,
        acquired_at: Instant,
        guard: RwLockReadGuard<'a, EcsWorld>,
    ) -> Self {
        Self {
            label,
            acquired_at,
            guard: Some(guard),
        }
    }
}

impl Deref for ProfiledEcsReadGuard<'_> {
    type Target = EcsWorld;

    fn deref(&self) -> &Self::Target {
        self.guard
            .as_deref()
            .expect("profiled ECS read guard already dropped")
    }
}

impl Drop for ProfiledEcsReadGuard<'_> {
    fn drop(&mut self) {
        let held = self.acquired_at.elapsed();
        drop(self.guard.take());
        if held > ECS_LOCK_PROFILE_THRESHOLD {
            tracing::warn!(
                target: "tickprof",
                label = self.label,
                held = ?held,
                threshold = ?ECS_LOCK_PROFILE_THRESHOLD,
                "ECS read lock held over threshold"
            );
        }
    }
}

pub struct ProfiledEcsWriteGuard<'a> {
    pub(crate) label: &'static str,
    pub(crate) acquired_at: Instant,
    pub(crate) guard: Option<RwLockWriteGuard<'a, EcsWorld>>,
}

impl<'a> ProfiledEcsWriteGuard<'a> {
    #[allow(dead_code)]
    pub const fn new(
        label: &'static str,
        acquired_at: Instant,
        guard: RwLockWriteGuard<'a, EcsWorld>,
    ) -> Self {
        Self {
            label,
            acquired_at,
            guard: Some(guard),
        }
    }
}

impl Deref for ProfiledEcsWriteGuard<'_> {
    type Target = EcsWorld;

    fn deref(&self) -> &Self::Target {
        self.guard
            .as_deref()
            .expect("profiled ECS write guard already dropped")
    }
}

impl DerefMut for ProfiledEcsWriteGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard
            .as_deref_mut()
            .expect("profiled ECS write guard already dropped")
    }
}

impl Drop for ProfiledEcsWriteGuard<'_> {
    fn drop(&mut self) {
        let held = self.acquired_at.elapsed();
        drop(self.guard.take());
        if held > ECS_LOCK_PROFILE_THRESHOLD {
            tracing::warn!(
                target: "tickprof",
                label = self.label,
                held = ?held,
                threshold = ?ECS_LOCK_PROFILE_THRESHOLD,
                "ECS write lock held over threshold"
            );
        }
    }
}
