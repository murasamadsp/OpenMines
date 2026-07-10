//! Ежедневный бонус (кнопка БОНУСЫ в клиенте → TY-событие `GDon`).
//!
//! НАМЕРЕННАЯ ДЕВИАЦИЯ от C# референса: в C# `GDonPacket` — заглушка (декод без
//! логики; донат там = реальные деньги, не реализован). По явному требованию
//! пользователя кнопка превращена в ежедневный бонус: клик → начисление раз в
//! кулдаун. Параметры лежат в `gameplay.bonus`.

#[cfg(test)]
mod tests {
    use bytes::BytesMut;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::mpsc::Receiver;

    struct BonusTestState {
        state: Arc<crate::game::GameState>,
        player: crate::db::PlayerRow,
        db_path: std::path::PathBuf,
        world_name: String,
        dir: std::path::PathBuf,
    }

    impl BonusTestState {
        fn cleanup(&self) {
            let _ = std::fs::remove_file(&self.db_path);
            let _ = std::fs::remove_file(self.db_path.with_extension("db-wal"));
            let _ = std::fs::remove_file(self.db_path.with_extension("db-shm"));
            let _ = std::fs::remove_file(self.dir.join(format!("{}_v2.map", self.world_name)));
            let _ =
                std::fs::remove_file(self.dir.join(format!("{}_durability.map", self.world_name)));
        }
    }

    async fn make_bonus_test_state(label: &str) -> BonusTestState {
        make_bonus_test_state_with_bonus(label, crate::config::BonusConfig::runtime_baseline())
            .await
    }

    async fn make_bonus_test_state_with_bonus(
        label: &str,
        bonus: crate::config::BonusConfig,
    ) -> BonusTestState {
        let dir = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_path = dir.join(format!("bonus_{label}_{}_{}.db", std::process::id(), nonce));
        let _ = std::fs::remove_file(&db_path);

        let database = crate::db::Database::open(&db_path).await.unwrap();
        let player = database
            .create_player("bonus-user", "p", "h")
            .await
            .unwrap();

        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();
        let world_name = format!("bonus_world_{label}_{}_{}", std::process::id(), nonce);
        let world = crate::world::World::new(&world_name, 2, 2, cell_defs, &dir).unwrap();
        let gameplay = crate::config::GameplayConfig {
            bonus,
            ..crate::config::GameplayConfig::runtime_baseline()
        };
        let config = crate::config::Config {
            world_name: world_name.clone(),
            port: 8090,
            world_chunks_w: 2,
            world_chunks_h: 2,
            data_dir: dir.to_string_lossy().to_string(),
            logging: crate::config::LoggingConfig::runtime_baseline(),
            cron: crate::config::CronConfig::runtime_baseline(),
            gameplay,
        };
        let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config)
            .await
            .unwrap();

        BonusTestState {
            state,
            player,
            db_path,
            world_name,
            dir,
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

    fn player_money(state: &Arc<crate::game::GameState>, pid: crate::game::PlayerId) -> i64 {
        state
            .query_player_opt(pid, |ecs, entity| {
                let player_stats = ecs.get::<crate::game::player::PlayerStats>(entity)?;
                Some(player_stats.money)
            })
            .unwrap()
    }

    fn claim_bonus(
        state: &Arc<crate::game::GameState>,
        pid: crate::game::PlayerId,
    ) -> crate::game::CommandEffects {
        crate::game::logic::commands::apply_player_command(
            state,
            crate::game::PlayerCommand::ClaimBonus { player_id: pid },
        )
    }

    #[tokio::test]
    async fn bonus_claim_uses_config_reward() {
        let reward_money = 42_424;
        let test = make_bonus_test_state_with_bonus(
            "configured_reward",
            crate::config::BonusConfig {
                cooldown_secs: 3_600,
                reward_money,
            },
        )
        .await;
        let (tx, mut rx) = crate::net::session::outbox::channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = crate::game::PlayerId(test.player.id);
        let before_money = player_money(&test.state, pid);

        let effects = claim_bonus(&test.state, pid);

        let events = drain_events(&mut rx);
        assert!(events.iter().any(|(event, _)| event == "P$"));
        assert_eq!(player_money(&test.state, pid), before_money + reward_money);
        assert!(matches!(
            effects.saves.as_slice(),
            [crate::game::SaveCommand::SavePlayer { row }] if row.id == test.player.id
        ));

        test.cleanup();
    }

    #[test]
    fn bonus_claim_from_plain_thread_returns_durable_effect() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let test = rt.block_on(make_bonus_test_state_with_bonus(
            "non_tokio_context",
            crate::config::BonusConfig {
                cooldown_secs: 3_600,
                reward_money: 7_777,
            },
        ));
        let (tx, mut rx) = crate::net::session::outbox::channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = crate::game::PlayerId(test.player.id);
        let effects = claim_bonus(&test.state, pid);

        let events = drain_events(&mut rx);
        assert!(events.iter().any(|(event, _)| event == "P$"));
        assert_eq!(effects.saves.len(), 1);

        rt.block_on(async {
            tokio::task::yield_now().await;
        });
        test.cleanup();
    }

    #[tokio::test]
    async fn bonus_missing_stats_is_explicit_error_not_silent_noop() {
        let test = make_bonus_test_state("missing_stats").await;
        let (tx, mut rx) = crate::net::session::outbox::channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = crate::game::PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerStats>();
        }

        let effects = claim_bonus(&test.state, pid);

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние бонуса недоступно."));
        assert!(effects.saves.is_empty());

        test.cleanup();
    }

    #[tokio::test]
    async fn bonus_missing_flags_is_explicit_error_without_reward_mutation() {
        let test = make_bonus_test_state("missing_flags").await;
        let (tx, mut rx) = crate::net::session::outbox::channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = crate::game::PlayerId(test.player.id);
        let before_money = player_money(&test.state, pid);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerFlags>();
        }

        let effects = claim_bonus(&test.state, pid);

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние бонуса недоступно."));
        assert_eq!(player_money(&test.state, pid), before_money);
        assert!(effects.saves.is_empty());

        test.cleanup();
    }
}
