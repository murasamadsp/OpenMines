use super::*;
use crossbeam_utils::CachePadded;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

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
    Chats(Vec<i64>),
    ChatColor(i32),
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
    async fn create_program(
        &self,
        _request: &crate::game::ProgramCreateRequest,
    ) -> Result<i32, PersistenceStoreFailure> {
        Ok(1)
    }

    async fn program_menu(
        &self,
        _request: &crate::game::ProgramMenuRequest,
    ) -> Result<Vec<crate::db::ProgramRow>, PersistenceStoreFailure> {
        Ok(Vec::new())
    }

    async fn copy_program(
        &self,
        _request: &crate::game::ProgramCopyRequest,
    ) -> Result<bool, PersistenceStoreFailure> {
        Ok(true)
    }

    async fn building_menu(
        &self,
        _request: &crate::game::BuildingMenuRequest,
    ) -> Result<Vec<crate::db::buildings::BuildingRow>, PersistenceStoreFailure> {
        Ok(Vec::new())
    }

    async fn auction_grid(
        &self,
        _request: &crate::game::AuctionGridRequest,
    ) -> Result<crate::game::AuctionGridResult, PersistenceStoreFailure> {
        Ok(crate::game::AuctionGridResult::Loaded { counts: Vec::new() })
    }

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

    fn save_chat_messages_batch(
        &self,
        messages: &[crate::game::ChatAppendRequest],
    ) -> impl Future<Output = anyhow::Result<()>> + Send {
        let ids = messages.iter().map(|msg| msg.id).collect();
        let store = self.clone();
        async move { store.persist(SavedBatch::Chats(ids)).await }
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

    fn cycle_chat_color(
        &self,
        request: &crate::game::ChatColorCycleRequest,
    ) -> impl Future<Output = Result<Option<i32>, PersistenceStoreFailure>> + Send {
        let store = self.clone();
        let player_id = request.player_id.as_i32();
        async move {
            store
                .persist(SavedBatch::ChatColor(player_id))
                .await
                .map_err(PersistenceStoreFailure::Transient)?;
            Ok(Some(1))
        }
    }

    fn chat_resync(
        &self,
        request: &crate::game::ChatResyncRequest,
    ) -> impl Future<Output = Result<crate::game::ChatResyncResult, PersistenceStoreFailure>> + Send
    {
        let channel_name = request.channel_tag.clone();
        async move {
            Ok(crate::game::ChatResyncResult::Success {
                channel_name,
                messages: vec![],
            })
        }
    }

    async fn chat_menu(
        &self,
        _request: &crate::game::ChatMenuRequest,
    ) -> Result<crate::game::ChatMenuResult, PersistenceStoreFailure> {
        Ok(crate::game::ChatMenuResult::Success { channels: vec![] })
    }

    fn chat_private(
        &self,
        request: &crate::game::ChatPrivateRequest,
    ) -> impl Future<Output = Result<crate::game::ChatPrivateResult, PersistenceStoreFailure>> + Send
    {
        let target_uid = request.target_uid;
        async move {
            Ok(crate::game::ChatPrivateResult::Success {
                target_name: "MockUser".to_string(),
                channel_tag: format!("_1_{}", target_uid.as_i32()),
                messages: vec![],
            })
        }
    }

    fn whois(
        &self,
        request: &crate::game::WhoisRequest,
    ) -> impl Future<Output = Result<crate::game::WhoisResult, PersistenceStoreFailure>> + Send
    {
        let ids = request.ids.clone();
        async move {
            Ok(crate::game::WhoisResult::Loaded {
                names: ids.into_iter().map(|id| (id, String::new())).collect(),
            })
        }
    }

    async fn clan_menu(
        &self,
        _request: &crate::game::ClanMenuRequest,
    ) -> Result<crate::game::ClanMenuResult, PersistenceStoreFailure> {
        Ok(crate::game::ClanMenuResult::Browse {
            invites: vec![],
            clans: vec![],
        })
    }

    async fn admin_money_all(
        &self,
        _request: &crate::game::AdminMoneyAllRequest,
    ) -> Result<u64, PersistenceStoreFailure> {
        Ok(1)
    }

    async fn admin_role(
        &self,
        request: &crate::game::AdminRoleRequest,
    ) -> Result<crate::game::AdminRoleResult, PersistenceStoreFailure> {
        Ok(crate::game::AdminRoleResult::Applied {
            target_id: crate::game::PlayerId::from(1),
            target_name: request.target_name.clone(),
        })
    }

    async fn admin_skill(
        &self,
        _request: &crate::game::AdminSkillRequest,
    ) -> Result<(), PersistenceStoreFailure> {
        Ok(())
    }

    async fn clan_command(
        &self,
        _request: &crate::game::ClanCommandRequest,
    ) -> Result<crate::game::ClanCommandResult, PersistenceStoreFailure> {
        Ok(crate::game::ClanCommandResult::Rejected {
            title: "Тест".to_string(),
            message: "Тестовый storage не реализует кланы".to_string(),
        })
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

    let (woken_tx, woken_rx) = std::sync::mpsc::channel();
    let (park_tx, park_rx) = std::sync::mpsc::channel();

    let test_waker = waker.clone();
    let handle_thread = std::thread::spawn(move || {
        test_waker.register_current();
        park_tx.send(()).unwrap();
        std::thread::park();
        woken_tx.send(()).unwrap();
    });

    park_rx.recv().unwrap();

    let mut runtime = PersistenceRuntime::start_with_store_and_waker(store.clone(), 4, waker);
    let mut completions = runtime.take_completion_receiver();
    let handle = runtime.handle();
    publish_program(&handle, 7, 11, 1);
    publish_program(&handle, 7, 12, 2);

    store.started.acquire().await.unwrap().forget();
    store.release.add_permits(1);
    store.started.acquire().await.unwrap().forget();
    assert!(completions.recv().await.is_some());

    let woken = woken_rx.recv_timeout(Duration::from_millis(200));
    assert!(
        woken.is_ok(),
        "simulation thread was not woken by persistence completion"
    );

    store.release.add_permits(1);
    drop(handle);
    runtime.shutdown().await;
    assert!(completions.recv().await.is_some());
    handle_thread.join().unwrap();
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

fn publish_program(handle: &PersistenceHandle, player_id: i32, session_id: u64, program_id: i32) {
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

fn publish_chat_color_cycle(handle: &PersistenceHandle, player_id: i32, session_id: u64) {
    handle
        .try_reserve(SaveKind::ChatColorCycle)
        .expect("chat color persistence capacity")
        .publish(SaveCommand::ChatColorCycle {
            request: crate::game::ChatColorCycleRequest {
                player_id: crate::game::PlayerId(player_id),
                session_id: crate::game::SessionId::new(session_id),
            },
        });
}

fn publish_auction_grid(handle: &PersistenceHandle, player_id: i32, session_id: u64) {
    handle
        .try_reserve(SaveKind::AuctionGrid)
        .expect("auction grid persistence capacity")
        .publish(SaveCommand::AuctionGrid {
            request: crate::game::AuctionGridRequest {
                player_id: crate::game::PlayerId(player_id),
                session_id: crate::game::SessionId::new(session_id),
                building_x: 12,
                building_y: 34,
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

#[tokio::test]
async fn chat_color_cycle_reserves_completion_and_publishes_result() {
    let store = TestStore::new(false, 0);
    let mut runtime = PersistenceRuntime::start_with_store(store.clone(), 1);
    let mut completions = runtime.take_completion_receiver();
    let handle = runtime.handle();
    publish_chat_color_cycle(&handle, 7, 11);

    while handle.backlog() != 0 {
        tokio::task::yield_now().await;
    }
    assert!(matches!(
        handle.try_reserve(SaveKind::ChatColorCycle),
        Err(PersistenceAdmissionError::Full)
    ));
    assert!(matches!(
        completions.try_recv(),
        Ok(crate::game::PersistenceCompletion::ChatColorCycled {
            request: crate::game::ChatColorCycleRequest { player_id, session_id },
            result: crate::game::ChatColorCycleResult::Cycled { color: 1 },
        }) if player_id == crate::game::PlayerId(7)
            && session_id == crate::game::SessionId::new(11)
    ));
    assert_eq!(
        *store.saved.lock().expect("saved lock"),
        vec![SavedBatch::ChatColor(7)]
    );

    drop(handle);
    runtime.shutdown().await;
}

#[tokio::test]
async fn chat_color_cycle_retries_transient_store_failure() {
    let store = TestStore::new(false, 2);
    let mut runtime = PersistenceRuntime::start_with_store(store.clone(), 1);
    let mut completions = runtime.take_completion_receiver();
    let handle = runtime.handle();
    publish_chat_color_cycle(&handle, 7, 11);
    drop(handle);

    runtime.shutdown().await;

    assert_eq!(store.calls.load(Ordering::Acquire), 3);
    assert!(matches!(
        completions.try_recv(),
        Ok(crate::game::PersistenceCompletion::ChatColorCycled {
            result: crate::game::ChatColorCycleResult::Cycled { color: 1 },
            ..
        })
    ));
}

#[tokio::test]
async fn auction_grid_reserves_completion_and_publishes_result() {
    let store = TestStore::new(false, 0);
    let mut runtime = PersistenceRuntime::start_with_store(store, 1);
    let mut completions = runtime.take_completion_receiver();
    let handle = runtime.handle();
    publish_auction_grid(&handle, 7, 11);

    while handle.backlog() != 0 {
        tokio::task::yield_now().await;
    }
    assert!(matches!(
        handle.try_reserve(SaveKind::AuctionGrid),
        Err(PersistenceAdmissionError::Full)
    ));
    assert!(matches!(
        completions.try_recv(),
        Ok(crate::game::PersistenceCompletion::AuctionGridLoaded {
            request: crate::game::AuctionGridRequest { player_id, session_id, building_x, building_y },
            result: crate::game::AuctionGridResult::Loaded { counts },
        }) if player_id == crate::game::PlayerId(7)
            && session_id == crate::game::SessionId::new(11)
            && building_x == 12
            && building_y == 34
            && counts.is_empty()
    ));

    drop(handle);
    runtime.shutdown().await;
}
