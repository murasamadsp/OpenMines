use crate::game::{GameState, PlayerId};
use std::sync::Arc;

#[derive(Debug, Eq, PartialEq)]
pub enum CrystalSpendResult {
    Spent { crystals: [i64; 6] },
    Insufficient,
    MissingState(&'static str),
    MissingEntity,
}

pub fn spend_crystal(
    state: &Arc<GameState>,
    pid: PlayerId,
    idx: usize,
    amount: i64,
) -> CrystalSpendResult {
    state
        .modify_player(pid, |ecs, entity| {
            if ecs
                .get::<crate::game::player::PlayerStats>(entity)
                .is_none()
            {
                return Some(CrystalSpendResult::MissingState("PlayerStats"));
            }
            if ecs
                .get::<crate::game::player::PlayerFlags>(entity)
                .is_none()
            {
                return Some(CrystalSpendResult::MissingState("PlayerFlags"));
            }

            let mut player_stats = ecs
                .get_mut::<crate::game::player::PlayerStats>(entity)
                .expect("PlayerStats checked before crystal spend");
            if player_stats.crystals[idx] < amount {
                return Some(CrystalSpendResult::Insufficient);
            }
            player_stats.crystals[idx] -= amount;
            let crystals = player_stats.crystals;
            ecs.get_mut::<crate::game::player::PlayerFlags>(entity)
                .expect("PlayerFlags checked before crystal spend")
                .dirty = true;

            Some(CrystalSpendResult::Spent { crystals })
        })
        .flatten()
        .unwrap_or(CrystalSpendResult::MissingEntity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct CrystalTestState {
        state: Arc<GameState>,
        player: crate::db::PlayerRow,
        db_path: std::path::PathBuf,
        world_name: String,
        dir: std::path::PathBuf,
    }

    impl CrystalTestState {
        fn cleanup(&self) {
            let _ = std::fs::remove_file(&self.db_path);
            let _ = std::fs::remove_file(self.db_path.with_extension("db-wal"));
            let _ = std::fs::remove_file(self.db_path.with_extension("db-shm"));
            let _ = std::fs::remove_file(self.dir.join(format!("{}_v2.map", self.world_name)));
            let _ =
                std::fs::remove_file(self.dir.join(format!("{}_durability.map", self.world_name)));
        }
    }

    async fn make_crystal_test_state(label: &str) -> CrystalTestState {
        let dir = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_path = dir.join(format!(
            "crystal_{label}_{}_{}.db",
            std::process::id(),
            nonce
        ));
        let _ = std::fs::remove_file(&db_path);

        let database = crate::db::Database::open(&db_path).await.unwrap();
        let player = database
            .create_player("crystal-user", "p", "h")
            .await
            .unwrap();

        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();
        let world_name = format!("crystal_world_{label}_{}_{}", std::process::id(), nonce);
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

        let (tx, mut rx) = crate::net::session::outbox::channel();
        crate::net::session::player::init::connect_in_tick(&state, &tx, &player, 1);
        while rx.try_recv().is_ok() {}

        CrystalTestState {
            state,
            player,
            db_path,
            world_name,
            dir,
        }
    }

    #[tokio::test]
    async fn spend_crystal_updates_crystals_and_marks_dirty() {
        let test = make_crystal_test_state("spend").await;
        let pid = PlayerId(test.player.id);
        test.state.modify_player(pid, |ecs, entity| {
            ecs.get_mut::<crate::game::player::PlayerStats>(entity)
                .expect("test player stats")
                .crystals[0] = 3;
        });

        let result = spend_crystal(&test.state, pid, 0, 2);

        assert_eq!(
            result,
            CrystalSpendResult::Spent {
                crystals: [1, 0, 0, 0, 0, 0],
            }
        );
        let (crystals, dirty) = test
            .state
            .query_player_opt(pid, |ecs, entity| {
                let stats = ecs.get::<crate::game::player::PlayerStats>(entity)?;
                let flags = ecs.get::<crate::game::player::PlayerFlags>(entity)?;
                Some((stats.crystals, flags.dirty))
            })
            .unwrap();
        assert_eq!(crystals[0], 1);
        assert!(dirty);

        test.cleanup();
    }

    #[tokio::test]
    async fn spend_crystal_insufficient_is_silent_without_dirty() {
        let test = make_crystal_test_state("insufficient").await;
        let pid = PlayerId(test.player.id);
        let result = spend_crystal(&test.state, pid, 0, 1);

        assert_eq!(result, CrystalSpendResult::Insufficient);
        let dirty = test
            .state
            .query_player_opt(pid, |ecs, entity| {
                Some(ecs.get::<crate::game::player::PlayerFlags>(entity)?.dirty)
            })
            .unwrap();
        assert!(!dirty);

        test.cleanup();
    }
}
