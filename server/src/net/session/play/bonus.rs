//! Ежедневный бонус (кнопка БОНУСЫ в клиенте → TY-событие `GDon`).
//!
//! НАМЕРЕННАЯ ДЕВИАЦИЯ от C# референса: в C# `GDonPacket` — заглушка (декод без
//! логики; донат там = реальные деньги, не реализован). По явному требованию
//! пользователя кнопка превращена в ежедневный бонус: клик → начисление раз в
//! кулдаун. Параметры (7 часов / 1 000 000 $) заданы пользователем.

use crate::game::player::{PlayerFlags, PlayerStats};
use crate::net::session::prelude::*;
use crate::net::session::social::commands::send_ok;
use crate::tasks::auction::now_unix;

/// Кулдаун между клеймами бонуса (7 часов, в секундах).
const BONUS_COOLDOWN_SECS: i64 = 7 * 3600;
/// Размер бонуса (деньги).
const BONUS_REWARD: i64 = 1_000_000;

/// Доступен ли бонус: прошло ≥ кулдауна с последнего клейма (0 = ни разу → да).
#[must_use]
pub fn bonus_available(last_bonus_at: i64) -> bool {
    now_unix() - last_bonus_at >= BONUS_COOLDOWN_SECS
}

/// Исход попытки клейма бонуса.
enum ClaimOutcome {
    /// Начислено; новые `money`/`creds` для пакета `P$`.
    Claimed { money: i64, creds: i64 },
    /// Кулдаун ещё не вышел; осталось секунд.
    NotReady { remaining: i64 },
}

fn send_bonus_state_error(tx: &mpsc::UnboundedSender<Vec<u8>>) {
    send_u_packet(
        tx,
        "OK",
        &ok_message("Бонус", "Состояние бонуса недоступно.").1,
    );
}

/// Обработка `GDon` (клик по кнопке БОНУСЫ). Sub-payload (`METHOD`) игнорируется.
pub fn handle_bonus_claim(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let now = now_unix();
    // Атомарно под ECS-локом: проверка кулдауна + начисление в одном замыкании,
    // чтобы спам `GDon` не дал двойной клейм. Начисляем в ECS (онлайн-игрок),
    // не прямым DB-write — иначе разойдётся с кэшем и flush затрёт.
    let outcome = state.modify_player(pid, |ecs, entity| {
        let Some(player_stats) = ecs.get::<PlayerStats>(entity) else {
            tracing::error!(player_id = %pid, component = "PlayerStats", "Player component missing for bonus claim");
            send_bonus_state_error(tx);
            return None;
        };
        if ecs.get::<PlayerFlags>(entity).is_none() {
            tracing::error!(player_id = %pid, component = "PlayerFlags", "Player component missing for bonus claim");
            send_bonus_state_error(tx);
            return None;
        }
        let last = player_stats.last_bonus_at;
        if now - last < BONUS_COOLDOWN_SECS {
            return Some(ClaimOutcome::NotReady {
                remaining: BONUS_COOLDOWN_SECS - (now - last),
            });
        }
        let (money, creds) = {
            let Some(mut pstats) = ecs.get_mut::<PlayerStats>(entity) else {
                tracing::error!(player_id = %pid, component = "PlayerStats", "Player component missing while applying bonus claim");
                send_bonus_state_error(tx);
                return None;
            };
            pstats.money += BONUS_REWARD;
            pstats.last_bonus_at = now;
            (pstats.money, pstats.creds)
        };
        let Some(mut f) = ecs.get_mut::<PlayerFlags>(entity) else {
            tracing::error!(player_id = %pid, component = "PlayerFlags", "Player component missing while applying bonus claim");
            send_bonus_state_error(tx);
            return None;
        };
        f.dirty = true;
        Some(ClaimOutcome::Claimed { money, creds })
    });

    match outcome.flatten() {
        Some(ClaimOutcome::Claimed {
            money: new_money,
            creds,
        }) => {
            send_u_packet(tx, "P$", &money(new_money, creds).1);
            send_u_packet(tx, "DR", b"0");
            let hours = BONUS_COOLDOWN_SECS / 3600;
            send_ok(
                tx,
                "Бонус",
                &format!("Вы получили {BONUS_REWARD}$!\nВозвращайтесь через {hours} часов."),
            );

            // Срочное сохранение в БД (write-through) для гарантированной сохранности при аварийном падении
            let row = state
                .modify_player(pid, |ecs, entity| {
                    crate::game::player::extract_player_row(ecs, entity)
                })
                .flatten();
            if let Some(r) = row {
                let db = state.db.clone();
                let state_c = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = db.save_player(&r).await {
                        tracing::error!(player_id = %pid, error = ?e, "Failed to write-through save player after daily bonus");
                    } else {
                        state_c.modify_player(pid, |ecs, entity| {
                            if let Some(mut flags) = ecs.get_mut::<crate::game::PlayerFlags>(entity)
                            {
                                flags.dirty = false;
                            }
                        });
                    }
                });
            }
        }
        Some(ClaimOutcome::NotReady { remaining }) => {
            let hours = remaining / 3600;
            let mins = (remaining % 3600) / 60;
            send_ok(
                tx,
                "Бонус",
                &format!("Бонус ещё не готов.\nПриходите через {hours}ч {mins}м."),
            );
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::mpsc::UnboundedReceiver;

    struct BonusTestState {
        state: Arc<GameState>,
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
            let _ = std::fs::remove_file(
                self.dir
                    .join(format!("{}_durability.mapb", self.world_name)),
            );
        }
    }

    async fn make_bonus_test_state(label: &str) -> BonusTestState {
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
        let config = crate::config::Config {
            world_name: world_name.clone(),
            port: 8090,
            world_chunks_w: 2,
            world_chunks_h: 2,
            data_dir: dir.to_string_lossy().to_string(),
            logging: crate::config::LoggingConfig::default(),
            cron: crate::config::CronConfig::default(),
            gameplay: crate::config::GameplayConfig::default(),
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

    fn drain_events(rx: &mut UnboundedReceiver<Vec<u8>>) -> Vec<(String, Vec<u8>)> {
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

    fn player_money(state: &Arc<GameState>, pid: PlayerId) -> i64 {
        state
            .query_player_opt(pid, |ecs, entity| {
                let player_stats = ecs.get::<PlayerStats>(entity)?;
                Some(player_stats.money)
            })
            .unwrap()
    }

    #[tokio::test]
    async fn bonus_missing_stats_is_explicit_error_not_silent_noop() {
        let test = make_bonus_test_state("missing_stats").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity).remove::<PlayerStats>();
        }

        handle_bonus_claim(&test.state, &tx, pid);

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние бонуса недоступно."));

        test.cleanup();
    }

    #[tokio::test]
    async fn bonus_missing_flags_is_explicit_error_without_reward_mutation() {
        let test = make_bonus_test_state("missing_flags").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let before_money = player_money(&test.state, pid);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity).remove::<PlayerFlags>();
        }

        handle_bonus_claim(&test.state, &tx, pid);

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние бонуса недоступно."));
        assert_eq!(player_money(&test.state, pid), before_money);

        test.cleanup();
    }
}
