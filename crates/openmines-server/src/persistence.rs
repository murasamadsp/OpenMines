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

struct PersistenceEnvelope {
    command: SaveCommand,
    enqueued_at: Instant,
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
    status: Arc<PersistenceStatus>,
}

pub struct PersistencePermit {
    permit: tokio::sync::mpsc::OwnedPermit<PersistenceEnvelope>,
    status: Arc<PersistenceStatus>,
    kind: SaveKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PersistenceAdmissionError {
    Full,
    Closed,
}

impl PersistenceHandle {
    pub fn try_reserve(
        &self,
        kind: SaveKind,
    ) -> Result<PersistencePermit, PersistenceAdmissionError> {
        match self.tx.clone().try_reserve_owned() {
            Ok(permit) => Ok(PersistencePermit {
                permit,
                status: self.status.clone(),
                kind,
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
        let status = Arc::new(PersistenceStatus::default());
        (Self { tx, status }, PersistenceTestReceiver { rx })
    }
}

#[cfg(test)]
pub struct PersistenceTestReceiver {
    rx: tokio::sync::mpsc::Receiver<PersistenceEnvelope>,
}

#[cfg(test)]
impl PersistenceTestReceiver {
    pub(crate) fn try_recv(&mut self) -> Option<SaveCommand> {
        self.rx.try_recv().ok().map(|envelope| envelope.command)
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
        });
        self.status.mark_accepted();
        crate::metrics::PERSISTENCE_COMMANDS_TOTAL
            .with_label_values(&[kind.name(), "accepted"])
            .inc();
    }
}

pub struct PersistenceRuntime {
    handle: PersistenceHandle,
    worker: tokio::task::JoinHandle<()>,
}

impl PersistenceRuntime {
    pub fn start(database: Arc<crate::db::Database>) -> Self {
        Self::start_with_store(database, QUEUE_CAPACITY)
    }

    fn start_with_store<S>(store: S, capacity: usize) -> Self
    where
        S: PersistenceStore,
    {
        let (tx, rx) = tokio::sync::mpsc::channel(capacity);
        let status = Arc::new(PersistenceStatus::default());
        let worker = tokio::spawn(run_worker(store, rx, status.clone()));
        Self {
            handle: PersistenceHandle { tx, status },
            worker,
        }
    }

    pub fn handle(&self) -> PersistenceHandle {
        self.handle.clone()
    }

    pub async fn shutdown(self) {
        let Self { handle, worker } = self;
        drop(handle);
        if let Err(error) = worker.await {
            tracing::error!(error = ?error, "Persistence worker failed during shutdown drain");
        }
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
}

async fn run_worker<S>(
    store: S,
    mut rx: tokio::sync::mpsc::Receiver<PersistenceEnvelope>,
    status: Arc<PersistenceStatus>,
) where
    S: PersistenceStore,
{
    while let Some(first) = rx.recv().await {
        let mut batch = Vec::with_capacity(BATCH_LIMIT);
        batch.push(first);
        while batch.len() < BATCH_LIMIT {
            let Ok(next) = rx.try_recv() else {
                break;
            };
            batch.push(next);
        }

        persist_batch(&store, &batch).await;
        status.mark_completed(batch.len());
        crate::metrics::PERSISTENCE_OLDEST_AGE_SECONDS.set(0.0);
    }
    crate::metrics::PERSISTENCE_QUEUE_DEPTH.set(0);
}

async fn persist_batch<S>(store: &S, batch: &[PersistenceEnvelope])
where
    S: PersistenceStore,
{
    let mut start = 0usize;
    while start < batch.len() {
        let kind = batch[start].command.kind();
        let end = batch[start..]
            .iter()
            .position(|envelope| envelope.command.kind() != kind)
            .map_or(batch.len(), |offset| start + offset);
        persist_compatible_batch(store, kind, &batch[start..end]).await;
        start = end;
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
                        SaveCommand::Building { .. } | SaveCommand::Box { .. } => {
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
                        SaveCommand::Player { .. } | SaveCommand::Box { .. } => {
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
                        SaveCommand::Player { .. } | SaveCommand::Building { .. } => {
                            unreachable!("compatible box batch")
                        }
                    })
                    .collect::<Vec<_>>();
                store.save_boxes_batch(&writes).await
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone)]
    struct TestStore {
        calls: Arc<CachePadded<AtomicUsize>>,
        failures_left: Arc<CachePadded<AtomicUsize>>,
        started: Arc<tokio::sync::Semaphore>,
        release: Arc<tokio::sync::Semaphore>,
        block_first: bool,
        saved: Arc<Mutex<Vec<SavedBatch>>>,
    }

    #[derive(Debug, Eq, PartialEq)]
    enum SavedBatch {
        Players(Vec<i32>),
        Buildings(Vec<i32>),
        Boxes(Vec<(i32, i32)>),
    }

    impl TestStore {
        fn new(block_first: bool, failures: usize) -> Self {
            Self {
                calls: Arc::new(CachePadded::new(AtomicUsize::new(0))),
                failures_left: Arc::new(CachePadded::new(AtomicUsize::new(failures))),
                started: Arc::new(tokio::sync::Semaphore::new(0)),
                release: Arc::new(tokio::sync::Semaphore::new(0)),
                block_first,
                saved: Arc::new(Mutex::new(Vec::new())),
            }
        }

        async fn persist(&self, batch: SavedBatch) -> anyhow::Result<()> {
            let call = self.calls.fetch_add(1, Ordering::AcqRel);
            self.started.add_permits(1);
            if self.block_first && call == 0 {
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
}
