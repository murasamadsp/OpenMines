#[allow(clippy::module_inception)]
#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::test_support::{ServerTestHarness, ServerTestHarnessBuilder, drain_events};

    async fn make_test_state(label: &str) -> ServerTestHarness {
        let mut builder = ServerTestHarnessBuilder::new(label, "move-user").await;
        builder.player.x = 10;
        builder.player.y = 10;
        builder.build().await
    }

    #[tokio::test]
    async fn stale_session_move_cannot_mutate_reconnected_player() {
        let test = make_test_state("stale_session_move").await;
        let (_tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);
        let pid = PlayerId(test.player.id);

        let effects = crate::game::logic::commands::apply_player_command(
            &test.state,
            pid,
            crate::game::SessionId::new(2),
            crate::game::PlayerCommand::Move {
                time: 0,
                x: 11,
                y: 10,
                direction: 3,
                programmatic: false,
            },
        );

        assert!(effects.events.is_empty());
        assert!(effects.saves.is_empty());
        assert_eq!(
            test.state.query_player(pid, |ecs, entity| {
                let position = ecs.get::<PlayerPosition>(entity)?;
                Some((position.x, position.y))
            }),
            Some(Some((10, 10)))
        );
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn manual_move_returns_output_without_sending_during_dispatch() {
        let test = make_test_state("manual_move_effect").await;
        test.state.world.set_cell(11, 10, cell_type::EMPTY);
        let session_id = crate::game::SessionId::new(1);
        let (_tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);
        let pid = PlayerId(test.player.id);

        let effects = crate::game::logic::commands::apply_player_command(
            &test.state,
            pid,
            session_id,
            crate::game::PlayerCommand::Move {
                time: 0,
                x: 11,
                y: 10,
                direction: 3,
                programmatic: false,
            },
        );

        assert_eq!(
            test.state.query_player(pid, |ecs, entity| {
                let position = ecs.get::<PlayerPosition>(entity)?;
                Some((position.x, position.y))
            }),
            Some(Some((11, 10)))
        );
        assert!(!effects.events.is_empty());
        assert!(effects.saves.is_empty());
        assert!(rx.try_recv().is_err(), "dispatch must not write to outbox");

        for event in effects.events {
            match event {
                crate::game::GameEvent::PlayerInit { .. } => {
                    panic!("ordinary move cannot produce player init")
                }
                crate::game::GameEvent::SessionBatch {
                    session_id,
                    player_id,
                    packets,
                } => crate::game::logic::player_init::deliver_initial_presentation(
                    &test.state,
                    session_id,
                    player_id,
                    packets,
                ),
                crate::game::GameEvent::Fanout { recipients, data }
                | crate::game::GameEvent::MovementFanout {
                    recipients, data, ..
                } => {
                    test.state.sessions.fanout(&recipients, &data);
                }
                crate::game::GameEvent::GuiView { .. }
                | crate::game::GameEvent::RefreshChunks { .. }
                | crate::game::GameEvent::ChatFanout { .. }
                | crate::game::GameEvent::WorldEffects { .. } => {
                    panic!("ordinary move cannot produce this event")
                }
            }
        }
        assert!(
            drain_events(&mut rx).iter().any(|(event, _)| event == "HB"),
            "presentation delivery must emit the movement HB"
        );
    }

    #[tokio::test]
    async fn moving_onto_teleport_returns_gui_view_without_direct_delivery() {
        let test = make_test_state("move_teleport_gui_effect").await;
        test.state.world.set_cell(11, 10, cell_type::EMPTY);
        let extra = crate::db::BuildingExtra {
            charge: 100,
            max_charge: 100,
            cost: 0,
            hp: 1_000,
            max_hp: 1_000,
            money_inside: 0,
            crystals_inside: [0; 6],
            items_inside: std::collections::HashMap::new(),
            craft_recipe_id: None,
            craft_num: 0,
            craft_end_ts: 0,
            craft_ready: false,
            clanzone: 0,
        };
        let spec = crate::game::BuildingInsertSpec {
            type_code: "T",
            pack_type: PackType::Teleport,
            x: 11,
            y: 10,
            owner_id: PlayerId(test.player.id),
            clan_id: 0,
            extra: &extra,
        };
        test.state.insert_building_runtime(&spec).await.unwrap();

        let session_id = crate::game::SessionId::new(1);
        let (_tx, mut rx) = test.connect_with_outbox(session_id.get());
        drain_events(&mut rx);
        let player_id = PlayerId(test.player.id);

        let effects = crate::game::logic::commands::apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Move {
                time: 0,
                x: 11,
                y: 10,
                direction: 3,
                programmatic: false,
            },
        );

        assert!(rx.try_recv().is_err(), "dispatch must not write GUI wire");
        assert!(matches!(
            effects.events.last(),
            Some(crate::game::GameEvent::GuiView {
                session_id: event_session,
                player_id: event_player,
                view: crate::game::GuiView::Teleport(_),
            }) if *event_session == session_id && *event_player == player_id
        ));
    }

    #[tokio::test]
    async fn moving_onto_owned_spot_returns_gui_view_without_direct_delivery() {
        let test = make_test_state("move_spot_gui_effect").await;
        test.state.world.set_cell(11, 10, cell_type::EMPTY);
        let extra = crate::db::BuildingExtra {
            charge: 0,
            max_charge: 0,
            cost: 0,
            hp: 1_000,
            max_hp: 1_000,
            money_inside: 0,
            crystals_inside: [0; 6],
            items_inside: std::collections::HashMap::new(),
            craft_recipe_id: None,
            craft_num: 0,
            craft_end_ts: 0,
            craft_ready: false,
            clanzone: 0,
        };
        let spec = crate::game::BuildingInsertSpec {
            type_code: "O",
            pack_type: PackType::Spot,
            x: 11,
            y: 10,
            owner_id: PlayerId(test.player.id),
            clan_id: 0,
            extra: &extra,
        };
        test.state.insert_building_runtime(&spec).await.unwrap();

        let session_id = crate::game::SessionId::new(1);
        let (_tx, mut rx) = test.connect_with_outbox(session_id.get());
        drain_events(&mut rx);
        let player_id = PlayerId(test.player.id);

        let effects = crate::game::logic::commands::apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Move {
                time: 0,
                x: 11,
                y: 10,
                direction: 3,
                programmatic: false,
            },
        );

        assert!(rx.try_recv().is_err(), "dispatch must not write GUI wire");
        assert!(matches!(
            effects.events.last(),
            Some(crate::game::GameEvent::GuiView {
                session_id: event_session,
                player_id: event_player,
                view: crate::game::GuiView::Spot(_),
            }) if *event_session == session_id && *event_player == player_id
        ));
    }

    #[tokio::test]
    async fn move_missing_position_is_explicit_error_not_silent_reject() {
        let test = make_test_state("move_missing_position").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity).remove::<PlayerPosition>();
        }

        handle_move(&test.state, &tx, pid, 0, 11, 10, 3, false);

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));
    }

    #[tokio::test]
    async fn move_distance_reject_stays_tp_back_without_state_error() {
        let test = make_test_state("move_distance_reject").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        handle_move(
            &test.state,
            &tx,
            PlayerId(test.player.id),
            0,
            15,
            10,
            3,
            false,
        );

        // @T is a legitimate gameplay reject. No OK state error should appear.
        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "@T");
        assert_eq!(events[0].1, b"10:10");
    }

    #[tokio::test]
    async fn programmatic_move_reject_does_not_send_tp_back() {
        let test = make_test_state("programmatic_move_distance_reject").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        handle_move(
            &test.state,
            &tx,
            PlayerId(test.player.id),
            0,
            15,
            10,
            3,
            true,
        );

        let events = drain_events(&mut rx);
        assert!(
            events.iter().all(|(event, _)| event != "@T"),
            "server-driven programmator move must not rubber-band the client"
        );
    }

    #[tokio::test]
    async fn programmatic_autodig_sends_self_hb_without_tp_back() {
        let test = make_test_state("programmatic_autodig_self_hb").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        test.state.modify_player(pid, |ecs, entity| {
            ecs.get_mut::<crate::game::player::PlayerSettings>(entity)?
                .auto_dig = true;
            Some(())
        });
        test.state.world.set_cell(10, 11, cell_type::ROCK);
        test.state.world.set_durability(10, 11, 0.0);

        handle_move(&test.state, &tx, pid, 0, 10, 11, -1, true);

        assert_eq!(test.state.world.get_cell(10, 11), cell_type::EMPTY);
        assert!(drain_events(&mut rx).iter().all(|(event, _)| event != "@T"));
        crate::net::presentation::deliver_world_effects_for_test(
            &test.state,
            test.state.drain_command_broadcasts(),
        );
        let events = drain_events(&mut rx);
        assert!(
            events.iter().any(|(event, _)| event == "HB"),
            "programmatic autodig must notify the owning client through HB"
        );
    }

    #[tokio::test]
    async fn moving_onto_own_gate_sends_gu_close_like_reference() {
        let mut test = make_test_state("own_gate_sends_gu").await;
        test.player.clan_id = Some(7);

        let extra = crate::db::BuildingExtra {
            charge: 0,
            max_charge: 0,
            cost: 0,
            hp: 0,
            max_hp: 0,
            money_inside: 0,
            crystals_inside: [0; 6],
            items_inside: std::collections::HashMap::new(),
            craft_recipe_id: None,
            craft_num: 0,
            craft_end_ts: 0,
            craft_ready: false,
            clanzone: 0,
        };
        let spec = crate::game::BuildingInsertSpec {
            type_code: " ",
            pack_type: PackType::Gate,
            x: 11,
            y: 10,
            owner_id: PlayerId(test.player.id),
            clan_id: 7,
            extra: &extra,
        };
        test.state.insert_building_runtime(&spec).await.unwrap();

        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        handle_move(
            &test.state,
            &tx,
            PlayerId(test.player.id),
            0,
            11,
            10,
            3,
            false,
        );

        let events = drain_events(&mut rx);
        assert!(
            events
                .iter()
                .any(|(event, payload)| event == "Gu" && payload == b"_"),
            "own Gate entry must close GUI with Gu like C# SendWindow(null), events: {events:?}"
        );
        assert!(
            events.iter().all(|(event, _)| event != "GU"),
            "Gate.GUIWin returns null; it must not open a GU window, events: {events:?}"
        );
    }
}
