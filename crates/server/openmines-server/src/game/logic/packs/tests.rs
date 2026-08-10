use super::*;
use crate::game::player::PlayerUI;
use crate::test_support::{ServerTestHarness, ServerTestHarnessBuilder, drain_events};

#[tokio::test]
async fn resp_fill_missing_player_flags_is_explicit_error_without_charge_or_crystal_mutation() {
    let test = make_charge_fill_test_state("resp_missing_flags", "R", 1, 100).await;
    let (tx, mut rx) = test.connect_with_outbox(1);
    drain_events(&mut rx);

    let player_entity = test.state.get_player_entity(test.player.id.into()).unwrap();
    {
        let mut ecs = test.state.ecs.write();
        ecs.entity_mut(player_entity).remove::<PlayerFlags>();
    }
    let before_crystals = player_crystals(&test.state, test.player.id.into());
    let before_charge = building_charge(&test.state, 10, 10);

    handle_resp_fill(&test.state, &tx, test.player.id.into(), "100", 10, 10);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "OK");
    assert_eq!(events[0].1, "РЕСП#Состояние респа недоступно.".as_bytes());
    assert_eq!(
        player_crystals(&test.state, test.player.id.into()),
        before_crystals
    );
    assert_eq!(building_charge(&test.state, 10, 10), before_charge);
}

#[tokio::test]
async fn resp_bind_missing_player_flags_is_explicit_error_without_resp_mutation() {
    let test = make_charge_fill_test_state("resp_bind_missing_flags", "R", 1, 100).await;
    let (tx, mut rx) = test.connect_with_outbox(1);
    drain_events(&mut rx);

    let player_entity = test.state.get_player_entity(test.player.id.into()).unwrap();
    {
        let mut ecs = test.state.ecs.write();
        ecs.entity_mut(player_entity).remove::<PlayerFlags>();
    }
    let before_resp = player_resp(&test.state, test.player.id.into());

    handle_resp_bind(&test.state, &tx, test.player.id.into(), 10, 10);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "OK");
    assert_eq!(events[0].1, "РЕСП#Состояние респа недоступно.".as_bytes());
    assert_eq!(player_resp(&test.state, test.player.id.into()), before_resp);
}

#[tokio::test]
async fn gun_fill_missing_player_flags_is_explicit_error_without_charge_or_crystal_mutation() {
    let test = make_charge_fill_test_state("gun_missing_flags", "G", 5, 100).await;
    let (tx, mut rx) = test.connect_with_outbox(1);
    drain_events(&mut rx);

    let player_entity = test.state.get_player_entity(test.player.id.into()).unwrap();
    {
        let mut ecs = test.state.ecs.write();
        ecs.entity_mut(player_entity).remove::<PlayerFlags>();
    }
    let before_crystals = player_crystals(&test.state, test.player.id.into());
    let before_charge = building_charge(&test.state, 10, 10);

    handle_gun_fill(&test.state, &tx, test.player.id.into(), "100", 10, 10);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "OK");
    assert_eq!(events[0].1, "Пушка#Состояние пушки недоступно.".as_bytes());
    assert_eq!(
        player_crystals(&test.state, test.player.id.into()),
        before_crystals
    );
    assert_eq!(building_charge(&test.state, 10, 10), before_charge);
}

#[tokio::test]
async fn gun_fill_allows_non_owner_like_reference() {
    let test = make_charge_fill_test_state("gun_non_owner_fill", "G", 5, 0).await;
    let mut filler = test.create_player("gun-fill-visitor").await;
    filler.x = 10;
    filler.y = 10;
    filler.crystals[5] = 100;

    let (tx, mut rx) = test.connect_player_with_outbox(&filler, 2);
    drain_events(&mut rx);

    handle_gun_fill(&test.state, &tx, filler.id.into(), "100", 10, 10);

    let events = drain_events(&mut rx);
    assert_eq!(building_charge(&test.state, 10, 10), 100);
    assert_eq!(player_crystals(&test.state, filler.id.into())[5], 0);
    assert!(
        events.iter().any(|(event, _)| event == "@B"),
        "events: {events:?}"
    );
    assert!(
        events.iter().any(|(event, _)| event == "GU"),
        "events: {events:?}"
    );
}

#[tokio::test]
async fn resp_profit_missing_player_flags_is_explicit_error_without_money_mutation() {
    let test = make_charge_fill_test_state("resp_profit_missing_flags", "R", 1, 100).await;
    let (tx, mut rx) = test.connect_with_outbox(1);
    drain_events(&mut rx);

    let player_entity = test.state.get_player_entity(test.player.id.into()).unwrap();
    let building_entity = test.state.building_entity_at(10, 10).unwrap();
    {
        let mut ecs = test.state.ecs.write();
        ecs.get_mut::<BuildingStorage>(building_entity)
            .unwrap()
            .money = 777;
        ecs.entity_mut(player_entity).remove::<PlayerFlags>();
    }
    let before_money = player_money(&test.state, test.player.id.into());

    handle_resp_profit(&test.state, &tx, test.player.id.into(), 10, 10);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "OK");
    assert_eq!(events[0].1, "РЕСП#Состояние респа недоступно.".as_bytes());
    assert_eq!(
        player_money(&test.state, test.player.id.into()),
        before_money
    );
    assert_eq!(building_storage_money(&test.state, 10, 10), 777);
}

#[tokio::test]
async fn resp_profit_success_moves_money_and_marks_player_and_building_dirty() {
    let test = make_charge_fill_test_state("resp_profit_success", "R", 1, 100).await;
    let (tx, mut rx) = test.connect_with_outbox(1);
    drain_events(&mut rx);

    let building_entity = test.state.building_entity_at(10, 10).unwrap();
    {
        let mut ecs = test.state.ecs.write();
        ecs.get_mut::<BuildingStorage>(building_entity)
            .unwrap()
            .money = 777;
    }
    let before_money = player_money(&test.state, test.player.id.into());

    handle_resp_profit(&test.state, &tx, test.player.id.into(), 10, 10);

    let events = drain_events(&mut rx);
    assert!(events.iter().any(|(event, _)| event == "P$"));
    assert_eq!(
        player_money(&test.state, test.player.id.into()),
        before_money + 777
    );
    assert_eq!(building_storage_money(&test.state, 10, 10), 0);
    assert!(player_dirty(&test.state, test.player.id.into()));
    assert!(building_dirty(&test.state, 10, 10));
}

#[tokio::test]
async fn resp_save_rejects_malformed_cost_without_cost_mutation() {
    let test = make_charge_fill_test_state("resp_save_bad_cost", "R", 1, 100).await;
    let (tx, mut rx) = test.connect_with_outbox(1);
    drain_events(&mut rx);

    set_player_window(&test.state, test.player.id.into(), "resp:10:10");
    let before_cost = building_cost(&test.state, 10, 10);

    handle_resp_save(&test.state, &tx, test.player.id.into(), "cost:nope");

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "OK");
    assert_eq!(events[0].1, "РЕСП#Некорректное действие.".as_bytes());
    assert_eq!(building_cost(&test.state, 10, 10), before_cost);
}

#[tokio::test]
async fn resp_save_missing_player_stats_is_explicit_error_without_partial_cost_mutation() {
    let test = make_charge_fill_test_state("resp_save_missing_player_stats", "R", 1, 100).await;
    let (tx, mut rx) = test.connect_with_outbox(1);
    drain_events(&mut rx);

    set_player_window(&test.state, test.player.id.into(), "resp:10:10");
    let player_entity = test.state.get_player_entity(test.player.id.into()).unwrap();
    {
        let mut ecs = test.state.ecs.write();
        ecs.entity_mut(player_entity).remove::<PlayerStats>();
    }
    let before_cost = building_cost(&test.state, 10, 10);

    handle_resp_save(&test.state, &tx, test.player.id.into(), "cost:123#clan:1");

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "OK");
    assert_eq!(events[0].1, "РЕСП#Состояние респа недоступно.".as_bytes());
    assert_eq!(building_cost(&test.state, 10, 10), before_cost);
}

#[tokio::test]
async fn resp_save_updates_clanzone_marks_dirty_and_refreshes_admin_gui() {
    let test = make_charge_fill_test_state("resp_save_clanzone", "R", 1, 100).await;
    let (tx, mut rx) = test.connect_with_outbox(1);
    drain_events(&mut rx);

    set_player_window(&test.state, test.player.id.into(), "resp:10:10");
    assert_eq!(building_clanzone(&test.state, 10, 10), 0);

    handle_resp_save(&test.state, &tx, test.player.id.into(), "clanzone:321#");

    let events = drain_events(&mut rx);
    assert_eq!(building_clanzone(&test.state, 10, 10), 321);
    assert!(building_dirty(&test.state, 10, 10));
    assert!(
        events.iter().any(|(event, payload)| {
            event == "GU" && String::from_utf8_lossy(payload).contains("321")
        }),
        "admin GUI refresh must include updated clanzone"
    );
}

#[tokio::test]
async fn resp_save_client_richlist_payload_returns_typed_building_effect() {
    let test = make_charge_fill_test_state("typed_resp_save", "R", 1, 100).await;
    let session_id = crate::game::SessionId::new(77);
    let (_tx, mut rx) = test.connect_with_outbox(session_id.get());
    drain_events(&mut rx);
    set_player_window(&test.state, test.player.id.into(), "resp:10:10");

    let effects = crate::game::logic::commands::apply_player_command(
        &test.state,
        test.player.id.into(),
        session_id,
        crate::game::PlayerCommand::Gui {
            command: crate::game::GuiCommand::parse(
                "resp_save:#:0#cost:123##clan:1#clanzone:321#".to_owned(),
            ),
        },
    );

    assert!(rx.try_recv().is_err());
    assert!(matches!(
        effects.saves.as_slice(),
        [crate::game::SaveCommand::Building { row }]
            if row.cost == 123 && row.clan_id == 0 && row.clanzone == 321
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
    assert_eq!(building_cost(&test.state, 10, 10), 123);
    assert_eq!(building_clanzone(&test.state, 10, 10), 321);
}

#[tokio::test]
async fn resp_bind_gui_command_returns_typed_player_save() {
    let test = make_charge_fill_test_state("typed_resp_bind", "R", 1, 100).await;
    let session_id = crate::game::SessionId::new(79);
    let (_tx, mut rx) = test.connect_with_outbox(session_id.get());
    drain_events(&mut rx);

    let effects = crate::game::logic::commands::apply_player_command(
        &test.state,
        test.player.id.into(),
        session_id,
        crate::game::PlayerCommand::Gui {
            command: crate::game::GuiCommand::parse("resp_bind:10:10".to_owned()),
        },
    );

    assert!(rx.try_recv().is_err());
    assert!(matches!(
        effects.saves.as_slice(),
        [crate::game::SaveCommand::Player { row }]
            if row.resp_x == Some(10) && row.resp_y == Some(10)
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
}

#[tokio::test]
async fn gun_fill_prog_missing_building_stats_does_not_dirty_building() {
    let test = make_charge_fill_test_state("gun_prog_missing_stats", "G", 5, 100).await;
    let (tx, _rx) = crate::net::session::outbox::channel();
    let building_entity = test.state.building_entity_at(10, 10).unwrap();
    {
        let mut ecs = test.state.ecs.write();
        ecs.entity_mut(building_entity).remove::<BuildingStats>();
    }

    handle_gun_fill_prog(&test.state, &tx, test.player.id.into(), 10, 10);

    assert!(!building_dirty(&test.state, 10, 10));
}

async fn make_charge_fill_test_state(
    label: &str,
    building_code: &str,
    crystal_index: usize,
    crystal_amount: i64,
) -> ServerTestHarness {
    let mut builder =
        ServerTestHarnessBuilder::new(&format!("charge_fill_{label}"), "charge-fill-user").await;
    builder.player.x = 10;
    builder.player.y = 10;
    builder.player.crystals[crystal_index] = crystal_amount;

    let extra = crate::db::BuildingExtra {
        charge: 0,
        max_charge: 1000,
        cost: 0,
        hp: 1000,
        max_hp: 1000,
        money_inside: 0,
        crystals_inside: [0; 6],
        items_inside: std::collections::HashMap::new(),
        craft_recipe_id: None,
        craft_num: 0,
        craft_end_ts: 0,
        craft_ready: false,
        clanzone: 0,
    };
    builder
        .database()
        .insert_building(building_code, 10, 10, builder.player.id, 0, &extra)
        .await
        .unwrap();

    builder.build().await
}

fn player_crystals(state: &Arc<GameState>, pid: PlayerId) -> [i64; 6] {
    state
        .query_player_opt(pid, |ecs, entity| {
            Some(ecs.get::<PlayerStats>(entity)?.crystals)
        })
        .unwrap()
}

fn building_charge(state: &Arc<GameState>, x: i32, y: i32) -> i32 {
    state
        .query_building_opt(x, y, |ecs, entity| {
            Some(ecs.get::<BuildingStats>(entity)?.charge)
        })
        .unwrap()
}

fn building_cost(state: &Arc<GameState>, x: i32, y: i32) -> i32 {
    state
        .query_building_opt(x, y, |ecs, entity| {
            Some(ecs.get::<BuildingStats>(entity)?.cost)
        })
        .unwrap()
}

fn building_clanzone(state: &Arc<GameState>, x: i32, y: i32) -> i32 {
    state
        .query_building_opt(x, y, |ecs, entity| {
            Some(ecs.get::<BuildingStats>(entity)?.clanzone)
        })
        .unwrap()
}

fn player_money(state: &Arc<GameState>, pid: PlayerId) -> i64 {
    state
        .query_player_opt(pid, |ecs, entity| {
            Some(ecs.get::<PlayerStats>(entity)?.money)
        })
        .unwrap()
}

fn player_resp(state: &Arc<GameState>, pid: PlayerId) -> (Option<i32>, Option<i32>) {
    state
        .query_player_opt(pid, |ecs, entity| {
            let meta = ecs.get::<PlayerMetadata>(entity)?;
            Some((meta.resp_x, meta.resp_y))
        })
        .unwrap()
}

fn building_storage_money(state: &Arc<GameState>, x: i32, y: i32) -> i64 {
    state
        .query_building_opt(x, y, |ecs, entity| {
            Some(ecs.get::<BuildingStorage>(entity)?.money)
        })
        .unwrap()
}

fn player_dirty(state: &Arc<GameState>, pid: PlayerId) -> bool {
    state
        .query_player_opt(pid, |ecs, entity| {
            Some(ecs.get::<PlayerFlags>(entity)?.dirty)
        })
        .unwrap()
}

fn building_dirty(state: &Arc<GameState>, x: i32, y: i32) -> bool {
    state
        .query_building_opt(x, y, |ecs, entity| {
            Some(ecs.get::<BuildingFlags>(entity)?.dirty)
        })
        .unwrap()
}

fn set_player_window(state: &Arc<GameState>, pid: PlayerId, window: &str) {
    state.modify_player(pid, |ecs, entity| {
        let mut ui = ecs.get_mut::<PlayerUI>(entity)?;
        ui.current_window = Some(window.to_string());
        Some(())
    });
}
