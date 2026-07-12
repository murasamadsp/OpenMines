use crate::game::{SaveCommand, SaveKind};
use crossbeam_utils::CachePadded;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

const QUEUE_CAPACITY: usize = 4_096;
const BATCH_LIMIT: usize = 128;
const RETRY_INITIAL_BACKOFF: Duration = Duration::from_millis(25);
const RETRY_MAX_BACKOFF: Duration = Duration::from_secs(2);
const BUILDING_DELETE_MAX_ATTEMPTS: u64 = 8;

struct PersistenceEnvelope {
    command: SaveCommand,
    enqueued_at: Instant,
    completion: Option<tokio::sync::mpsc::OwnedPermit<crate::game::PersistenceCompletion>>,
}

#[derive(Default)]
struct PersistenceStatus {
    accepted: CachePadded<AtomicU64>,
    completed: CachePadded<AtomicU64>,
    high_water: CachePadded<AtomicU64>,
}

impl PersistenceStatus {
    fn mark_accepted(&self) {
        let accepted = self.accepted.fetch_add(1, Ordering::AcqRel) + 1;
        let completed = self.completed.load(Ordering::Acquire);
        let backlog = accepted.saturating_sub(completed);
        self.high_water.fetch_max(backlog, Ordering::AcqRel);
        crate::metrics::PERSISTENCE_QUEUE_DEPTH.set(i64::try_from(backlog).unwrap_or(i64::MAX));
        crate::metrics::PERSISTENCE_QUEUE_HIGH_WATER
            .set(i64::try_from(self.high_water.load(Ordering::Acquire)).unwrap_or(i64::MAX));
    }

    fn mark_completed(&self, count: usize) {
        self.completed
            .fetch_add(u64::try_from(count).unwrap_or(u64::MAX), Ordering::AcqRel);
        crate::metrics::PERSISTENCE_QUEUE_DEPTH
            .set(i64::try_from(self.backlog()).unwrap_or(i64::MAX));
    }

    fn backlog(&self) -> u64 {
        self.accepted
            .load(Ordering::Acquire)
            .saturating_sub(self.completed.load(Ordering::Acquire))
    }
}

#[derive(Clone)]
pub struct PersistenceHandle {
    tx: tokio::sync::mpsc::Sender<PersistenceEnvelope>,
    completion_tx: tokio::sync::mpsc::Sender<crate::game::PersistenceCompletion>,
    status: Arc<PersistenceStatus>,
}

pub struct PersistencePermit {
    permit: tokio::sync::mpsc::OwnedPermit<PersistenceEnvelope>,
    status: Arc<PersistenceStatus>,
    kind: SaveKind,
    completion: Option<tokio::sync::mpsc::OwnedPermit<crate::game::PersistenceCompletion>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PersistenceAdmissionError {
    Full,
    Closed,
}

impl PersistenceHandle {
    pub fn check_capacity(&self, kind: SaveKind) -> Result<(), PersistenceAdmissionError> {
        if self.tx.is_closed()
            || (matches!(kind, SaveKind::Program | SaveKind::BuildingDelete)
                && self.completion_tx.is_closed())
        {
            return Err(PersistenceAdmissionError::Closed);
        }
        if self.tx.capacity() == 0
            || (matches!(kind, SaveKind::Program | SaveKind::BuildingDelete)
                && self.completion_tx.capacity() == 0)
        {
            return Err(PersistenceAdmissionError::Full);
        }
        Ok(())
    }

    pub fn try_reserve(
        &self,
        kind: SaveKind,
    ) -> Result<PersistencePermit, PersistenceAdmissionError> {
        let completion = if matches!(kind, SaveKind::Program | SaveKind::BuildingDelete) {
            match self.completion_tx.clone().try_reserve_owned() {
                Ok(permit) => Some(permit),
                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                    crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                        .with_label_values(&[kind.name(), "completion_saturated"])
                        .inc();
                    return Err(PersistenceAdmissionError::Full);
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                        .with_label_values(&[kind.name(), "completion_closed"])
                        .inc();
                    return Err(PersistenceAdmissionError::Closed);
                }
            }
        } else {
            None
        };
        match self.tx.clone().try_reserve_owned() {
            Ok(permit) => Ok(PersistencePermit {
                permit,
                status: self.status.clone(),
                kind,
                completion,
            }),
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                    .with_label_values(&[kind.name(), "saturated"])
                    .inc();
                Err(PersistenceAdmissionError::Full)
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                    .with_label_values(&[kind.name(), "closed"])
                    .inc();
                Err(PersistenceAdmissionError::Closed)
            }
        }
    }

    #[cfg(test)]
    fn backlog(&self) -> u64 {
        self.status.backlog()
    }

    #[cfg(test)]
    pub(crate) fn test_channel(capacity: usize) -> (Self, PersistenceTestReceiver) {
        let (tx, rx) = tokio::sync::mpsc::channel(capacity);
        let (completion_tx, completion_rx) = tokio::sync::mpsc::channel(capacity);
        let status = Arc::new(PersistenceStatus::default());
        (
            Self {
                tx,
                completion_tx,
                status,
            },
            PersistenceTestReceiver { rx, completion_rx },
        )
    }
}

#[cfg(test)]
pub struct PersistenceTestReceiver {
    rx: tokio::sync::mpsc::Receiver<PersistenceEnvelope>,
    completion_rx: tokio::sync::mpsc::Receiver<crate::game::PersistenceCompletion>,
}

#[cfg(test)]
impl PersistenceTestReceiver {
    pub(crate) fn try_recv(&mut self) -> Option<SaveCommand> {
        self.rx.try_recv().ok().map(|envelope| envelope.command)
    }

    pub(crate) fn completion_capacity(&self) -> usize {
        self.completion_rx.capacity()
    }
}

impl PersistencePermit {
    pub fn publish(self, command: SaveCommand) {
        let kind = command.kind();
        assert_eq!(
            self.kind, kind,
            "persistence permit kind must match published command"
        );
        self.permit.send(PersistenceEnvelope {
            command,
            enqueued_at: Instant::now(),
            completion: self.completion,
        });
        self.status.mark_accepted();
        crate::metrics::PERSISTENCE_COMMANDS_TOTAL
            .with_label_values(&[kind.name(), "accepted"])
            .inc();
    }
}

pub struct PersistenceRuntime {
    handle: PersistenceHandle,
    completion_rx: Option<tokio::sync::mpsc::Receiver<crate::game::PersistenceCompletion>>,
    worker: tokio::task::JoinHandle<()>,
}

impl PersistenceRuntime {
    pub fn start(
        database: Arc<crate::db::Database>,
        simulation_waker: crate::simulation_waker::SimulationWaker,
    ) -> Self {
        Self::start_with_store_and_waker(database, QUEUE_CAPACITY, simulation_waker)
    }

    #[cfg(test)]
    fn start_with_store<S>(store: S, capacity: usize) -> Self
    where
        S: PersistenceStore,
    {
        Self::start_with_store_and_waker(
            store,
            capacity,
            crate::simulation_waker::SimulationWaker::default(),
        )
    }

    fn start_with_store_and_waker<S>(
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

trait PersistenceStore: Clone + Send + Sync + 'static {
    fn save_players_batch(
        &self,
        players: &[crate::db::PlayerRow],
    ) -> impl Future<Output = anyhow::Result<()>> + Send;

    fn save_buildings_batch(
        &self,
        buildings: &[crate::db::BuildingRow],
    ) -> impl Future<Output = anyhow::Result<()>> + Send;

    fn save_boxes_batch(
        &self,
        writes: &[crate::db::BoxWrite],
    ) -> impl Future<Output = anyhow::Result<()>> + Send;

    fn save_program(
        &self,
        request: &crate::game::ProgramSaveRequest,
    ) -> impl Future<Output = Result<Option<crate::db::ProgramRow>, PersistenceStoreFailure>> + Send;

    fn delete_building(
        &self,
        write: &crate::db::BuildingDeleteWrite,
    ) -> impl Future<Output = Result<crate::db::BuildingDeleteOutcome, PersistenceStoreFailure>> + Send;
}

#[derive(Debug)]
enum PersistenceStoreFailure {
    Transient(anyhow::Error),
    Permanent(anyhow::Error),
}

impl PersistenceStore for Arc<crate::db::Database> {
    async fn save_players_batch(&self, players: &[crate::db::PlayerRow]) -> anyhow::Result<()> {
        crate::db::Database::save_players_batch(self, players).await
    }

    async fn save_buildings_batch(
        &self,
        buildings: &[crate::db::BuildingRow],
    ) -> anyhow::Result<()> {
        crate::db::Database::save_buildings_batch(self, buildings).await
    }

    async fn save_boxes_batch(&self, writes: &[crate::db::BoxWrite]) -> anyhow::Result<()> {
        crate::db::Database::save_boxes_batch(self, writes).await
    }

    async fn save_program(
        &self,
        request: &crate::game::ProgramSaveRequest,
    ) -> Result<Option<crate::db::ProgramRow>, PersistenceStoreFailure> {
        self.save_select_program(
            request.player_id.as_i32(),
            request.program_id,
            &request.source,
        )
        .await
        .map_err(|error| PersistenceStoreFailure::Transient(error.into()))
        .and_then(|result| match result {
            openmines_storage::programs::SaveSelectProgramResult::Saved(program) => {
                Ok(Some(program))
            }
            openmines_storage::programs::SaveSelectProgramResult::ProgramUnavailable => Ok(None),
            openmines_storage::programs::SaveSelectProgramResult::PlayerUnavailable => {
                Err(PersistenceStoreFailure::Permanent(anyhow::anyhow!(
                    "player {} is unavailable for program selection",
                    request.player_id
                )))
            }
        })
    }

    async fn delete_building(
        &self,
        write: &crate::db::BuildingDeleteWrite,
    ) -> Result<crate::db::BuildingDeleteOutcome, PersistenceStoreFailure> {
        self.apply_building_delete(write)
            .await
            .map_err(PersistenceStoreFailure::Transient)
    }
}

async fn run_worker<S>(
    store: S,
    mut rx: tokio::sync::mpsc::Receiver<PersistenceEnvelope>,
    status: Arc<PersistenceStatus>,
    simulation_waker: crate::simulation_waker::SimulationWaker,
) where
    S: PersistenceStore,
{
    let _wake_owner_on_exit = WakeOwnerOnDrop(simulation_waker.clone());
    while let Some(first) = rx.recv().await {
        let mut batch = Vec::with_capacity(BATCH_LIMIT);
        batch.push(first);
        while batch.len() < BATCH_LIMIT {
            let Ok(next) = rx.try_recv() else {
                break;
            };
            batch.push(next);
        }

        simulation_waker.wake();
        persist_batch(&store, &mut batch, &simulation_waker).await;
        status.mark_completed(batch.len());
        simulation_waker.wake();
        crate::metrics::PERSISTENCE_OLDEST_AGE_SECONDS.set(0.0);
    }
    crate::metrics::PERSISTENCE_QUEUE_DEPTH.set(0);
}

struct WakeOwnerOnDrop(crate::simulation_waker::SimulationWaker);

impl Drop for WakeOwnerOnDrop {
    fn drop(&mut self) {
        self.0.wake();
    }
}

async fn persist_batch<S>(
    store: &S,
    batch: &mut [PersistenceEnvelope],
    simulation_waker: &crate::simulation_waker::SimulationWaker,
) where
    S: PersistenceStore,
{
    let mut start = 0usize;
    while start < batch.len() {
        let kind = batch[start].command.kind();
        let end = batch[start..]
            .iter()
            .position(|envelope| envelope.command.kind() != kind)
            .map_or(batch.len(), |offset| start + offset);
        match kind {
            SaveKind::Program => {
                persist_program_batch(store, &mut batch[start..end], simulation_waker).await;
            }
            SaveKind::BuildingDelete => {
                persist_building_delete_batch(store, &mut batch[start..end], simulation_waker)
                    .await;
            }
            SaveKind::Player | SaveKind::Building | SaveKind::Box => {
                persist_compatible_batch(store, kind, &batch[start..end]).await;
            }
        }
        start = end;
    }
}

async fn persist_building_delete_batch<S>(
    store: &S,
    batch: &mut [PersistenceEnvelope],
    simulation_waker: &crate::simulation_waker::SimulationWaker,
) where
    S: PersistenceStore,
{
    for envelope in batch {
        let SaveCommand::BuildingDelete { request } = &envelope.command else {
            unreachable!("compatible building-delete batch");
        };
        let request = request.clone();
        let write = crate::db::BuildingDeleteWrite {
            building_id: request.expected.building_id,
            x: request.expected.x,
            y: request.expected.y,
            clear_resp_bindings: request.view.pack_type == crate::game::PackType::Resp,
            box_write: request.box_write.clone(),
        };
        let oldest = envelope.enqueued_at;
        let mut attempt = 0u64;
        let mut backoff = RETRY_INITIAL_BACKOFF;
        let result = loop {
            crate::metrics::PERSISTENCE_OLDEST_AGE_SECONDS.set(oldest.elapsed().as_secs_f64());
            match store.delete_building(&write).await {
                Ok(crate::db::BuildingDeleteOutcome::Deleted {
                    cleared_resp_bindings,
                }) => {
                    break crate::game::BuildingDeleteResult::Deleted {
                        cleared_resp_bindings,
                    };
                }
                Ok(crate::db::BuildingDeleteOutcome::IdentityMismatch) => {
                    break crate::game::BuildingDeleteResult::IdentityMismatch;
                }
                Err(PersistenceStoreFailure::Permanent(error)) => {
                    break crate::game::BuildingDeleteResult::PermanentFailure {
                        message: error.to_string(),
                    };
                }
                Err(PersistenceStoreFailure::Transient(error)) => {
                    attempt = attempt.saturating_add(1);
                    crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                        .with_label_values(&[SaveKind::BuildingDelete.name(), "retry"])
                        .inc();
                    tracing::warn!(
                        attempt,
                        ?backoff,
                        error = ?error,
                        building_id = request.expected.building_id,
                        x = request.expected.x,
                        y = request.expected.y,
                        "Building delete failed transiently; retrying"
                    );
                    if attempt >= BUILDING_DELETE_MAX_ATTEMPTS {
                        break crate::game::BuildingDeleteResult::PermanentFailure {
                            message: error.to_string(),
                        };
                    }
                    tokio::time::sleep(backoff).await;
                    backoff = backoff.saturating_mul(2).min(RETRY_MAX_BACKOFF);
                }
            }
        };

        envelope
            .completion
            .take()
            .expect("building-delete command must reserve completion capacity")
            .send(crate::game::PersistenceCompletion::BuildingDeleted { request, result });
        simulation_waker.wake();
        crate::metrics::PERSISTENCE_COMMANDS_TOTAL
            .with_label_values(&[SaveKind::BuildingDelete.name(), "persisted"])
            .inc();
        crate::metrics::PERSISTENCE_BATCH_SIZE.observe(1.0);
    }
}

async fn persist_program_batch<S>(
    store: &S,
    batch: &mut [PersistenceEnvelope],
    simulation_waker: &crate::simulation_waker::SimulationWaker,
) where
    S: PersistenceStore,
{
    for envelope in batch {
        let SaveCommand::Program { request } = &envelope.command else {
            unreachable!("compatible program batch");
        };
        let request = request.clone();
        let oldest = envelope.enqueued_at;
        let mut attempt = 0u64;
        let mut backoff = RETRY_INITIAL_BACKOFF;
        let result = loop {
            crate::metrics::PERSISTENCE_OLDEST_AGE_SECONDS.set(oldest.elapsed().as_secs_f64());
            match store.save_program(&request).await {
                Ok(Some(program)) => {
                    break crate::game::ProgramSaveResult::Saved {
                        program_name: program.name,
                    };
                }
                Ok(None) => break crate::game::ProgramSaveResult::Rejected,
                Err(PersistenceStoreFailure::Permanent(error)) => {
                    break crate::game::ProgramSaveResult::PermanentFailure {
                        message: error.to_string(),
                    };
                }
                Err(PersistenceStoreFailure::Transient(error)) => {
                    attempt = attempt.saturating_add(1);
                    crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                        .with_label_values(&[SaveKind::Program.name(), "retry"])
                        .inc();
                    tracing::warn!(
                        attempt,
                        ?backoff,
                        error = ?error,
                        player_id = %request.player_id,
                        program_id = request.program_id,
                        "Program persistence failed transiently; retrying"
                    );
                    tokio::time::sleep(backoff).await;
                    backoff = backoff.saturating_mul(2).min(RETRY_MAX_BACKOFF);
                }
            }
        };

        envelope
            .completion
            .take()
            .expect("program command must reserve completion capacity")
            .send(crate::game::PersistenceCompletion::ProgramSaved { request, result });
        simulation_waker.wake();
        crate::metrics::PERSISTENCE_COMMANDS_TOTAL
            .with_label_values(&[SaveKind::Program.name(), "persisted"])
            .inc();
        crate::metrics::PERSISTENCE_BATCH_SIZE.observe(1.0);
    }
}

async fn persist_compatible_batch<S>(store: &S, kind: SaveKind, batch: &[PersistenceEnvelope])
where
    S: PersistenceStore,
{
    let oldest = batch[0].enqueued_at;
    let mut attempt = 0u64;
    let mut backoff = RETRY_INITIAL_BACKOFF;
    loop {
        crate::metrics::PERSISTENCE_OLDEST_AGE_SECONDS.set(oldest.elapsed().as_secs_f64());
        let result = match kind {
            SaveKind::Player => {
                let rows = batch
                    .iter()
                    .map(|envelope| match &envelope.command {
                        SaveCommand::Player { row } => row.as_ref().clone(),
                        SaveCommand::Building { .. }
                        | SaveCommand::Box { .. }
                        | SaveCommand::Program { .. }
                        | SaveCommand::BuildingDelete { .. } => {
                            unreachable!("compatible player batch")
                        }
                    })
                    .collect::<Vec<_>>();
                store.save_players_batch(&rows).await
            }
            SaveKind::Building => {
                let rows = batch
                    .iter()
                    .map(|envelope| match &envelope.command {
                        SaveCommand::Building { row } => row.as_ref().clone(),
                        SaveCommand::Player { .. }
                        | SaveCommand::Box { .. }
                        | SaveCommand::Program { .. }
                        | SaveCommand::BuildingDelete { .. } => {
                            unreachable!("compatible building batch")
                        }
                    })
                    .collect::<Vec<_>>();
                store.save_buildings_batch(&rows).await
            }
            SaveKind::Box => {
                let writes = batch
                    .iter()
                    .map(|envelope| match &envelope.command {
                        SaveCommand::Box { write } => write.clone(),
                        SaveCommand::Player { .. }
                        | SaveCommand::Building { .. }
                        | SaveCommand::Program { .. }
                        | SaveCommand::BuildingDelete { .. } => {
                            unreachable!("compatible box batch")
                        }
                    })
                    .collect::<Vec<_>>();
                store.save_boxes_batch(&writes).await
            }
            SaveKind::Program | SaveKind::BuildingDelete => {
                unreachable!("completion command routed to compatible batch")
            }
        };
        match result {
            Ok(()) => {
                let batch_size =
                    u32::try_from(batch.len()).expect("persistence batch limit fits u32");
                crate::metrics::PERSISTENCE_BATCH_SIZE.observe(f64::from(batch_size));
                if kind == SaveKind::Player {
                    crate::metrics::PLAYER_SAVE_TOTAL
                        .inc_by(u64::try_from(batch.len()).unwrap_or(u64::MAX));
                }
                crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                    .with_label_values(&[kind.name(), "persisted"])
                    .inc_by(u64::try_from(batch.len()).unwrap_or(u64::MAX));
                return;
            }
            Err(error) => {
                attempt = attempt.saturating_add(1);
                crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                    .with_label_values(&[kind.name(), "retry"])
                    .inc();
                tracing::warn!(
                    attempt,
                    ?backoff,
                    error = ?error,
                    batch_size = batch.len(),
                    "Persistence batch failed; retrying without dropping durable commands"
                );
                tokio::time::sleep(backoff).await;
                backoff = backoff.saturating_mul(2).min(RETRY_MAX_BACKOFF);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[derive(Clone)]
    struct TestStore {
        calls: Arc<CachePadded<AtomicUsize>>,
        failures_left: Arc<CachePadded<AtomicUsize>>,
        started: Arc<tokio::sync::Semaphore>,
        release: Arc<tokio::sync::Semaphore>,
        blocked_calls: Arc<Vec<usize>>,
        permanent_program_failure: Arc<CachePadded<AtomicBool>>,
        permanent_building_failure: Arc<CachePadded<AtomicBool>>,
        saved: Arc<Mutex<Vec<SavedBatch>>>,
    }

    #[derive(Debug, Eq, PartialEq)]
    enum SavedBatch {
        Players(Vec<i32>),
        Buildings(Vec<i32>),
        Boxes(Vec<(i32, i32)>),
        Program {
            player_id: i32,
            program_id: i32,
            source: String,
        },
        BuildingDelete(i32),
    }

    impl TestStore {
        fn new(block_first: bool, failures: usize) -> Self {
            Self {
                calls: Arc::new(CachePadded::new(AtomicUsize::new(0))),
                failures_left: Arc::new(CachePadded::new(AtomicUsize::new(failures))),
                started: Arc::new(tokio::sync::Semaphore::new(0)),
                release: Arc::new(tokio::sync::Semaphore::new(0)),
                blocked_calls: Arc::new(if block_first { vec![0] } else { Vec::new() }),
                permanent_program_failure: Arc::new(CachePadded::new(AtomicBool::new(false))),
                permanent_building_failure: Arc::new(CachePadded::new(AtomicBool::new(false))),
                saved: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn reject_program_permanently(&self) {
            self.permanent_program_failure
                .store(true, Ordering::Release);
        }

        fn with_blocked_calls(calls: &[usize]) -> Self {
            let mut store = Self::new(false, 0);
            store.blocked_calls = Arc::new(calls.to_vec());
            store
        }

        fn reject_building_permanently(&self) {
            self.permanent_building_failure
                .store(true, Ordering::Release);
        }

        async fn persist(&self, batch: SavedBatch) -> anyhow::Result<()> {
            let call = self.calls.fetch_add(1, Ordering::AcqRel);
            self.started.add_permits(1);
            if self.blocked_calls.contains(&call) {
                self.release
                    .acquire()
                    .await
                    .expect("release semaphore")
                    .forget();
            }
            if self
                .failures_left
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |left| {
                    if left > 0 { Some(left - 1) } else { None }
                })
                .is_ok()
            {
                anyhow::bail!("injected persistence failure");
            }
            self.saved.lock().expect("saved lock").push(batch);
            Ok(())
        }
    }

    impl PersistenceStore for TestStore {
        fn save_players_batch(
            &self,
            players: &[crate::db::PlayerRow],
        ) -> impl Future<Output = anyhow::Result<()>> + Send {
            let ids = players.iter().map(|player| player.id).collect::<Vec<_>>();
            let store = self.clone();
            async move { store.persist(SavedBatch::Players(ids)).await }
        }

        fn save_buildings_batch(
            &self,
            buildings: &[crate::db::BuildingRow],
        ) -> impl Future<Output = anyhow::Result<()>> + Send {
            let ids = buildings
                .iter()
                .map(|building| building.id)
                .collect::<Vec<_>>();
            let store = self.clone();
            async move { store.persist(SavedBatch::Buildings(ids)).await }
        }

        fn save_boxes_batch(
            &self,
            writes: &[crate::db::BoxWrite],
        ) -> impl Future<Output = anyhow::Result<()>> + Send {
            let positions = writes.iter().map(|write| (write.x, write.y)).collect();
            let store = self.clone();
            async move { store.persist(SavedBatch::Boxes(positions)).await }
        }

        fn save_program(
            &self,
            request: &crate::game::ProgramSaveRequest,
        ) -> impl Future<Output = Result<Option<crate::db::ProgramRow>, PersistenceStoreFailure>> + Send
        {
            let store = self.clone();
            let request = request.clone();
            async move {
                if store.permanent_program_failure.load(Ordering::Acquire) {
                    store.calls.fetch_add(1, Ordering::AcqRel);
                    return Err(PersistenceStoreFailure::Permanent(anyhow::anyhow!(
                        "injected permanent program failure"
                    )));
                }
                store
                    .persist(SavedBatch::Program {
                        player_id: request.player_id.as_i32(),
                        program_id: request.program_id,
                        source: request.source.clone(),
                    })
                    .await
                    .map_err(PersistenceStoreFailure::Transient)?;
                Ok(Some(crate::db::ProgramRow {
                    id: request.program_id,
                    player_id: request.player_id.as_i32(),
                    name: "main".to_owned(),
                    code: request.source,
                }))
            }
        }

        fn delete_building(
            &self,
            write: &crate::db::BuildingDeleteWrite,
        ) -> impl Future<Output = Result<crate::db::BuildingDeleteOutcome, PersistenceStoreFailure>> + Send
        {
            let store = self.clone();
            let building_id = write.building_id;
            async move {
                if store.permanent_building_failure.load(Ordering::Acquire) {
                    store.calls.fetch_add(1, Ordering::AcqRel);
                    return Err(PersistenceStoreFailure::Permanent(anyhow::anyhow!(
                        "injected permanent building failure"
                    )));
                }
                store
                    .persist(SavedBatch::BuildingDelete(building_id))
                    .await
                    .map_err(PersistenceStoreFailure::Transient)?;
                Ok(crate::db::BuildingDeleteOutcome::Deleted {
                    cleared_resp_bindings: 0,
                })
            }
        }
    }

    fn player(id: i32) -> crate::db::PlayerRow {
        crate::db::PlayerRow {
            id,
            name: format!("player-{id}"),
            passwd: String::new(),
            hash: String::new(),
            x: 0,
            y: 0,
            dir: 0,
            health: 100,
            max_health: 100,
            money: 0,
            creds: 0,
            skin: 0,
            auto_dig: false,
            aggression: false,
            crystals: [0; 6],
            clan_id: None,
            resp_x: None,
            resp_y: None,
            inventory: HashMap::new(),
            skills: crate::db::SkillSlots {
                skills: HashMap::new(),
                total_slots: 20,
            },
            role: 0,
            selected_program_id: None,
            selected_program: None,
            programmator_running: false,
            programmator_snapshot: None,
            clan_rank: 0,
            last_bonus_at: 0,
        }
    }

    #[test]
    fn capacity_check_distinguishes_closed_from_saturation() {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let (completion_tx, completion_rx) = tokio::sync::mpsc::channel(1);
        let handle = PersistenceHandle {
            tx,
            completion_tx,
            status: Arc::new(PersistenceStatus::default()),
        };

        assert_eq!(handle.check_capacity(SaveKind::Player), Ok(()));
        let permit = handle.tx.try_reserve().unwrap();
        assert_eq!(
            handle.check_capacity(SaveKind::Player),
            Err(PersistenceAdmissionError::Full)
        );
        drop(permit);
        drop(completion_rx);
        assert_eq!(
            handle.check_capacity(SaveKind::Program),
            Err(PersistenceAdmissionError::Closed)
        );
        assert_eq!(handle.check_capacity(SaveKind::Player), Ok(()));
        drop(rx);
        assert_eq!(
            handle.check_capacity(SaveKind::Player),
            Err(PersistenceAdmissionError::Closed)
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn each_completion_wakes_owner_before_later_batch_work_finishes() {
        let store = TestStore::with_blocked_calls(&[0, 1]);
        let waker = crate::simulation_waker::SimulationWaker::default();
        waker.register_current();
        let mut runtime = PersistenceRuntime::start_with_store_and_waker(store.clone(), 4, waker);
        let mut completions = runtime.take_completion_receiver();
        let handle = runtime.handle();
        publish_program(&handle, 7, 11, 1);
        publish_program(&handle, 7, 12, 2);

        store.started.acquire().await.unwrap().forget();
        std::thread::park_timeout(Duration::ZERO);
        store.release.add_permits(1);
        store.started.acquire().await.unwrap().forget();
        assert!(completions.recv().await.is_some());

        let started = Instant::now();
        std::thread::park_timeout(Duration::from_millis(200));
        assert!(started.elapsed() < Duration::from_millis(50));

        store.release.add_permits(1);
        drop(handle);
        runtime.shutdown().await;
        assert!(completions.recv().await.is_some());
    }

    fn publish(handle: &PersistenceHandle, id: i32) {
        handle
            .try_reserve(SaveKind::Player)
            .expect("persistence capacity")
            .publish(SaveCommand::Player {
                row: Box::new(player(id)),
            });
    }

    fn publish_building(handle: &PersistenceHandle, id: i32) {
        handle
            .try_reserve(SaveKind::Building)
            .expect("persistence capacity")
            .publish(SaveCommand::Building {
                row: Box::new(building(id)),
            });
    }

    fn publish_box(handle: &PersistenceHandle, x: i32, y: i32) {
        handle
            .try_reserve(SaveKind::Box)
            .expect("persistence capacity")
            .publish(SaveCommand::Box {
                write: crate::db::BoxWrite {
                    x,
                    y,
                    crystals: None,
                },
            });
    }

    fn publish_program(
        handle: &PersistenceHandle,
        player_id: i32,
        session_id: u64,
        program_id: i32,
    ) {
        handle
            .try_reserve(SaveKind::Program)
            .expect("program persistence capacity")
            .publish(SaveCommand::Program {
                request: crate::game::ProgramSaveRequest {
                    player_id: crate::game::PlayerId(player_id),
                    session_id: crate::game::SessionId::new(session_id),
                    program_id,
                    source: "source".to_owned(),
                },
            });
    }

    fn publish_building_delete(handle: &PersistenceHandle, building_id: i32, operation_id: u64) {
        handle
            .try_reserve(SaveKind::BuildingDelete)
            .expect("building-delete persistence capacity")
            .publish(SaveCommand::BuildingDelete {
                request: crate::game::BuildingDeleteRequest {
                    operation_id: crate::game::BuildingDeleteOperationId::new(operation_id),
                    expected: crate::game::BuildingIdentity {
                        building_id,
                        x: 10,
                        y: 20,
                    },
                    view: crate::game::PackView {
                        id: building_id,
                        pack_type: crate::game::PackType::Resp,
                        x: 10,
                        y: 20,
                        owner_id: crate::game::PlayerId(1),
                        clan_id: 0,
                        charge: 0,
                        max_charge: 0,
                        hp: 100,
                        max_hp: 100,
                    },
                    cause: crate::game::BuildingDeleteCause::Damage {
                        trigger_player_id: None,
                    },
                    box_write: None,
                    inventory_drop_item: None,
                },
            });
    }

    fn building(id: i32) -> crate::db::BuildingRow {
        crate::db::BuildingRow {
            id,
            type_code: "G".to_owned(),
            x: id,
            y: 0,
            owner_id: 1,
            clan_id: 0,
            charge: 0,
            max_charge: 0,
            cost: 0,
            hp: 100,
            max_hp: 100,
            money_inside: 0,
            crystals_inside: [0; 6],
            items_inside: HashMap::new(),
            craft_recipe_id: None,
            craft_num: 0,
            craft_end_ts: 0,
            craft_ready: false,
            clanzone: 0,
        }
    }

    #[tokio::test]
    async fn saturation_rejects_before_mutation_and_shutdown_drains_fifo() {
        let store = TestStore::new(true, 0);
        let runtime = PersistenceRuntime::start_with_store(store.clone(), 1);
        let handle = runtime.handle();

        publish(&handle, 1);
        store
            .started
            .acquire()
            .await
            .expect("started semaphore")
            .forget();
        publish(&handle, 2);
        assert_eq!(handle.backlog(), 2);
        assert!(matches!(
            handle.try_reserve(SaveKind::Player),
            Err(PersistenceAdmissionError::Full)
        ));

        store.release.add_permits(1);
        drop(handle);
        runtime.shutdown().await;

        assert_eq!(
            *store.saved.lock().expect("saved lock"),
            vec![SavedBatch::Players(vec![1]), SavedBatch::Players(vec![2])]
        );
    }

    #[tokio::test]
    async fn transient_failures_retry_without_losing_batch() {
        let store = TestStore::new(false, 2);
        let runtime = PersistenceRuntime::start_with_store(store.clone(), 4);
        let handle = runtime.handle();
        publish(&handle, 7);
        drop(handle);

        runtime.shutdown().await;

        assert_eq!(store.calls.load(Ordering::Acquire), 3);
        assert_eq!(
            *store.saved.lock().expect("saved lock"),
            vec![SavedBatch::Players(vec![7])]
        );
    }

    #[tokio::test]
    async fn slow_store_does_not_block_admission_while_queue_has_capacity() {
        let store = TestStore::new(true, 0);
        let runtime = PersistenceRuntime::start_with_store(store.clone(), 4);
        let handle = runtime.handle();
        publish(&handle, 1);
        store
            .started
            .acquire()
            .await
            .expect("started semaphore")
            .forget();

        let admission_started = Instant::now();
        for id in 2..=5 {
            publish(&handle, id);
        }
        assert!(
            admission_started.elapsed() < Duration::from_millis(100),
            "blocked SQLite worker must not block persistence producers"
        );

        store.release.add_permits(1);
        drop(handle);
        runtime.shutdown().await;

        assert_eq!(
            *store.saved.lock().expect("saved lock"),
            vec![
                SavedBatch::Players(vec![1]),
                SavedBatch::Players(vec![2, 3, 4, 5])
            ]
        );
    }

    #[tokio::test]
    async fn mixed_kinds_batch_only_contiguous_commands_and_preserve_fifo() {
        let store = TestStore::new(false, 0);
        let runtime = PersistenceRuntime::start_with_store(store.clone(), 8);
        let handle = runtime.handle();
        publish(&handle, 1);
        publish(&handle, 2);
        publish_building(&handle, 10);
        publish_building(&handle, 11);
        publish_box(&handle, 20, 21);
        publish(&handle, 3);
        drop(handle);

        runtime.shutdown().await;

        assert_eq!(
            *store.saved.lock().expect("saved lock"),
            vec![
                SavedBatch::Players(vec![1, 2]),
                SavedBatch::Buildings(vec![10, 11]),
                SavedBatch::Boxes(vec![(20, 21)]),
                SavedBatch::Players(vec![3]),
            ]
        );
    }

    #[tokio::test]
    async fn program_transient_failure_retries_and_completes_once() {
        let store = TestStore::new(false, 2);
        let mut runtime = PersistenceRuntime::start_with_store(store.clone(), 4);
        let mut completions = runtime.take_completion_receiver();
        let handle = runtime.handle();
        publish_program(&handle, 7, 11, 23);
        drop(handle);

        runtime.shutdown().await;

        assert_eq!(store.calls.load(Ordering::Acquire), 3);
        assert_eq!(
            *store.saved.lock().expect("saved lock"),
            vec![SavedBatch::Program {
                player_id: 7,
                program_id: 23,
                source: "source".to_owned(),
            }]
        );
        assert!(matches!(
            completions.try_recv(),
            Ok(crate::game::PersistenceCompletion::ProgramSaved {
                request: crate::game::ProgramSaveRequest {
                    player_id: crate::game::PlayerId(7),
                    session_id,
                    program_id: 23,
                    ..
                },
                result: crate::game::ProgramSaveResult::Saved { ref program_name },
            }) if session_id == crate::game::SessionId::new(11) && program_name == "main"
        ));
        assert!(completions.try_recv().is_err());
    }

    #[tokio::test]
    async fn program_permanent_failure_does_not_retry_forever() {
        let store = TestStore::new(false, 0);
        store.reject_program_permanently();
        let mut runtime = PersistenceRuntime::start_with_store(store.clone(), 4);
        let mut completions = runtime.take_completion_receiver();
        let handle = runtime.handle();
        publish_program(&handle, 7, 11, 23);
        drop(handle);

        runtime.shutdown().await;

        assert_eq!(store.calls.load(Ordering::Acquire), 1);
        assert!(store.saved.lock().expect("saved lock").is_empty());
        assert!(matches!(
            completions.try_recv(),
            Ok(crate::game::PersistenceCompletion::ProgramSaved {
                result: crate::game::ProgramSaveResult::PermanentFailure { .. },
                ..
            })
        ));
    }

    #[tokio::test]
    async fn pending_program_completion_bounds_new_program_admission() {
        let store = TestStore::new(false, 0);
        let mut runtime = PersistenceRuntime::start_with_store(store, 1);
        let mut completions = runtime.take_completion_receiver();
        let handle = runtime.handle();
        publish_program(&handle, 7, 11, 23);

        while handle.backlog() != 0 {
            tokio::task::yield_now().await;
        }
        assert!(matches!(
            handle.try_reserve(SaveKind::Program),
            Err(PersistenceAdmissionError::Full)
        ));
        assert!(completions.try_recv().is_ok());
        assert!(handle.try_reserve(SaveKind::Program).is_ok());

        drop(handle);
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn building_delete_retries_and_completes_once() {
        let store = TestStore::new(false, 2);
        let mut runtime = PersistenceRuntime::start_with_store(store.clone(), 4);
        let mut completions = runtime.take_completion_receiver();
        let handle = runtime.handle();
        publish_building_delete(&handle, 41, 7);
        drop(handle);

        runtime.shutdown().await;

        assert_eq!(store.calls.load(Ordering::Acquire), 3);
        assert_eq!(
            *store.saved.lock().expect("saved lock"),
            vec![SavedBatch::BuildingDelete(41)]
        );
        assert!(matches!(
            completions.try_recv(),
            Ok(crate::game::PersistenceCompletion::BuildingDeleted {
                request: crate::game::BuildingDeleteRequest {
                    operation_id,
                    expected: crate::game::BuildingIdentity { building_id: 41, .. },
                    ..
                },
                result: crate::game::BuildingDeleteResult::Deleted {
                    cleared_resp_bindings: 0
                },
            }) if operation_id == crate::game::BuildingDeleteOperationId::new(7)
        ));
        assert!(matches!(
            completions.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected)
        ));
    }

    #[tokio::test]
    async fn permanent_building_delete_failure_completes_without_retry_loop() {
        let store = TestStore::new(false, 0);
        store.reject_building_permanently();
        let mut runtime = PersistenceRuntime::start_with_store(store.clone(), 4);
        let mut completions = runtime.take_completion_receiver();
        let handle = runtime.handle();
        publish_building_delete(&handle, 41, 9);
        drop(handle);

        runtime.shutdown().await;

        assert_eq!(store.calls.load(Ordering::Acquire), 1);
        assert!(store.saved.lock().expect("saved lock").is_empty());
        assert!(matches!(
            completions.try_recv(),
            Ok(crate::game::PersistenceCompletion::BuildingDeleted {
                request: crate::game::BuildingDeleteRequest {
                    operation_id,
                    ..
                },
                result: crate::game::BuildingDeleteResult::PermanentFailure { .. },
            }) if operation_id == crate::game::BuildingDeleteOperationId::new(9)
        ));
        assert!(matches!(
            completions.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected)
        ));
    }
}
