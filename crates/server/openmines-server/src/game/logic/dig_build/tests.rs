#[allow(clippy::module_inception)]
#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::test_support::{ServerTestHarness, drain_events};
    use std::sync::Arc;
    use std::time::Duration;

    async fn make_test_state(label: &str) -> ServerTestHarness {
        ServerTestHarness::new(label, "build-user").await
    }

    fn basket_green(payload: &[u8]) -> i64 {
        std::str::from_utf8(payload)
            .unwrap()
            .split(':')
            .next()
            .unwrap()
            .parse()
            .unwrap()
    }

    fn directed_mine_fx_amount(payload: &[u8]) -> Option<i64> {
        let mut i = 0;
        while i + 10 <= payload.len() {
            if payload[i] != b'D' {
                return None;
            }
            let fx = payload[i + 1];
            let amount = u16::from_le_bytes([payload[i + 2], payload[i + 3]]);
            if fx == 2 {
                return Some(i64::from(amount));
            }
            i += 10;
        }
        None
    }

    fn queued_hb_payloads(state: &GameState) -> Vec<Vec<u8>> {
        state
            .drain_command_broadcasts()
            .into_iter()
            .filter_map(|effect| match effect {
                crate::game::BroadcastEffect::Nearby { data, .. } => {
                    let mut data = bytes::BytesMut::from(data.as_slice());
                    let packet = crate::protocol::Packet::try_decode(&mut data)
                        .expect("queued HB must decode")
                        .expect("queued HB must be complete");
                    assert_eq!(packet.event_name, *b"HB");
                    Some(packet.payload.to_vec())
                }
                crate::game::BroadcastEffect::Direct { .. }
                | crate::game::BroadcastEffect::CellUpdate(_)
                | crate::game::BroadcastEffect::BlockUpdate(_) => None,
            })
            .collect()
    }

    #[test]
    fn crystal_add_matches_reference_overflow_guard() {
        assert_eq!(add_crystals_like_reference(10, 5), 15);
        assert_eq!(add_crystals_like_reference(10, -5), 5);
        assert_eq!(add_crystals_like_reference(3, -5), i64::MAX);
        assert_eq!(add_crystals_like_reference(i64::MAX - 1, 10), i64::MAX);
    }

    #[tokio::test]
    async fn crystal_spend_missing_player_stats_is_explicit_error_not_insufficient_resources() {
        let test = make_test_state("crystal_spend_missing_stats").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerStats>();
        }

        assert!(!try_spend_crystal(&test.state, &tx, pid, 0, 1));

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));
    }

    #[tokio::test]
    async fn crystal_spend_missing_player_flags_is_explicit_error_without_crystal_mutation() {
        let test = make_test_state("crystal_spend_missing_flags").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.get_mut::<crate::game::player::PlayerStats>(entity)
                .unwrap()
                .crystals[0] = 10;
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerFlags>();
        }

        assert!(!try_spend_crystal(&test.state, &tx, pid, 0, 1));

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));
        assert_eq!(player_crystal(&test.state, pid, 0), 10);
    }

    #[tokio::test]
    async fn crystal_spend_success_marks_player_dirty() {
        let test = make_test_state("crystal_spend_dirty").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.get_mut::<crate::game::player::PlayerStats>(entity)
                .unwrap()
                .crystals[0] = 10;
            ecs.get_mut::<crate::game::player::PlayerFlags>(entity)
                .unwrap()
                .dirty = false;
        }

        assert!(try_spend_crystal(&test.state, &tx, pid, 0, 1));

        assert_eq!(player_crystal(&test.state, pid, 0), 9);
        assert!(player_dirty(&test.state, pid));
        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "@B");
    }

    #[tokio::test]
    async fn dig_missing_player_skills_is_explicit_error_not_silent_noop() {
        let test = make_test_state("dig_missing_skills").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut cd = ecs
                .get_mut::<crate::game::player::PlayerCooldowns>(entity)
                .unwrap();
            cd.last_dig -= Duration::from_millis(500);
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerSkillsComp>();
        }

        handle_dig(&test.state, &tx, pid, 0, false);

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));
    }

    #[tokio::test]
    async fn programmatic_dig_ignores_open_gui_but_manual_dig_does_not() {
        let test = make_test_state("programmatic_dig_window").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut pos = ecs
                .get_mut::<crate::game::player::PlayerPosition>(entity)
                .unwrap();
            pos.x = 10;
            pos.y = 10;
            pos.dir = 0;
            ecs.get_mut::<crate::game::player::PlayerUI>(entity)
                .unwrap()
                .current_window = Some("prog".to_string());
        }
        test.state.world.set_cell(10, 11, cell_type::GREEN);
        test.state.world.set_durability(10, 11, 100.0);

        handle_dig(&test.state, &tx, pid, 0, false);
        assert!(!drain_events(&mut rx).iter().any(|(event, _)| event == "@B"));

        {
            let mut ecs = test.state.ecs.write();
            ecs.get_mut::<crate::game::player::PlayerCooldowns>(entity)
                .unwrap()
                .last_dig -= Duration::from_millis(500);
        }
        handle_dig(&test.state, &tx, pid, 0, true);

        let events = drain_events(&mut rx);
        assert!(
            events.iter().any(|(event, _)| event == "@B"),
            "programmatic Dig must call Bz even while programmator GUI state is open"
        );
        assert!(
            queued_hb_payloads(&test.state)
                .iter()
                .any(|payload| !payload.is_empty()),
            "programmatic Dig must queue self HB for programmator visual sync"
        );
    }

    #[tokio::test]
    async fn crystal_mine_fx_caps_visual_amount_without_capping_inventory() {
        let test = make_test_state("crystal_mine_fx_final_amount").await;
        {
            let now = crate::time::now_unix();
            test.state
                .active_events
                .write()
                .list
                .push(crate::game::ActiveEvent {
                    id: "drop_x100000".to_string(),
                    title: "drop_x100000".to_string(),
                    starts_at: now - 1,
                    ends_at: now + 60,
                    xp_mult: 1.0,
                    drop_mult: 100_000.0,
                });
        }
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut pos = ecs
                .get_mut::<crate::game::player::PlayerPosition>(entity)
                .unwrap();
            pos.x = 10;
            pos.y = 10;
            pos.dir = 0;
            ecs.get_mut::<crate::game::player::PlayerCooldowns>(entity)
                .unwrap()
                .last_dig -= Duration::from_millis(500);
        }
        test.state.world.set_cell(10, 11, cell_type::GREEN);
        test.state.world.set_durability(10, 11, 100.0);

        handle_dig(&test.state, &tx, pid, 0, false);

        let events = drain_events(&mut rx);
        let basket_amount = events
            .iter()
            .find(|(event, _)| event == "@B")
            .map(|(_, payload)| basket_green(payload))
            .expect("@B after crystal mine");
        let fx_amount = queued_hb_payloads(&test.state)
            .iter()
            .find_map(|payload| directed_mine_fx_amount(payload))
            .expect("mine D FX after crystal mine");

        assert!(
            basket_amount > i64::from(u16::MAX),
            "test must exercise event/drop multiplier, got {basket_amount}"
        );
        assert_eq!(fx_amount, 255);
    }

    #[tokio::test]
    async fn crystal_mine_sends_skill_progress_before_basket_like_reference() {
        let test = make_test_state("crystal_mine_packet_order").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut pos = ecs
                .get_mut::<crate::game::player::PlayerPosition>(entity)
                .unwrap();
            pos.x = 10;
            pos.y = 10;
            pos.dir = 0;
            ecs.get_mut::<crate::game::player::PlayerCooldowns>(entity)
                .unwrap()
                .last_dig -= Duration::from_millis(500);
        }
        test.state.world.set_cell(10, 11, cell_type::GREEN);
        test.state.world.set_durability(10, 11, 100.0);

        handle_dig(&test.state, &tx, pid, 0, false);

        let events = drain_events(&mut rx);
        let skill_idx = events
            .iter()
            .position(|(event, _)| event == "@S")
            .expect("@S after MineGeneral exp");
        let basket_idx = events
            .iter()
            .position(|(event, _)| event == "@B")
            .expect("@B after crystal mine");

        assert!(
            skill_idx < basket_idx,
            "C# Mine() sends @S before @B, got {events:?}"
        );
    }

    #[tokio::test]
    async fn build_turn_marks_player_dirty_even_when_build_does_not_happen() {
        let test = make_test_state("build_turn_dirty").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut pos = ecs
                .get_mut::<crate::game::player::PlayerPosition>(entity)
                .unwrap();
            pos.x = 10;
            pos.y = 10;
            pos.dir = 0;
            ecs.get_mut::<crate::game::player::PlayerFlags>(entity)
                .unwrap()
                .dirty = false;
        }
        test.state.world.set_cell(9, 10, cell_type::EMPTY);

        let bld = XbldClient {
            direction: 1,
            block_type: "G",
        };
        handle_build(&test.state, &tx, pid, &bld, false);

        let (dir, dirty) = test
            .state
            .query_player_opt(pid, |ecs, entity| {
                let pos = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
                let flags = ecs.get::<crate::game::player::PlayerFlags>(entity)?;
                Some((pos.dir, flags.dirty))
            })
            .unwrap();
        assert_eq!(dir, 1);
        assert!(dirty);
    }

    #[tokio::test]
    async fn dig_turn_marks_player_dirty_even_without_crystal_gain() {
        let test = make_test_state("dig_turn_dirty").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut pos = ecs
                .get_mut::<crate::game::player::PlayerPosition>(entity)
                .unwrap();
            pos.x = 10;
            pos.y = 10;
            pos.dir = 0;
            ecs.get_mut::<crate::game::player::PlayerCooldowns>(entity)
                .unwrap()
                .last_dig -= Duration::from_millis(500);
            ecs.get_mut::<crate::game::player::PlayerFlags>(entity)
                .unwrap()
                .dirty = false;
        }
        test.state.world.set_cell(9, 10, cell_type::EMPTY);

        handle_dig(&test.state, &tx, pid, 1, false);

        let (dir, dirty) = test
            .state
            .query_player_opt(pid, |ecs, entity| {
                let pos = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
                let flags = ecs.get::<crate::game::player::PlayerFlags>(entity)?;
                Some((pos.dir, flags.dirty))
            })
            .unwrap();
        assert_eq!(dir, 1);
        assert!(dirty);
    }

    #[tokio::test]
    async fn dig_missing_player_flags_is_explicit_error_before_world_damage() {
        let test = make_test_state("dig_missing_flags_no_damage").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut pos = ecs
                .get_mut::<crate::game::player::PlayerPosition>(entity)
                .unwrap();
            pos.x = 10;
            pos.y = 10;
            pos.dir = 0;
            let mut cd = ecs
                .get_mut::<crate::game::player::PlayerCooldowns>(entity)
                .unwrap();
            cd.last_dig -= Duration::from_millis(500);
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerFlags>();
        }
        test.state.world.set_cell(10, 11, cell_type::ROCK);
        test.state.world.set_durability(10, 11, 0.0);

        handle_dig(&test.state, &tx, pid, 0, false);

        assert_eq!(test.state.world.get_cell(10, 11), cell_type::ROCK);
        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));
    }

    #[tokio::test]
    async fn dig_crystal_missing_player_flags_is_explicit_error_without_crystal_gain() {
        let test = make_test_state("dig_crystal_missing_flags").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut pos = ecs
                .get_mut::<crate::game::player::PlayerPosition>(entity)
                .unwrap();
            pos.x = 10;
            pos.y = 10;
            pos.dir = 0;
            let mut cd = ecs
                .get_mut::<crate::game::player::PlayerCooldowns>(entity)
                .unwrap();
            cd.last_dig -= Duration::from_millis(500);
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerFlags>();
        }
        test.state.world.set_cell(10, 11, cell_type::GREEN);
        test.state.world.set_durability(10, 11, 100.0);

        handle_dig(&test.state, &tx, pid, 0, false);

        assert_eq!(player_crystal(&test.state, pid, 0), 0);
        assert_eq!(test.state.world.get_cell(10, 11), cell_type::GREEN);
        let events = drain_events(&mut rx);
        assert!(events.iter().any(|(event, payload)| {
            event == "OK"
                && std::str::from_utf8(payload)
                    .is_ok_and(|message| message.contains("Состояние игрока недоступно."))
        }));
        assert!(!events.iter().any(|(event, _)| event == "@B"));
    }

    #[tokio::test]
    async fn dig_box_missing_player_flags_keeps_box_and_sends_explicit_error() {
        let test = make_test_state("dig_box_missing_flags").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut pos = ecs
                .get_mut::<crate::game::player::PlayerPosition>(entity)
                .unwrap();
            pos.x = 10;
            pos.y = 10;
            pos.dir = 0;
            let mut cd = ecs
                .get_mut::<crate::game::player::PlayerCooldowns>(entity)
                .unwrap();
            cd.last_dig -= Duration::from_millis(500);
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerFlags>();
        }
        test.state
            .put_box_cell_authoritative(10, 11, [3, 2, 1, 0, 0, 0]);

        handle_dig(&test.state, &tx, pid, 0, false);

        assert_eq!(player_crystal(&test.state, pid, 0), 0);
        assert_eq!(test.state.world.get_cell(10, 11), cell_type::BOX);
        let events = drain_events(&mut rx);
        assert!(events.iter().any(|(event, payload)| {
            event == "OK"
                && std::str::from_utf8(payload)
                    .is_ok_and(|message| message.contains("Состояние игрока недоступно."))
        }));
        assert!(!events.iter().any(|(event, _)| event == "@B"));
    }

    #[tokio::test]
    async fn dig_box_publishes_intent_without_mutating_box_or_crystals() {
        let test = make_test_state("dig_box_intent").await;
        let (tx, mut rx) = test.connect_with_outbox(2);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut pos = ecs
                .get_mut::<crate::game::player::PlayerPosition>(entity)
                .unwrap();
            pos.x = 10;
            pos.y = 10;
            pos.dir = 0;
            ecs.get_mut::<crate::game::player::PlayerCooldowns>(entity)
                .unwrap()
                .last_dig -= Duration::from_millis(500);
        }
        test.state
            .put_box_cell_authoritative(10, 11, [3, 2, 1, 0, 0, 0]);

        handle_dig(&test.state, &tx, pid, 0, false);

        let intents = test.state.drain_box_pickups();
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].player_pos, (10, 10).into());
        assert_eq!(intents[0].box_pos, (10, 11).into());
        assert!(matches!(
            intents[0].source,
            crate::game::BoxPickupSource::Dig {
                session_id: Some(session_id),
                exclude_self: true,
                ..
            } if session_id == crate::game::SessionId::new(2)
        ));
        assert_eq!(player_crystal(&test.state, pid, 0), 0);
        assert_eq!(test.state.world.get_cell(10, 11), cell_type::BOX);
        assert!(!drain_events(&mut rx).iter().any(|(event, _)| event == "@B"));
    }

    #[tokio::test]
    async fn crystal_spend_insufficient_resources_stays_quiet_false() {
        let test = make_test_state("crystal_spend_insufficient").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        assert!(!try_spend_crystal(&test.state, &tx, pid, 0, 1));

        let events = drain_events(&mut rx);
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn build_missing_player_skills_is_explicit_error_not_blocked_fallback() {
        let test = make_test_state("build_missing_skills").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut cd = ecs
                .get_mut::<crate::game::player::PlayerCooldowns>(entity)
                .unwrap();
            cd.last_build -= Duration::from_millis(500);
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerSkillsComp>();
        }

        let bld = XbldClient {
            direction: 0,
            block_type: "G",
        };
        handle_build(&test.state, &tx, pid, &bld, false);

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));
    }

    #[tokio::test]
    async fn build_yellow_upgrade_missing_skills_does_not_use_default_cost_or_mutate_world() {
        let test = make_test_state("build_yellow_upgrade_missing_skills").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut pos = ecs
                .get_mut::<crate::game::player::PlayerPosition>(entity)
                .unwrap();
            pos.x = 10;
            pos.y = 10;
            pos.dir = 0;
            let mut cd = ecs
                .get_mut::<crate::game::player::PlayerCooldowns>(entity)
                .unwrap();
            cd.last_build -= Duration::from_millis(500);
            let mut stats = ecs
                .get_mut::<crate::game::player::PlayerStats>(entity)
                .unwrap();
            stats.crystals[4] = 10;
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerSkillsComp>();
        }
        test.state.world.set_cell(10, 11, cell_type::GREEN_BLOCK);
        test.state.world.set_durability(10, 11, 5.0);

        let bld = XbldClient {
            direction: 0,
            block_type: "G",
        };
        handle_build(&test.state, &tx, pid, &bld, false);

        assert_eq!(player_crystal(&test.state, pid, 4), 10);
        assert_eq!(test.state.world.get_cell(10, 11), cell_type::GREEN_BLOCK);
        assert!((test.state.world.get_durability(10, 11) - 5.0).abs() < f32::EPSILON);
        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));
    }

    #[tokio::test]
    async fn build_red_upgrade_missing_skills_does_not_use_default_cost_or_mutate_world() {
        let test = make_test_state("build_red_upgrade_missing_skills").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut pos = ecs
                .get_mut::<crate::game::player::PlayerPosition>(entity)
                .unwrap();
            pos.x = 10;
            pos.y = 10;
            pos.dir = 0;
            let mut cd = ecs
                .get_mut::<crate::game::player::PlayerCooldowns>(entity)
                .unwrap();
            cd.last_build -= Duration::from_millis(500);
            let mut stats = ecs
                .get_mut::<crate::game::player::PlayerStats>(entity)
                .unwrap();
            stats.crystals[2] = 10;
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerSkillsComp>();
        }
        test.state.world.set_cell(10, 11, cell_type::YELLOW_BLOCK);
        test.state.world.set_durability(10, 11, 7.0);

        let bld = XbldClient {
            direction: 0,
            block_type: "G",
        };
        handle_build(&test.state, &tx, pid, &bld, false);

        assert_eq!(player_crystal(&test.state, pid, 2), 10);
        assert_eq!(test.state.world.get_cell(10, 11), cell_type::YELLOW_BLOCK);
        assert!((test.state.world.get_durability(10, 11) - 7.0).abs() < f32::EPSILON);
        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));
    }

    #[tokio::test]
    async fn build_cooldown_block_stays_quiet_noop() {
        let test = make_test_state("build_cooldown_quiet").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let bld = XbldClient {
            direction: 0,
            block_type: "G",
        };
        handle_build(&test.state, &tx, PlayerId(test.player.id), &bld, false);

        let events = drain_events(&mut rx);
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn baseline_build_cycle_can_destroy_and_replace_a_green_block() {
        let test = make_test_state("build_cycle").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);
        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut pos = ecs
                .get_mut::<crate::game::player::PlayerPosition>(entity)
                .unwrap();
            pos.x = 10;
            pos.y = 10;
            pos.dir = 0;
            ecs.get_mut::<crate::game::player::PlayerStats>(entity)
                .unwrap()
                .crystals[0] = 10;
            ecs.get_mut::<crate::game::player::PlayerCooldowns>(entity)
                .unwrap()
                .last_build -= Duration::from_millis(500);
        }
        test.state.world.destroy_cell_and_road(10, 11);
        let build = XbldClient {
            direction: 0,
            block_type: "G",
        };

        handle_build(&test.state, &tx, pid, &build, false);
        assert_eq!(test.state.world.get_cell(10, 11), cell_type::GREEN_BLOCK);
        for _ in 0..12 {
            test.state
                .modify_player(pid, |ecs, entity| {
                    ecs.get_mut::<crate::game::player::PlayerCooldowns>(entity)?
                        .last_dig -= Duration::from_millis(500);
                    Some(())
                })
                .unwrap();
            handle_dig(&test.state, &tx, pid, 0, false);
        }
        assert!(is_truly_empty(test.state.world.get_cell_typed(10, 11)));

        test.state
            .modify_player(pid, |ecs, entity| {
                ecs.get_mut::<crate::game::player::PlayerCooldowns>(entity)?
                    .last_build -= Duration::from_millis(500);
                Some(())
            })
            .unwrap();
        handle_build(&test.state, &tx, pid, &build, false);
        assert_eq!(test.state.world.get_cell(10, 11), cell_type::GREEN_BLOCK);
    }

    fn player_crystal(state: &Arc<GameState>, pid: PlayerId, idx: usize) -> i64 {
        state
            .query_player_opt(pid, |ecs, entity| {
                Some(
                    ecs.get::<crate::game::player::PlayerStats>(entity)?
                        .crystals[idx],
                )
            })
            .unwrap()
    }

    fn player_dirty(state: &Arc<GameState>, pid: PlayerId) -> bool {
        state
            .query_player_opt(pid, |ecs, entity| {
                Some(ecs.get::<crate::game::player::PlayerFlags>(entity)?.dirty)
            })
            .unwrap()
    }
}
