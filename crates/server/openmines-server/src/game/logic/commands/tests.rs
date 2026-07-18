use super::{apply_persistence_completion, apply_player_command};
use bytes::Bytes;

#[tokio::test]
async fn local_chat_queues_nearby_hb_instead_of_delivering_during_dispatch() {
    let test = crate::test_support::ServerTestHarness::new("local_chat_side_effect", "local").await;
    let player_id = crate::game::PlayerId(test.player.id);
    let session_id = crate::game::SessionId::new(1);
    let mut receiver = test.connect(session_id.get());
    crate::test_support::ServerTestHarness::drain_events(&mut receiver);

    let effects = apply_player_command(
        &test.state,
        player_id,
        session_id,
        crate::game::PlayerCommand::LocalChat {
            message: "queued bubble".to_owned(),
        },
    );

    assert!(effects.events.is_empty());
    assert!(
        receiver.try_recv().is_err(),
        "dispatch must not write chat HB"
    );
    assert!(matches!(
        test.state.drain_command_broadcasts().as_slice(),
        [crate::game::BroadcastEffect::Nearby { data, .. }]
            if openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(data.as_slice()))
                .expect("queued local chat must decode")
                .is_some_and(|packet| packet.event_name == *b"HB")
    ));
}

#[tokio::test]
async fn chat_color_completion_delivers_only_to_the_current_session() {
    let test = crate::test_support::ServerTestHarness::new(
        "chat_color_completion_session_guard",
        "chat-color-player",
    )
    .await;
    let player_id = crate::game::PlayerId(test.player.id);
    let stale_session = crate::game::SessionId::new(101);
    let mut receiver = test.connect(stale_session.get());
    crate::test_support::ServerTestHarness::drain_events(&mut receiver);

    let effects = apply_player_command(
        &test.state,
        player_id,
        stale_session,
        crate::game::PlayerCommand::ChatSettings {
            payload: Bytes::from_static(b"_"),
        },
    );
    assert!(matches!(
        effects.saves.as_slice(),
        [crate::game::SaveCommand::ChatColorCycle {
            request: crate::game::ChatColorCycleRequest { player_id: id, session_id }
        }] if *id == player_id && *session_id == stale_session
    ));

    apply_persistence_completion(
        &test.state,
        crate::game::PersistenceCompletion::ChatColorCycled {
            request: crate::game::ChatColorCycleRequest {
                player_id,
                session_id: stale_session,
            },
            result: crate::game::ChatColorCycleResult::Cycled { color: 3 },
        },
    );
    assert_eq!(
        crate::test_support::ServerTestHarness::drain_events(&mut receiver),
        vec![("mC".to_owned(), b"3".to_vec())]
    );

    let current_session = crate::game::SessionId::new(102);
    let mut current_receiver = test.connect(current_session.get());
    crate::test_support::ServerTestHarness::drain_events(&mut current_receiver);
    apply_persistence_completion(
        &test.state,
        crate::game::PersistenceCompletion::ChatColorCycled {
            request: crate::game::ChatColorCycleRequest {
                player_id,
                session_id: stale_session,
            },
            result: crate::game::ChatColorCycleResult::Cycled { color: 4 },
        },
    );
    assert!(crate::test_support::ServerTestHarness::drain_events(&mut current_receiver).is_empty());
}

#[tokio::test]
async fn moneyall_is_admitted_and_completion_is_session_guarded() {
    let test = crate::test_support::ServerTestHarness::new(
        "moneyall_completion_session_guard",
        "moneyall-admin",
    )
    .await;
    let player_id = crate::game::PlayerId(test.player.id);
    let session_id = crate::game::SessionId::new(101);
    let mut receiver = test.connect(session_id.get());
    crate::test_support::ServerTestHarness::drain_events(&mut receiver);
    test.state.modify_player(player_id, |ecs, entity| {
        ecs.get_mut::<crate::game::player::PlayerStats>(entity)
            .expect("connected player stats")
            .role = 2;
    });

    let effects = apply_player_command(
        &test.state,
        player_id,
        session_id,
        crate::game::PlayerCommand::Slash {
            command: crate::game::SlashCommand::MoneyAll { amount: 15 },
        },
    );
    assert!(matches!(
        effects.saves.as_slice(),
        [crate::game::SaveCommand::AdminMoneyAll { request }]
            if request.player_id == player_id && request.session_id == session_id && request.amount == 15
    ));

    let effects = apply_persistence_completion(
        &test.state,
        crate::game::PersistenceCompletion::AdminMoneyAllApplied {
            request: crate::game::AdminMoneyAllRequest {
                player_id,
                session_id,
                amount: 15,
            },
            result: crate::game::AdminMoneyAllResult::Applied {
                affected_players: 1,
            },
        },
    );
    assert!(matches!(
        effects.broadcasts.as_slice(),
        [crate::game::BroadcastEffect::Direct { data, .. }]
            if openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(data.as_slice()))
                .expect("money completion packet must decode")
                .is_some_and(|packet| packet.event_name == *b"P$")
    ));
    assert!(matches!(
        effects.events.as_slice(),
        [crate::game::GameEvent::SessionBatch { session_id: current, .. }] if *current == session_id
    ));

    let current_session = crate::game::SessionId::new(102);
    let mut current_receiver = test.connect(current_session.get());
    crate::test_support::ServerTestHarness::drain_events(&mut current_receiver);
    let stale_effects = apply_persistence_completion(
        &test.state,
        crate::game::PersistenceCompletion::AdminMoneyAllApplied {
            request: crate::game::AdminMoneyAllRequest {
                player_id,
                session_id,
                amount: 15,
            },
            result: crate::game::AdminMoneyAllResult::Applied {
                affected_players: 1,
            },
        },
    );
    assert!(stale_effects.events.is_empty());
}

#[tokio::test]
async fn clan_create_is_admitted_without_legacy_outbox_delivery() {
    let test =
        crate::test_support::ServerTestHarness::new("clan_command_is_durable", "clan-founder")
            .await;
    let player_id = crate::game::PlayerId(test.player.id);
    let session_id = crate::game::SessionId::new(201);
    let mut receiver = test.connect(session_id.get());
    crate::test_support::ServerTestHarness::drain_events(&mut receiver);
    test.state.modify_player(player_id, |ecs, entity| {
        ecs.get_mut::<crate::game::player::PlayerStats>(entity)
            .expect("connected player stats")
            .creds = 1_000;
    });

    let effects = apply_player_command(
        &test.state,
        player_id,
        session_id,
        crate::game::PlayerCommand::Slash {
            command: crate::game::SlashCommand::Clan {
                action: crate::game::ClanAction::Create {
                    name: "Kernel clan".to_string(),
                    tag: "KRN".to_string(),
                },
            },
        },
    );

    assert!(effects.events.is_empty());
    assert!(matches!(
        effects.saves.as_slice(),
        [crate::game::SaveCommand::ClanCommand { request }]
            if request.player_id == player_id
                && request.session_id == session_id
                && request.create_reserved
    ));
    assert!(
        receiver.try_recv().is_err(),
        "dispatch must not write clan packets"
    );
    let creds = test.state.query_player(player_id, |ecs, entity| {
        ecs.get::<crate::game::player::PlayerStats>(entity)
            .expect("connected player stats")
            .creds
    });
    assert_eq!(
        creds,
        Some(0),
        "create cost must be reserved before persistence"
    );
}

#[tokio::test]
async fn programmer_menu_is_admitted_without_session_task() {
    let test =
        crate::test_support::ServerTestHarness::new("program_menu_durable", "programmer").await;
    let player_id = crate::game::PlayerId(test.player.id);
    let session_id = crate::game::SessionId::new(202);
    let mut receiver = test.connect(session_id.get());
    crate::test_support::ServerTestHarness::drain_events(&mut receiver);

    let effects = apply_player_command(
        &test.state,
        player_id,
        session_id,
        crate::game::PlayerCommand::OpenProgrammer,
    );

    assert!(effects.events.is_empty());
    assert!(matches!(
        effects.saves.as_slice(),
        [crate::game::SaveCommand::ProgramMenu { request }]
            if request.player_id == player_id && request.session_id == session_id
    ));
    assert!(
        receiver.try_recv().is_err(),
        "dispatch must not write programmer GUI"
    );
}

#[tokio::test]
async fn program_copy_is_admitted_without_session_task() {
    let test =
        crate::test_support::ServerTestHarness::new("program_copy_durable", "programmer").await;
    let player_id = crate::game::PlayerId(test.player.id);
    let session_id = crate::game::SessionId::new(204);
    let mut receiver = test.connect(session_id.get());
    crate::test_support::ServerTestHarness::drain_events(&mut receiver);

    let effects = apply_player_command(
        &test.state,
        player_id,
        session_id,
        crate::game::PlayerCommand::ProgramAction {
            event: "PCOP".to_string(),
            payload: bytes::Bytes::from_static(b"42"),
        },
    );

    assert!(effects.events.is_empty());
    assert!(matches!(
        effects.saves.as_slice(),
        [crate::game::SaveCommand::ProgramCopy { request }]
            if request.player == player_id && request.session == session_id && request.program == 42
    ));
    assert!(
        receiver.try_recv().is_err(),
        "dispatch must not write programmer packets"
    );
}

#[tokio::test]
async fn copied_program_queues_menu_through_persistence() {
    let test =
        crate::test_support::ServerTestHarness::new("program_copy_completion", "programmer").await;
    let player_id = crate::game::PlayerId(test.player.id);
    let session_id = crate::game::SessionId::new(205);
    let mut receiver = test.connect(session_id.get());
    crate::test_support::ServerTestHarness::drain_events(&mut receiver);

    let effects = apply_persistence_completion(
        &test.state,
        crate::game::PersistenceCompletion::ProgramCopied {
            request: crate::game::ProgramCopyRequest {
                player: player_id,
                session: session_id,
                program: 42,
            },
            result: crate::game::ProgramCopyResult::Copied,
        },
    );

    assert!(effects.events.is_empty());
    assert!(matches!(
        effects.saves.as_slice(),
        [crate::game::SaveCommand::ProgramMenu { request }]
            if request.player_id == player_id && request.session_id == session_id
    ));
    assert!(
        receiver.try_recv().is_err(),
        "completion must not bypass presentation"
    );
}

#[tokio::test]
async fn building_menu_is_admitted_without_session_task() {
    let test =
        crate::test_support::ServerTestHarness::new("building_menu_durable", "builder").await;
    let player_id = crate::game::PlayerId(test.player.id);
    let session_id = crate::game::SessionId::new(206);
    let mut receiver = test.connect(session_id.get());
    crate::test_support::ServerTestHarness::drain_events(&mut receiver);

    let effects = apply_player_command(
        &test.state,
        player_id,
        session_id,
        crate::game::PlayerCommand::RequestMyBuildings,
    );

    assert!(effects.events.is_empty());
    assert!(matches!(
        effects.saves.as_slice(),
        [crate::game::SaveCommand::BuildingMenu { request }]
            if request.player_id == player_id && request.session_id == session_id
    ));
    assert!(receiver.try_recv().is_err());
}

#[tokio::test]
async fn whois_is_admitted_and_completion_preserves_wire_order() {
    let test = crate::test_support::ServerTestHarness::new("whois_durable", "online-name").await;
    let player_id = crate::game::PlayerId(test.player.id);
    let session_id = crate::game::SessionId::new(207);
    let mut receiver = test.connect(session_id.get());
    crate::test_support::ServerTestHarness::drain_events(&mut receiver);

    let effects = apply_player_command(
        &test.state,
        player_id,
        session_id,
        crate::game::PlayerCommand::Whois {
            ids: vec![player_id.as_i32(), 9_999, player_id.as_i32()],
        },
    );
    assert!(effects.events.is_empty());
    let request = match effects.saves.as_slice() {
        [crate::game::SaveCommand::Whois { request }] => request.clone(),
        _ => panic!("whois must enqueue exactly one persistence request"),
    };
    assert_eq!(
        request.ids,
        vec![player_id.as_i32(), 9_999, player_id.as_i32()]
    );
    assert_eq!(
        request.online_names,
        vec![(player_id.as_i32(), "online-name".to_string())]
    );
    assert!(
        receiver.try_recv().is_err(),
        "dispatch must not send NL directly"
    );

    let effects = apply_persistence_completion(
        &test.state,
        crate::game::PersistenceCompletion::WhoisLoaded {
            request,
            result: crate::game::WhoisResult::Loaded {
                names: vec![
                    (player_id.as_i32(), "online-name".to_string()),
                    (9_999, String::new()),
                    (player_id.as_i32(), "online-name".to_string()),
                ],
            },
        },
    );
    let [crate::game::GameEvent::SessionBatch { packets, .. }] = effects.events.as_slice() else {
        panic!("whois completion must produce one session batch");
    };
    let mut bytes = bytes::BytesMut::from(packets[0].as_slice());
    let packet = openmines_protocol::Packet::try_decode(&mut bytes)
        .expect("NL frame must decode")
        .expect("NL frame must be complete");
    assert_eq!(packet.event_name, *b"NL");
    assert_eq!(
        packet.payload,
        format!(
            "{}:online-name,9999:,{}:online-name",
            player_id.as_i32(),
            player_id.as_i32()
        )
        .into_bytes()
    );
}

#[tokio::test]
async fn clan_menu_completion_emits_legacy_gui_for_current_session() {
    let test = crate::test_support::ServerTestHarness::new("clan_menu_completion", "clan-ui").await;
    let player_id = crate::game::PlayerId(test.player.id);
    let session_id = crate::game::SessionId::new(208);
    let mut receiver = test.connect(session_id.get());
    crate::test_support::ServerTestHarness::drain_events(&mut receiver);

    let effects = apply_persistence_completion(
        &test.state,
        crate::game::PersistenceCompletion::ClanMenuLoaded {
            request: crate::game::ClanMenuRequest {
                player_id,
                session_id,
                player_clan_id: None,
                action: crate::game::ClanMenuAction::Main,
                invite_candidates: Vec::new(),
            },
            result: crate::game::ClanMenuResult::Browse {
                invites: Vec::new(),
                clans: vec![crate::game::ClanMenuListEntry {
                    id: 42,
                    name: "Kernel".to_string(),
                    abr: "KRN".to_string(),
                    member_count: 3,
                }],
            },
        },
    );

    let [crate::game::GameEvent::SessionBatch { packets, .. }] = effects.events.as_slice() else {
        panic!("clan menu completion must produce one session batch");
    };
    let mut bytes = bytes::BytesMut::from(packets[0].as_slice());
    let packet = openmines_protocol::Packet::try_decode(&mut bytes)
        .expect("clan GUI frame must decode")
        .expect("clan GUI frame must be complete");
    assert_eq!(packet.event_name, *b"GU");
    let payload = String::from_utf8(packet.payload.to_vec()).expect("GUI payload must be UTF-8");
    assert!(payload.contains("КЛАНЫ"));
    assert!(payload.contains("clan_create"));
    assert!(payload.contains("clan_view:42"));
}

#[tokio::test]
async fn clan_read_buttons_enqueue_the_matching_menu_action() {
    let test = crate::test_support::ServerTestHarness::new("clan_read_actions", "clan-read").await;
    let player_id = crate::game::PlayerId(test.player.id);
    let session_id = crate::game::SessionId::new(209);
    let _receiver = test.connect(session_id.get());

    for (button, action) in [
        ("clan_members", crate::game::ClanMenuAction::Members),
        ("clan_invite_list", crate::game::ClanMenuAction::InviteList),
        ("clan_invites_view", crate::game::ClanMenuAction::Invites),
        ("clan_requests", crate::game::ClanMenuAction::Requests),
    ] {
        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::parse(button.to_string()),
            },
        );
        assert!(matches!(
            effects.saves.as_slice(),
            [crate::game::SaveCommand::ClanMenu { request }]
                if request.action == action
        ));
    }
}

#[tokio::test]
async fn clan_mutation_buttons_enqueue_typed_mutations() {
    let test =
        crate::test_support::ServerTestHarness::new("clan_mutation_actions", "clan-mutation").await;
    let player_id = crate::game::PlayerId(test.player.id);
    let session_id = crate::game::SessionId::new(210);
    let _receiver = test.connect(session_id.get());

    for (button, action) in [
        ("clan_leave", crate::game::ClanAction::Leave),
        (
            "clan_invite_accept:17",
            crate::game::ClanAction::AcceptInvite { clan_id: 17 },
        ),
        (
            "clan_invite_decline:17",
            crate::game::ClanAction::DeclineInvite { clan_id: 17 },
        ),
        (
            "clan_accept:17",
            crate::game::ClanAction::AcceptRequest {
                target_id: crate::game::PlayerId::from(17),
            },
        ),
        (
            "clan_decline:17",
            crate::game::ClanAction::DeclineRequest {
                target_id: crate::game::PlayerId::from(17),
            },
        ),
        (
            "clan_promote:17",
            crate::game::ClanAction::Promote {
                target_id: crate::game::PlayerId::from(17),
            },
        ),
        (
            "clan_kick_id:17",
            crate::game::ClanAction::KickById {
                target_id: crate::game::PlayerId::from(17),
            },
        ),
        (
            "clan_invite_send:17",
            crate::game::ClanAction::Invite {
                target_id: crate::game::PlayerId::from(17),
            },
        ),
        (
            "clan_request:17",
            crate::game::ClanAction::Request { clan_id: 17 },
        ),
    ] {
        let effects = apply_player_command(
            &test.state,
            player_id,
            session_id,
            crate::game::PlayerCommand::Gui {
                command: crate::game::GuiCommand::parse(button.to_string()),
            },
        );
        assert!(matches!(
            effects.saves.as_slice(),
            [crate::game::SaveCommand::ClanCommand { request }]
                if request.action == action && !request.create_reserved
        ));
    }
}

#[tokio::test]
async fn clan_invite_join_completion_does_not_write_to_a_replaced_session() {
    let test =
        crate::test_support::ServerTestHarness::new("clan_invite_stale_session", "clan-stale")
            .await;
    let player_id = crate::game::PlayerId(test.player.id);
    let stale_session = crate::game::SessionId::new(211);
    let _stale_receiver = test.connect(stale_session.get());
    let current_session = crate::game::SessionId::new(212);
    let _current_receiver = test.connect(current_session.get());

    let effects = apply_persistence_completion(
        &test.state,
        crate::game::PersistenceCompletion::ClanCommandApplied {
            request: crate::game::ClanCommandRequest {
                player_id,
                session_id: stale_session,
                action: crate::game::ClanAction::AcceptInvite { clan_id: 17 },
                create_reserved: false,
            },
            result: crate::game::ClanCommandResult::Joined { clan_id: 17 },
        },
    );

    assert!(
        effects.events.is_empty(),
        "stale session must receive no cS/OK"
    );
}

#[tokio::test]
async fn programmer_menu_completion_emits_gui_for_current_session() {
    let test =
        crate::test_support::ServerTestHarness::new("program_menu_completion", "programmer").await;
    let player_id = crate::game::PlayerId(test.player.id);
    let session_id = crate::game::SessionId::new(203);
    let mut receiver = test.connect(session_id.get());
    crate::test_support::ServerTestHarness::drain_events(&mut receiver);

    let effects = apply_persistence_completion(
        &test.state,
        crate::game::PersistenceCompletion::ProgramMenuLoaded {
            request: crate::game::ProgramMenuRequest {
                player_id,
                session_id,
            },
            result: crate::game::ProgramMenuResult::Loaded {
                programs: Vec::new(),
            },
        },
    );

    assert!(matches!(
        effects.events.as_slice(),
        [crate::game::GameEvent::SessionBatch { session_id: current, packets, .. }]
            if *current == session_id
                && openmines_protocol::Packet::try_decode(
                    &mut bytes::BytesMut::from(packets[0].as_slice())
                )
                .expect("programmer GUI packet must decode")
                .is_some_and(|packet| packet.event_name == *b"GU")
    ));
}

#[tokio::test]
async fn role_is_admitted_without_session_task() {
    let test = crate::test_support::ServerTestHarness::new("role_admission", "role-admin").await;
    let player_id = crate::game::PlayerId(test.player.id);
    let session_id = crate::game::SessionId::new(103);
    let mut receiver = test.connect(session_id.get());
    crate::test_support::ServerTestHarness::drain_events(&mut receiver);
    test.state.modify_player(player_id, |ecs, entity| {
        ecs.get_mut::<crate::game::player::PlayerStats>(entity)
            .expect("connected player stats")
            .role = 2;
    });

    let effects = apply_player_command(
        &test.state,
        player_id,
        session_id,
        crate::game::PlayerCommand::Slash {
            command: crate::game::SlashCommand::Role {
                target: "target".to_owned(),
                role: crate::db::Role::Moderator,
            },
        },
    );
    assert!(matches!(
        effects.saves.as_slice(),
        [crate::game::SaveCommand::AdminRole { request }]
            if request.player_id == player_id
                && request.session_id == session_id
                && request.target_name == "target"
                && request.role == crate::db::Role::Moderator
    ));
    assert!(effects.events.is_empty());
}

#[tokio::test]
async fn skill_is_admitted_for_persistence_without_outbox_delivery() {
    let test = crate::test_support::ServerTestHarness::new("skill_admission", "skill-admin").await;
    let player_id = crate::game::PlayerId(test.player.id);
    let session_id = crate::game::SessionId::new(104);
    let mut receiver = test.connect(session_id.get());
    crate::test_support::ServerTestHarness::drain_events(&mut receiver);
    test.state.modify_player(player_id, |ecs, entity| {
        ecs.get_mut::<crate::game::player::PlayerStats>(entity)
            .expect("connected player stats")
            .role = 2;
    });

    let effects = apply_player_command(
        &test.state,
        player_id,
        session_id,
        crate::game::PlayerCommand::Slash {
            command: crate::game::SlashCommand::Skill {
                target: "me".to_owned(),
                code: "U".to_owned(),
                level: 5,
                slot: Some(0),
                exp: 1.0,
            },
        },
    );
    assert!(matches!(
        effects.saves.as_slice(),
        [crate::game::SaveCommand::AdminSkill { request }]
            if request.player_id == player_id
                && request.target_id == player_id
                && request.session_id == session_id
                && request.target_session_id == Some(session_id)
                && request.skill_code == "U"
                && request.level == 5
                && request.slot == 0
                && (request.exp - 1.0).abs() < f32::EPSILON
    ));
    assert!(effects.events.is_empty());
    assert!(
        receiver.try_recv().is_err(),
        "dispatch must not write skill packets"
    );
}

#[tokio::test]
async fn teleport_emits_refresh_event_without_session_helper() {
    let test = crate::test_support::ServerTestHarness::new("teleport_effect", "tp-admin").await;
    let player_id = crate::game::PlayerId(test.player.id);
    let session_id = crate::game::SessionId::new(105);
    let mut receiver = test.connect(session_id.get());
    crate::test_support::ServerTestHarness::drain_events(&mut receiver);
    test.state.modify_player(player_id, |ecs, entity| {
        ecs.get_mut::<crate::game::player::PlayerStats>(entity)
            .expect("connected player stats")
            .role = 2;
    });

    let effects = apply_player_command(
        &test.state,
        player_id,
        session_id,
        crate::game::PlayerCommand::Slash {
            command: crate::game::SlashCommand::Teleport { x: 12, y: 12 },
        },
    );
    assert!(matches!(
        effects.events.as_slice(),
        [crate::game::GameEvent::SessionBatch { session_id: current, packets, .. },
         crate::game::GameEvent::RefreshChunks { session_id: refresh, player_id: target }]
            if *current == session_id
                && *refresh == session_id
                && *target == player_id
                && openmines_protocol::Packet::try_decode(&mut bytes::BytesMut::from(packets[0].as_slice()))
                    .expect("teleport packet must decode")
                    .is_some_and(|packet| packet.event_name == *b"@T")
    ));
    assert!(
        receiver.try_recv().is_err(),
        "dispatch must not refresh chunks directly"
    );
}
