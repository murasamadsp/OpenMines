//! Ежедневный бонус (кнопка БОНУСЫ в клиенте → TY-событие `GDon`).
//!
//! НАМЕРЕННАЯ ДЕВИАЦИЯ от C# референса: в C# `GDonPacket` — заглушка (декод без
//! логики; донат там = реальные деньги, не реализован). По явному требованию
//! пользователя кнопка превращена в ежедневный бонус: клик → начисление раз в
//! кулдаун. Параметры лежат в `gameplay.bonus`.

#[cfg(test)]
mod tests {
    use crate::test_support::{ServerTestHarness, drain_events};
    use std::sync::Arc;

    async fn make_bonus_test_state(label: &str) -> ServerTestHarness {
        ServerTestHarness::new(&format!("bonus_{label}"), "bonus-user").await
    }

    async fn make_bonus_test_state_with_bonus(
        label: &str,
        bonus: crate::config::BonusConfig,
    ) -> ServerTestHarness {
        let gameplay = crate::config::GameplayConfig {
            bonus,
            ..crate::config::GameplayConfig::runtime_baseline()
        };
        ServerTestHarness::with_gameplay(&format!("bonus_{label}"), "bonus-user", gameplay).await
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
        let mut rx = test.connect(1);
        drain_events(&mut rx);

        let pid = crate::game::PlayerId(test.player.id);
        let before_money = player_money(&test.state, pid);

        let effects = claim_bonus(&test.state, pid);

        let events = drain_events(&mut rx);
        assert!(events.iter().any(|(event, _)| event == "P$"));
        assert_eq!(player_money(&test.state, pid), before_money + reward_money);
        assert!(matches!(
            effects.saves.as_slice(),
            [crate::game::SaveCommand::Player { row }] if row.id == test.player.id
        ));
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
        let mut rx = test.connect(1);
        drain_events(&mut rx);

        let pid = crate::game::PlayerId(test.player.id);
        let effects = claim_bonus(&test.state, pid);

        let events = drain_events(&mut rx);
        assert!(events.iter().any(|(event, _)| event == "P$"));
        assert_eq!(effects.saves.len(), 1);

        rt.block_on(async {
            tokio::task::yield_now().await;
        });
    }

    #[tokio::test]
    async fn bonus_missing_stats_is_explicit_error_not_silent_noop() {
        let test = make_bonus_test_state("missing_stats").await;
        let mut rx = test.connect(1);
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
    }

    #[tokio::test]
    async fn bonus_missing_flags_is_explicit_error_without_reward_mutation() {
        let test = make_bonus_test_state("missing_flags").await;
        let mut rx = test.connect(1);
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
    }
}
