use std::sync::Arc;

use super::handle::{PersistenceHandle, PersistenceStatus};
use super::store::PersistenceStore;
use super::worker::run_worker;

const QUEUE_CAPACITY: usize = 4_096;

pub struct PersistenceRuntime {
    pub(crate) handle: PersistenceHandle,
    pub(crate) completion_rx:
        Option<tokio::sync::mpsc::Receiver<crate::game::PersistenceCompletion>>,
    pub(crate) worker: tokio::task::JoinHandle<()>,
}

impl PersistenceRuntime {
    pub fn start(
        database: Arc<crate::db::Database>,
        simulation_waker: crate::simulation_waker::SimulationWaker,
    ) -> Self {
        Self::start_with_store_and_waker(database, QUEUE_CAPACITY, simulation_waker)
    }

    #[cfg(test)]
    pub(crate) fn start_with_store<S>(store: S, capacity: usize) -> Self
    where
        S: PersistenceStore,
    {
        Self::start_with_store_and_waker(
            store,
            capacity,
            crate::simulation_waker::SimulationWaker::default(),
        )
    }

    pub(crate) fn start_with_store_and_waker<S>(
        store: S,
        capacity: usize,
        simulation_waker: crate::simulation_waker::SimulationWaker,
    ) -> Self
    where
        S: PersistenceStore,
    {
        let (tx, rx) = tokio::sync::mpsc::channel(capacity);
        let (completion_tx, completion_rx) = tokio::sync::mpsc::channel(capacity);
        let status = Arc::new(PersistenceStatus::default());
        let worker = tokio::spawn(run_worker(store, rx, status.clone(), simulation_waker));
        Self {
            handle: PersistenceHandle {
                tx,
                completion_tx,
                status,
            },
            completion_rx: Some(completion_rx),
            worker,
        }
    }

    pub fn handle(&self) -> PersistenceHandle {
        self.handle.clone()
    }

    pub const fn take_completion_receiver(
        &mut self,
    ) -> tokio::sync::mpsc::Receiver<crate::game::PersistenceCompletion> {
        self.completion_rx
            .take()
            .expect("persistence completion receiver already taken")
    }

    pub async fn shutdown(self) {
        let Self {
            handle,
            completion_rx,
            worker,
        } = self;
        drop(handle);
        drop(completion_rx);
        worker
            .await
            .expect("persistence worker failed during shutdown drain");
    }
}
