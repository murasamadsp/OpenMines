use crate::game::{GameState, PlayerId};
use crate::world::WorldProvider;
use std::sync::Arc;

pub enum HazardBoxPickupResult {
    Picked {
        save: crate::game::SaveCommand,
        broadcasts: Vec<crate::game::BroadcastEffect>,
    },
    Stale,
}

pub fn apply_hazard_box_pickup(
    state: &Arc<GameState>,
    intent: crate::game::BoxPickupIntent,
) -> HazardBoxPickupResult {
    state
        .modify_player(intent.player_id, |ecs, entity| {
            let Some(pos) = ecs.get::<crate::game::player::PlayerPosition>(entity) else {
                return Some(HazardBoxPickupResult::Stale);
            };
            if crate::game::WorldPos::from((pos.x, pos.y)) != intent.pos {
                return Some(HazardBoxPickupResult::Stale);
            }
            if ecs
                .get::<crate::game::player::PlayerStats>(entity)
                .is_none()
                || ecs
                    .get::<crate::game::player::PlayerFlags>(entity)
                    .is_none()
            {
                return Some(HazardBoxPickupResult::Stale);
            }

            let (x, y): (i32, i32) = intent.pos.into();
            if state.world.get_cell_typed(x, y)
                != crate::world::CellType(crate::world::cells::cell_type::BOX)
            {
                return Some(HazardBoxPickupResult::Stale);
            }
            let Some(picked) = state.remove_box_cell_authoritative(x, y) else {
                return Some(HazardBoxPickupResult::Stale);
            };

            let crystals = {
                let mut player_stats = ecs
                    .get_mut::<crate::game::player::PlayerStats>(entity)
                    .expect("PlayerStats checked before hazard box pickup");
                for (slot, amount) in player_stats.crystals.iter_mut().zip(picked) {
                    *slot = slot.saturating_add(amount);
                }
                player_stats.crystals
            };
            ecs.get_mut::<crate::game::player::PlayerFlags>(entity)
                .expect("PlayerFlags checked before hazard box pickup")
                .dirty = true;

            let mut broadcasts = vec![crate::game::BroadcastEffect::CellUpdate(intent.pos)];
            if let Some(connection) = ecs.get::<crate::game::player::PlayerConnection>(entity) {
                broadcasts.push(crate::game::BroadcastEffect::Direct {
                    session_id: connection.session_id,
                    data: crate::net::session::wire::make_u_packet_bytes(
                        "@B",
                        &crate::protocol::packets::basket(&crystals, 1).1,
                    ),
                });
            }

            Some(HazardBoxPickupResult::Picked {
                save: crate::game::SaveCommand::Box {
                    write: crate::db::BoxWrite {
                        x,
                        y,
                        crystals: None,
                    },
                },
                broadcasts,
            })
        })
        .flatten()
        .unwrap_or(HazardBoxPickupResult::Stale)
}

#[derive(Debug, Eq, PartialEq)]
pub enum BoxPickupResult {
    Picked {
        picked: [i64; 6],
        crystals: [i64; 6],
    },
    Empty,
    MissingState(&'static str),
    MissingEntity,
}

pub fn pickup_box(state: &Arc<GameState>, pid: PlayerId, x: i32, y: i32) -> BoxPickupResult {
    state
        .modify_player(pid, |ecs, entity| {
            if ecs
                .get::<crate::game::player::PlayerStats>(entity)
                .is_none()
            {
                return Some(BoxPickupResult::MissingState("PlayerStats"));
            }
            if ecs
                .get::<crate::game::player::PlayerFlags>(entity)
                .is_none()
            {
                return Some(BoxPickupResult::MissingState("PlayerFlags"));
            }

            let Some(picked) = state.remove_box_cell(x, y) else {
                return Some(BoxPickupResult::Empty);
            };

            let mut player_stats = ecs
                .get_mut::<crate::game::player::PlayerStats>(entity)
                .expect("PlayerStats checked before box pickup");
            for (slot, amount) in player_stats.crystals.iter_mut().zip(picked) {
                *slot = slot.saturating_add(amount);
            }
            let crystals = player_stats.crystals;
            ecs.get_mut::<crate::game::player::PlayerFlags>(entity)
                .expect("PlayerFlags checked before box pickup")
                .dirty = true;

            Some(BoxPickupResult::Picked { picked, crystals })
        })
        .flatten()
        .unwrap_or(BoxPickupResult::MissingEntity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::WorldProvider;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct BoxTestState {
        state: Arc<GameState>,
        player: crate::db::PlayerRow,
        db_path: std::path::PathBuf,
        world_name: String,
        dir: std::path::PathBuf,
    }

    impl BoxTestState {
        fn cleanup(&self) {
            let _ = std::fs::remove_file(&self.db_path);
            let _ = std::fs::remove_file(self.db_path.with_extension("db-wal"));
            let _ = std::fs::remove_file(self.db_path.with_extension("db-shm"));
            let _ = std::fs::remove_file(self.dir.join(format!("{}_v2.map", self.world_name)));
            let _ =
                std::fs::remove_file(self.dir.join(format!("{}_durability.map", self.world_name)));
        }
    }

    async fn make_box_test_state(label: &str) -> BoxTestState {
        let dir = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_path = dir.join(format!("box_{label}_{}_{}.db", std::process::id(), nonce));
        let _ = std::fs::remove_file(&db_path);

        let database = crate::db::Database::open(&db_path).await.unwrap();
        let player = database.create_player("box-user", "p", "h").await.unwrap();

        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();
        let world_name = format!("box_world_{label}_{}_{}", std::process::id(), nonce);
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

        BoxTestState {
            state,
            player,
            db_path,
            world_name,
            dir,
        }
    }

    #[tokio::test]
    async fn pickup_box_updates_crystals_marks_dirty_and_removes_box() {
        let test = make_box_test_state("pickup").await;
        let pid = PlayerId(test.player.id);
        test.state.put_box_cell(10, 11, [3, 2, 1, 0, 0, 0]);

        let result = pickup_box(&test.state, pid, 10, 11);

        assert_eq!(
            result,
            BoxPickupResult::Picked {
                picked: [3, 2, 1, 0, 0, 0],
                crystals: [3, 2, 1, 0, 0, 0],
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
        assert_eq!(crystals, [3, 2, 1, 0, 0, 0]);
        assert!(dirty);
        assert_eq!(test.state.box_take(10, 11), None);
        assert_eq!(
            test.state.world.get_cell(10, 11),
            crate::world::cells::cell_type::EMPTY
        );

        test.cleanup();
    }

    #[tokio::test]
    async fn pickup_box_missing_flags_keeps_box_and_player_crystals() {
        let test = make_box_test_state("missing_flags").await;
        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerFlags>();
        }
        test.state.put_box_cell(10, 11, [3, 2, 1, 0, 0, 0]);

        let result = pickup_box(&test.state, pid, 10, 11);

        assert_eq!(result, BoxPickupResult::MissingState("PlayerFlags"));
        let crystals = test
            .state
            .query_player_opt(pid, |ecs, entity| {
                Some(
                    ecs.get::<crate::game::player::PlayerStats>(entity)?
                        .crystals,
                )
            })
            .unwrap();
        assert_eq!(crystals, [0; 6]);
        assert_eq!(test.state.box_take(10, 11), Some([3, 2, 1, 0, 0, 0]));

        test.cleanup();
    }
}
