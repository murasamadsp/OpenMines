#[cfg(test)]
mod tests {
    use crate::game::programmator::ProgrammatorState;
    use crate::game::{GameState, PlayerId};
    use crate::test_support::{ServerTestHarness, drain_events};
    use std::sync::Arc;

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
        let test = ServerTestHarness::new("geo_missing_programmator", "geo-user").await;
        let mut rx = test.connect(1);
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
    }

    #[tokio::test]
    async fn geo_without_installed_skill_stays_quiet_noop() {
        let test = ServerTestHarness::new("geo_no_skill_quiet", "geo-user").await;
        let mut rx = test.connect(1);
        drain_events(&mut rx);

        apply_geo_command(&test.state, PlayerId(test.player.id));

        let events = drain_events(&mut rx);
        assert!(events.is_empty());
    }
}
