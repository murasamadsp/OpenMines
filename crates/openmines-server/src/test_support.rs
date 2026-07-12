use bytes::BytesMut;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Once;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::Receiver;

pub struct ServerTestHarness {
    pub(crate) state: Arc<crate::game::GameState>,
    pub(crate) player: crate::db::PlayerRow,
    dir: PathBuf,
    db_path: PathBuf,
    world_name: String,
}

pub struct ServerTestHarnessBuilder {
    database: Arc<crate::db::Database>,
    pub(crate) player: crate::db::PlayerRow,
    dir: PathBuf,
    db_path: PathBuf,
    world_name: String,
    gameplay: crate::config::GameplayConfig,
    world_chunks_w: u32,
    world_chunks_h: u32,
}

impl ServerTestHarness {
    pub(crate) async fn new(label: &str, username: &str) -> Self {
        Self::with_gameplay(
            label,
            username,
            crate::config::GameplayConfig::runtime_baseline(),
        )
        .await
    }

    pub(crate) async fn with_gameplay(
        label: &str,
        username: &str,
        gameplay: crate::config::GameplayConfig,
    ) -> Self {
        let mut builder = ServerTestHarnessBuilder::new(label, username).await;
        builder.gameplay = gameplay;
        builder.build().await
    }

    pub(crate) fn connect(&self, session_id: u64) -> Receiver<Vec<u8>> {
        self.connect_with_outbox(session_id).1
    }

    pub(crate) async fn create_player(&self, username: &str) -> crate::db::PlayerRow {
        self.state
            .db
            .create_player(username, "p", "h")
            .await
            .expect("create additional test player")
    }

    pub(crate) fn connect_with_outbox(
        &self,
        session_id: u64,
    ) -> (crate::net::session::outbox::Outbox, Receiver<Vec<u8>>) {
        self.connect_player_with_outbox(&self.player, session_id)
    }

    pub(crate) fn connect_player_with_outbox(
        &self,
        player: &crate::db::PlayerRow,
        session_id: u64,
    ) -> (crate::net::session::outbox::Outbox, Receiver<Vec<u8>>) {
        let (outbox, receiver) = crate::net::session::outbox::channel();
        crate::net::session::player::init::connect_in_tick(
            &self.state,
            &outbox,
            player,
            session_id,
        );
        (outbox, receiver)
    }

    pub(crate) fn drain_events(receiver: &mut Receiver<Vec<u8>>) -> Vec<(String, Vec<u8>)> {
        drain_events(receiver)
    }
}

impl Drop for ServerTestHarness {
    fn drop(&mut self) {
        cleanup_files(&self.dir, &self.db_path, &self.world_name);
    }
}

impl ServerTestHarnessBuilder {
    pub(crate) async fn new(label: &str, username: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after Unix epoch")
            .as_nanos();
        let unique = format!("{label}_{}_{}", std::process::id(), nonce);
        let dir = std::env::temp_dir();
        let db_path = dir.join(format!("{unique}.db"));
        let database = crate::db::Database::open(&db_path)
            .await
            .expect("open test database");
        let player = database
            .create_player(username, "p", "h")
            .await
            .expect("create test player");
        Self {
            database: Arc::new(database),
            player,
            dir,
            db_path,
            world_name: format!("{unique}_world"),
            gameplay: crate::config::GameplayConfig::runtime_baseline(),
            world_chunks_w: 2,
            world_chunks_h: 2,
        }
    }

    pub(crate) fn database(&self) -> &crate::db::Database {
        &self.database
    }

    pub(crate) async fn create_player(&self, username: &str) -> crate::db::PlayerRow {
        self.database
            .create_player(username, "p", "h")
            .await
            .expect("create additional test player")
    }

    pub(crate) fn world_chunks(&mut self, width: u32, height: u32) {
        self.world_chunks_w = width;
        self.world_chunks_h = height;
    }

    pub(crate) async fn build(self) -> ServerTestHarness {
        ensure_buildings_config();
        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .expect("load test cell definitions");
        let world = crate::world::World::new(
            &self.world_name,
            self.world_chunks_w,
            self.world_chunks_h,
            cell_defs,
            &self.dir,
        )
        .expect("create test world");
        let config = crate::config::Config {
            world_name: self.world_name.clone(),
            port: 8090,
            world_chunks_w: self.world_chunks_w,
            world_chunks_h: self.world_chunks_h,
            data_dir: self.dir.to_string_lossy().into_owned(),
            logging: crate::config::LoggingConfig::runtime_baseline(),
            cron: crate::config::CronConfig::runtime_baseline(),
            gameplay: self.gameplay,
        };
        let state = crate::game::GameState::new(Arc::new(world), self.database, config)
            .await
            .expect("create test game state");

        ServerTestHarness {
            state,
            player: self.player,
            dir: self.dir,
            db_path: self.db_path,
            world_name: self.world_name,
        }
    }
}

fn ensure_buildings_config() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        if crate::game::buildings::get_building_config(crate::game::buildings::PackType::Market)
            .is_none()
        {
            let _ = crate::game::buildings::load_buildings_config(crate::test_config_path(
                "configs/buildings.json",
            ));
        }
    });
}

pub fn drain_events(receiver: &mut Receiver<Vec<u8>>) -> Vec<(String, Vec<u8>)> {
    let mut events = Vec::new();
    while let Ok(frame) = receiver.try_recv() {
        let mut bytes = BytesMut::from(frame.as_slice());
        let packet = crate::protocol::Packet::try_decode(&mut bytes)
            .expect("valid test packet")
            .expect("complete test packet");
        events.push((packet.event_str().to_owned(), packet.payload.to_vec()));
    }
    events
}

fn cleanup_files(dir: &std::path::Path, db_path: &std::path::Path, world_name: &str) {
    for path in [
        db_path.to_path_buf(),
        db_path.with_extension("db-wal"),
        db_path.with_extension("db-shm"),
        dir.join(format!("{world_name}_v2.map")),
        dir.join(format!("{world_name}_road_v2.map")),
        dir.join(format!("{world_name}_durability.map")),
        dir.join(format!("{world_name}_world.journal")),
    ] {
        let _ = std::fs::remove_file(path);
    }
}
