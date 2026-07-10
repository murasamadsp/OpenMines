#[cfg(test)]
mod tests {
    use crate::game::programmator::ProgrammatorState;
    use crate::game::{GameState, PlayerId};
    use bytes::BytesMut;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::mpsc::Receiver;

    struct TestState {
        state: Arc<GameState>,
        player: crate::db::PlayerRow,
        world_name: String,
        db_path: std::path::PathBuf,
    }

    impl TestState {
        fn cleanup(&self) {
            let dir = std::env::temp_dir();
            let _ = std::fs::remove_file(&self.db_path);
            let _ = std::fs::remove_file(self.db_path.with_extension("db-wal"));
            let _ = std::fs::remove_file(self.db_path.with_extension("db-shm"));
            let _ = std::fs::remove_file(dir.join(format!("{}_v2.map", self.world_name)));
            let _ = std::fs::remove_file(dir.join(format!("{}_durability.map", self.world_name)));
        }
    }

    async fn make_test_state(label: &str) -> TestState {
        let dir = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_path = dir.join(format!("{label}_{}_{}.db", std::process::id(), nonce));
        let _ = std::fs::remove_file(&db_path);
        let database = crate::db::Database::open(&db_path).await.unwrap();
        let player = database.create_player("geo-user", "p", "h").await.unwrap();

        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();
        let world_name = format!("{label}_world_{}_{}", std::process::id(), nonce);
        let world = crate::world::World::new(&world_name, 2, 2, cell_defs, &dir).unwrap();
        let config = crate::config::Config {
            world_name: world_name.clone(),
            port: 8090,
            world_chunks_w: 2,
            world_chunks_h: 2,
            data_dir: dir.to_string_lossy().to_string(),
            logging: crate::config::LoggingConfig::runtime_baseline(),
            cron: crate::config::CronConfig::runtime_baseline(),
            gameplay: crate::config::GameplayConfig::runtime_baseline(),
        };
        let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config)
            .await
            .unwrap();

        TestState {
            state,
            player,
            world_name,
            db_path,
        }
    }

    fn drain_events(rx: &mut Receiver<Vec<u8>>) -> Vec<(String, Vec<u8>)> {
        let mut events = Vec::new();
        while let Ok(frame) = rx.try_recv() {
            let mut buf = BytesMut::from(&frame[..]);
            let packet = crate::protocol::Packet::try_decode(&mut buf)
                .expect("valid packet")
                .expect("decoded packet");
            events.push((packet.event_str().to_owned(), packet.payload.to_vec()));
        }
        events
    }

    fn apply_geo_command(state: &Arc<GameState>, pid: PlayerId) {
        crate::game::logic::commands::apply_player_command(
            state,
            crate::game::PlayerCommand::Geology {
                player_id: pid,
                programmatic: false,
            },
        );
    }

    #[tokio::test]
    async fn geo_missing_programmator_state_is_explicit_error_not_not_running_fallback() {
        let test = make_test_state("geo_missing_programmator").await;
        let (tx, mut rx) = crate::net::session::outbox::channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity).remove::<ProgrammatorState>();
        }

        apply_geo_command(&test.state, pid);

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));

        test.cleanup();
    }

    #[tokio::test]
    async fn geo_without_installed_skill_stays_quiet_noop() {
        let test = make_test_state("geo_no_skill_quiet").await;
        let (tx, mut rx) = crate::net::session::outbox::channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        apply_geo_command(&test.state, PlayerId(test.player.id));

        let events = drain_events(&mut rx);
        assert!(events.is_empty());

        test.cleanup();
    }
}
