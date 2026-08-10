#[allow(clippy::module_inception)]
#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::test_support::drain_events;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[test]
    fn up_page_json_has_reference_titles() {
        let slots = SkillSlots {
            skills: HashMap::new(),
            total_slots: 20,
        };

        let no_selection: serde_json::Value =
            serde_json::from_str(&build_up_page_json(&slots, 20, -1, false)).unwrap();
        let selected: serde_json::Value =
            serde_json::from_str(&build_up_page_json(&slots, 20, 0, false)).unwrap();

        assert_eq!(no_selection["title"], "xxx");
        assert_eq!(selected["title"], "penis");
    }

    #[test]
    fn anti_gun_is_marked_upgrade_ready_when_exp_needed_is_zero() {
        let slots = SkillSlots {
            skills: HashMap::from([(
                0,
                SkillEntry {
                    code: SkillType::AntiGun.code().to_string(),
                    level: 1,
                    exp: 0.0,
                },
            )]),
            total_slots: 20,
        };

        let json: serde_json::Value =
            serde_json::from_str(&build_up_page_json(&slots, 20, 0, false)).unwrap();
        assert_eq!(json["k"], "u:1:0:1#");
    }

    #[test]
    fn empty_slot_install_list_matches_reference_requirements() {
        let slots = SkillSlots {
            skills: HashMap::from([(
                0,
                SkillEntry {
                    code: SkillType::Digging.code().to_string(),
                    level: 1,
                    exp: 0.0,
                },
            )]),
            total_slots: 20,
        };

        let json: serde_json::Value =
            serde_json::from_str(&build_up_page_json(&slots, 20, 4, false)).unwrap();
        let install_list = json["i"].as_str().unwrap();
        let codes: Vec<&str> = install_list.split(':').collect();
        let build_road_unmet = format!("_{}", SkillType::BuildRoad.code());

        assert!(codes.contains(&build_road_unmet.as_str()));
        assert!(codes.contains(&SkillType::BuildGreen.code()));
        assert!(codes.contains(&SkillType::BuildYellow.code()));
        assert!(codes.contains(&SkillType::BuildRed.code()));
        assert!(codes.contains(&SkillType::AntiGun.code()));
        assert!(!codes.contains(&SkillType::Digging.code()));
    }

    #[test]
    fn up_page_json_marks_owner_admin() {
        let slots = SkillSlots {
            skills: HashMap::new(),
            total_slots: 20,
        };

        let owner: serde_json::Value =
            serde_json::from_str(&build_up_page_json(&slots, 20, -1, true)).unwrap();
        let non_owner: serde_json::Value =
            serde_json::from_str(&build_up_page_json(&slots, 20, -1, false)).unwrap();

        assert_eq!(owner["admin"], true);
        assert!(non_owner.get("admin").is_none());
    }

    #[test]
    fn up_admin_page_matches_reference_content() {
        let view = PackView {
            id: 1,
            pack_type: crate::game::PackType::Up,
            x: 10,
            y: 10,
            owner_id: PlayerId(1),
            clan_id: 0,
            charge: 0,
            max_charge: 0,
            hp: 123,
            max_hp: 1000,
        };

        let json = build_up_admin_page_json(&view);
        assert_eq!(json["title"], "UP");
        assert_eq!(
            json["richList"],
            serde_json::json!([
                "hp 123/1000",
                "text",
                "",
                "",
                "",
                "динаху",
                "text",
                "",
                "",
                ""
            ])
        );
    }

    #[test]
    fn generic_skill_description_includes_cost_field() {
        let entry = SkillEntry {
            code: SkillType::Induction.code().to_string(),
            level: 2,
            exp: 0.0,
        };

        let description = build_skill_description(SkillType::Induction, &entry);
        assert!(description.contains("cost:1"));
    }

    #[tokio::test]
    async fn buyslot_keeps_creds_and_does_not_send_money_packet() {
        let test = make_up_test_state("buyslot").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let view = PackView {
            id: 1,
            pack_type: crate::game::PackType::Up,
            x: 10,
            y: 10,
            owner_id: test.player.id.into(),
            clan_id: 0,
            charge: 0,
            max_charge: 0,
            hp: 1000,
            max_hp: 1000,
        };
        open_up_gui(&test.state, &tx, test.player.id.into(), &view);
        drain_events(&mut rx);

        assert!(handle_up_button(
            &test.state,
            &tx,
            test.player.id.into(),
            "buyslot"
        ));

        let events = drain_events(&mut rx);
        assert!(events.iter().all(|(event, _)| event != "P$"));
        assert!(events.iter().any(|(event, payload)| {
            event == "GU" && std::str::from_utf8(payload).is_ok_and(|s| s.starts_with("up:"))
        }));

        let (creds, total_slots, dirty) =
            player_creds_slots_and_dirty(&test.state, test.player.id.into());
        assert_eq!(creds, 1001);
        assert_eq!(total_slots, 21);
        assert!(dirty);
    }

    #[tokio::test]
    async fn buyslot_gui_command_returns_typed_player_save() {
        let test = make_up_test_state("typed_buyslot").await;
        let session_id = crate::game::SessionId::new(78);
        let (tx, mut rx) = test.connect_with_outbox(session_id.get());
        drain_events(&mut rx);
        open_test_up_gui(&test.state, &tx, test.player.id.into());
        drain_events(&mut rx);

        let effects = crate::game::logic::commands::apply_player_command(
            &test.state,
            test.player.id.into(),
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::parse("buyslot".to_owned()),
            },
        );

        assert!(rx.try_recv().is_err());
        assert!(matches!(
            effects.saves.as_slice(),
            [crate::game::SaveCommand::Player { .. }]
        ));
        assert!(matches!(
            effects.events.as_slice(),
            [crate::game::GameEvent::SessionBatch { session_id: event_session, packets, .. }]
                if *event_session == session_id
                    && packets.iter().any(|packet| {
                        openmines_protocol::Packet::try_decode(
                            &mut bytes::BytesMut::from(packet.as_slice()),
                        )
                        .is_ok_and(|decoded| decoded.is_some_and(|packet| packet.event_name == *b"GU"))
                    })
        ));
        assert_eq!(player_slot_count(&test.state, test.player.id.into()), 21);
    }

    #[tokio::test]
    async fn skill_select_gui_command_returns_typed_session_effect() {
        let test = make_up_test_state("typed_skill_select").await;
        let session_id = crate::game::SessionId::new(83);
        let (tx, mut rx) = test.connect_with_outbox(session_id.get());
        drain_events(&mut rx);
        open_test_up_gui(&test.state, &tx, test.player.id.into());
        drain_events(&mut rx);

        let effects = crate::game::logic::commands::apply_player_command(
            &test.state,
            test.player.id.into(),
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::parse("skill:3".to_owned()),
            },
        );

        assert!(rx.try_recv().is_err());
        assert!(effects.saves.is_empty());
        assert!(matches!(
            effects.events.as_slice(),
            [crate::game::GameEvent::SessionBatch { session_id: event_session, packets, .. }]
                if *event_session == session_id
                    && packets.iter().any(|packet| {
                        openmines_protocol::Packet::try_decode(
                            &mut bytes::BytesMut::from(packet.as_slice()),
                        )
                        .is_ok_and(|decoded| decoded.is_some_and(|packet| packet.event_name == *b"GU"))
                    })
        ));
        let selected = test
            .state
            .query_player_opt(test.player.id.into(), |ecs, entity| {
                ecs.get::<PlayerUI>(entity)?.current_window.clone()
            });
        assert!(selected.is_some_and(|window| window.split(':').nth(3) == Some("3")));
    }

    #[tokio::test]
    async fn skill_upgrade_sends_health_packet_for_non_health_skill() {
        let mut test = make_up_test_state("upgrade_health_packet").await;
        test.player.money = 1_000;
        test.player.skills.skills.get_mut(&1).unwrap().exp = 1.0;

        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);
        let expected_health = player_health_payload(&test.state, test.player.id.into());

        let view = PackView {
            id: 1,
            pack_type: crate::game::PackType::Up,
            x: 10,
            y: 10,
            owner_id: test.player.id.into(),
            clan_id: 0,
            charge: 0,
            max_charge: 0,
            hp: 1000,
            max_hp: 1000,
        };
        open_up_gui(&test.state, &tx, test.player.id.into(), &view);
        assert!(handle_up_button(
            &test.state,
            &tx,
            test.player.id.into(),
            "skill:1"
        ));
        drain_events(&mut rx);

        assert!(handle_up_button(
            &test.state,
            &tx,
            test.player.id.into(),
            "upgrade"
        ));

        let events = drain_events(&mut rx);
        let event_names = events
            .iter()
            .map(|(event, _)| event.as_str())
            .collect::<Vec<_>>();

        assert!(
            event_names
                .windows(3)
                .any(|window| window == ["@S", "LV", "@L"])
        );
        assert!(
            events
                .iter()
                .any(|(event, payload)| event == "@L" && payload == expected_health.as_bytes())
        );
    }

    #[tokio::test]
    async fn skill_delete_sends_level_without_skills_packet() {
        let test = make_up_test_state("delete_no_skills_packet").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        open_test_up_gui(&test.state, &tx, test.player.id.into());
        assert!(handle_up_button(
            &test.state,
            &tx,
            test.player.id.into(),
            "skill:1"
        ));
        drain_events(&mut rx);

        assert!(handle_up_button(
            &test.state,
            &tx,
            test.player.id.into(),
            "delete:1"
        ));

        let events = drain_events(&mut rx);
        assert!(events.iter().any(|(event, _)| event == "LV"));
        assert!(events.iter().all(|(event, _)| event != "@S"));
    }

    #[tokio::test]
    async fn skill_install_sends_level_without_skills_packet() {
        let test = make_up_test_state("install_no_skills_packet").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        open_test_up_gui(&test.state, &tx, test.player.id.into());
        assert!(handle_up_button(
            &test.state,
            &tx,
            test.player.id.into(),
            "skill:4"
        ));
        drain_events(&mut rx);

        assert!(handle_up_button(
            &test.state,
            &tx,
            test.player.id.into(),
            "install:p#4"
        ));

        let events = drain_events(&mut rx);
        assert!(events.iter().any(|(event, _)| event == "LV"));
        assert!(events.iter().all(|(event, _)| event != "@S"));
    }

    #[tokio::test]
    async fn up_button_missing_ui_is_explicit_error_not_unhandled_button() {
        let test = make_up_test_state("up_button_missing_ui").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity).remove::<PlayerUI>();
        }

        assert!(handle_up_button(&test.state, &tx, pid, "buyslot"));

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние апгрейда недоступно."));
    }

    #[tokio::test]
    async fn buyslot_missing_skills_is_explicit_error_not_not_enough_slots_noop() {
        let test = make_up_test_state("buyslot_missing_skills").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        open_test_up_gui(&test.state, &tx, pid);
        drain_events(&mut rx);
        {
            let entity = test.state.get_player_entity(pid).unwrap();
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity).remove::<PlayerSkillsComp>();
        }

        assert!(handle_up_button(&test.state, &tx, pid, "buyslot"));

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние апгрейда недоступно."));
    }

    #[tokio::test]
    async fn skill_upgrade_missing_flags_is_explicit_error_before_money_or_skill_mutation() {
        let mut test = make_up_test_state("upgrade_missing_flags").await;
        test.player.money = 1_000;
        test.player.skills.skills.get_mut(&1).unwrap().exp = 1.0;

        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        open_test_up_gui(&test.state, &tx, pid);
        assert!(handle_up_button(&test.state, &tx, pid, "skill:1"));
        drain_events(&mut rx);
        remove_player_flags(&test.state, pid);

        assert!(handle_up_button(&test.state, &tx, pid, "upgrade"));

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert!(
            std::str::from_utf8(&events[0].1)
                .unwrap()
                .contains("Состояние апгрейда недоступно.")
        );
        assert_eq!(player_money(&test.state, pid), 1_000);
        assert_eq!(skill_level_exp(&test.state, pid, 1), (1, 1.0));
    }

    #[tokio::test]
    async fn skill_upgrade_at_i32_max_is_explicit_error_without_partial_mutation() {
        let test = make_up_test_state("upgrade_max_level").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        open_test_up_gui(&test.state, &tx, pid);
        assert!(handle_up_button(&test.state, &tx, pid, "skill:1"));
        drain_events(&mut rx);

        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test
                .state
                .ecs_write_profiled("up_building.max_level_preflight");
            ecs.get_mut::<PlayerStats>(entity).unwrap().money = i64::MAX;
            {
                let mut skills = ecs.get_mut::<PlayerSkillsComp>(entity).unwrap();
                let entry = skills.states.skills.get_mut(&1).unwrap();
                entry.level = i32::MAX;
                entry.exp = 1.0;
            }
            ecs.get_mut::<crate::game::PlayerFlags>(entity)
                .unwrap()
                .dirty = false;
        }

        assert!(handle_up_button(&test.state, &tx, pid, "upgrade"));

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert_eq!(
            events[0].1,
            "Апгрейд#Навык уже достиг максимального уровня.".as_bytes()
        );
        assert_eq!(player_money(&test.state, pid), i64::MAX);
        let (level, exp) = skill_level_exp(&test.state, pid, 1);
        assert_eq!(level, i32::MAX);
        assert_eq!(exp.to_bits(), 1.0_f32.to_bits());
        assert!(!player_creds_slots_and_dirty(&test.state, pid).2);
    }

    #[tokio::test]
    async fn skill_delete_missing_flags_is_explicit_error_without_skill_mutation_or_rerender() {
        let test = make_up_test_state("delete_missing_flags").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        open_test_up_gui(&test.state, &tx, pid);
        assert!(handle_up_button(&test.state, &tx, pid, "skill:1"));
        drain_events(&mut rx);
        remove_player_flags(&test.state, pid);

        assert!(handle_up_button(&test.state, &tx, pid, "delete:1"));

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert!(skill_exists(&test.state, pid, 1));
    }

    #[tokio::test]
    async fn skill_install_missing_flags_is_explicit_error_without_skill_mutation() {
        let test = make_up_test_state("install_missing_flags").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        open_test_up_gui(&test.state, &tx, pid);
        assert!(handle_up_button(&test.state, &tx, pid, "skill:4"));
        drain_events(&mut rx);
        remove_player_flags(&test.state, pid);

        assert!(handle_up_button(&test.state, &tx, pid, "install:p#4"));

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert!(!skill_exists(&test.state, pid, 4));
    }

    #[tokio::test]
    async fn buyslot_missing_flags_is_explicit_error_without_slot_mutation() {
        let test = make_up_test_state("buyslot_missing_flags").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        open_test_up_gui(&test.state, &tx, pid);
        drain_events(&mut rx);
        remove_player_flags(&test.state, pid);

        assert!(handle_up_button(&test.state, &tx, pid, "buyslot"));

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert_eq!(player_slot_count(&test.state, pid), 20);
    }

    fn open_test_up_gui(
        state: &Arc<GameState>,
        tx: &dyn crate::net::session::wire::PacketSink,
        pid: PlayerId,
    ) {
        let view = PackView {
            id: 1,
            pack_type: crate::game::PackType::Up,
            x: 10,
            y: 10,
            owner_id: pid,
            clan_id: 0,
            charge: 0,
            max_charge: 0,
            hp: 1000,
            max_hp: 1000,
        };
        open_up_gui(state, tx, pid, &view);
    }

    async fn make_up_test_state(label: &str) -> crate::test_support::ServerTestHarness {
        let mut builder = crate::test_support::ServerTestHarnessBuilder::new(
            &format!("up_building_{label}"),
            "up-user",
        )
        .await;
        builder.player.creds = 1001;
        builder.build().await
    }

    fn player_creds_slots_and_dirty(state: &Arc<GameState>, pid: PlayerId) -> (i64, i32, bool) {
        state
            .query_player_opt(pid, |ecs, entity| {
                let player_stats = ecs.get::<PlayerStats>(entity)?;
                let skills = ecs.get::<PlayerSkillsComp>(entity)?;
                let flags = ecs.get::<crate::game::PlayerFlags>(entity)?;
                Some((player_stats.creds, skills.states.total_slots, flags.dirty))
            })
            .unwrap()
    }

    fn player_health_payload(state: &Arc<GameState>, pid: PlayerId) -> String {
        state
            .query_player_opt(pid, |ecs, entity| {
                let player_stats = ecs.get::<PlayerStats>(entity)?;
                Some(format!(
                    "{}:{}",
                    player_stats.health, player_stats.max_health
                ))
            })
            .unwrap()
    }

    fn remove_player_flags(state: &Arc<GameState>, pid: PlayerId) {
        let entity = state.get_player_entity(pid).unwrap();
        let mut ecs = state.ecs_write_profiled("up_building.upgrade_apply");
        ecs.entity_mut(entity).remove::<crate::game::PlayerFlags>();
    }

    fn player_money(state: &Arc<GameState>, pid: PlayerId) -> i64 {
        state
            .query_player_opt(pid, |ecs, entity| {
                let player_stats = ecs.get::<PlayerStats>(entity)?;
                Some(player_stats.money)
            })
            .unwrap()
    }

    fn player_slot_count(state: &Arc<GameState>, pid: PlayerId) -> i32 {
        state
            .query_player_opt(pid, |ecs, entity| {
                let skills = ecs.get::<PlayerSkillsComp>(entity)?;
                Some(skills.states.total_slots)
            })
            .unwrap()
    }

    fn skill_exists(state: &Arc<GameState>, pid: PlayerId, slot: i32) -> bool {
        state
            .query_player_opt(pid, |ecs, entity| {
                let skills = ecs.get::<PlayerSkillsComp>(entity)?;
                Some(skills.states.skills.contains_key(&slot))
            })
            .unwrap()
    }

    fn skill_level_exp(state: &Arc<GameState>, pid: PlayerId, slot: i32) -> (i32, f32) {
        state
            .query_player_opt(pid, |ecs, entity| {
                let skills = ecs.get::<PlayerSkillsComp>(entity)?;
                let skill = skills.states.skills.get(&slot)?;
                Some((skill.level, skill.exp))
            })
            .unwrap()
    }
}
