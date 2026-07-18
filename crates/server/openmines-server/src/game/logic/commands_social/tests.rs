use super::*;
use crate::game::player::{PlayerFlags, PlayerPosition};
use crate::test_support::{ServerTestHarness, drain_events};

async fn make_command_test_state(label: &str) -> ServerTestHarness {
    ServerTestHarness::new(&format!("commands_{label}"), "command-user").await
}

fn make_admin_and_remove_flags(game_state: &Arc<GameState>, pid: PlayerId) {
    let entity = game_state.get_player_entity(pid).unwrap();
    let mut ecs = game_state.ecs.write();
    let mut admin_stats = ecs.get_mut::<PlayerStats>(entity).unwrap();
    admin_stats.role = 2;
    ecs.entity_mut(entity).remove::<PlayerFlags>();
}

fn make_admin(game_state: &Arc<GameState>, pid: PlayerId) {
    let entity = game_state.get_player_entity(pid).unwrap();
    let mut ecs = game_state.ecs.write();
    let mut admin_stats = ecs.get_mut::<PlayerStats>(entity).unwrap();
    admin_stats.role = 2;
}

fn player_money(game_state: &Arc<GameState>, pid: PlayerId) -> i64 {
    game_state
        .query_player_opt(pid, |ecs, entity| {
            let money_stats = ecs.get::<PlayerStats>(entity)?;
            Some(money_stats.money)
        })
        .unwrap()
}

fn player_pos(game_state: &Arc<GameState>, pid: PlayerId) -> (i32, i32) {
    game_state
        .query_player_opt(pid, |ecs, entity| {
            let pos = ecs.get::<PlayerPosition>(entity)?;
            Some((pos.x, pos.y))
        })
        .unwrap()
}

fn player_skill_entry(game_state: &Arc<GameState>, pid: PlayerId, slot: i32) -> Option<SkillEntry> {
    game_state.query_player_opt(pid, |ecs, entity| {
        let skills = ecs.get::<PlayerSkillsComp>(entity)?;
        skills.states.skills.get(&slot).cloned()
    })
}

fn player_skill_count(game_state: &Arc<GameState>, pid: PlayerId, code: &str) -> usize {
    game_state
        .query_player_opt(pid, |ecs, entity| {
            let skills = ecs.get::<PlayerSkillsComp>(entity)?;
            Some(
                skills
                    .states
                    .skills
                    .values()
                    .filter(|entry| entry.code == code)
                    .count(),
            )
        })
        .unwrap()
}

#[tokio::test]
async fn skill_sets_wire_code_slot_and_syncs_player_packets() {
    let test = make_command_test_state("skill_set").await;
    let (tx, mut rx) = test.connect_with_outbox(1);
    drain_events(&mut rx);

    let pid = PlayerId(test.player.id);
    make_admin(&test.state, pid);

    handle_chat_skill_command(&test.state, &tx, pid, &["me", "U", "200", "10", "900000"]).await;

    let entry = player_skill_entry(&test.state, pid, 10).unwrap();
    assert_eq!(entry.code, SkillType::Geology.code());
    assert_eq!(entry.level, 200);
    assert!((entry.exp - 900_000.0).abs() < f32::EPSILON);
    assert_eq!(
        player_skill_count(&test.state, pid, SkillType::Geology.code()),
        1
    );
    let saved = test
        .state
        .db
        .get_player_by_id(test.player.id)
        .await
        .unwrap()
        .unwrap();
    let saved_entry = saved.skills.skills.get(&10).unwrap();
    assert_eq!(saved_entry.code, SkillType::Geology.code());
    assert_eq!(saved_entry.level, 200);
    assert!((saved_entry.exp - 900_000.0).abs() < f32::EPSILON);

    let events = drain_events(&mut rx);
    assert!(events.iter().any(|(event, _)| event == "@S"));
    assert!(events.iter().any(|(event, _)| event == "LV"));
    assert!(events.iter().any(|(event, _)| event == "sp"));
    assert!(events.iter().any(|(event, _)| event == "@L"));
    assert!(events.iter().any(|(event, _)| event == "@B"));
    assert_eq!(events.last().unwrap().0, "OK");
}

#[tokio::test]
async fn skill_exp_above_wire_limit_is_rejected_before_mutation() {
    let test = make_command_test_state("skill_max_exp").await;
    let (tx, mut rx) = test.connect_with_outbox(1);
    drain_events(&mut rx);

    let pid = PlayerId(test.player.id);
    make_admin(&test.state, pid);
    let command = format!("/skill me U 200 10 {}", f32::MAX);

    handle_chat_command(&test.state, &tx, pid, &command).await;

    assert!(player_skill_entry(&test.state, pid, 10).is_none());
    let saved = test
        .state
        .db
        .get_player_by_id(test.player.id)
        .await
        .unwrap()
        .unwrap();
    assert!(!saved.skills.skills.contains_key(&10));

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "OK");
    let message = std::str::from_utf8(&events[0].1).unwrap();
    assert!(message.contains("EXP слишком большое"));
    assert!(message.contains("21474834"));
}

#[tokio::test]
async fn skill_rejects_slots_outside_domain_before_mutation() {
    let test = make_command_test_state("skill_slot_limit").await;
    let (tx, mut rx) = test.connect_with_outbox(1);
    drain_events(&mut rx);

    let pid = PlayerId(test.player.id);
    make_admin(&test.state, pid);
    handle_chat_command(&test.state, &tx, pid, "/skill me U 200 34 1").await;

    assert!(player_skill_entry(&test.state, pid, 34).is_none());
    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "OK");
    let message = std::str::from_utf8(&events[0].1).unwrap();
    assert!(message.contains("SLOT должен быть целым числом от 0 до 33"));
}

#[tokio::test]
async fn skill_unknown_code_is_explicit_wire_error_without_mutation() {
    let test = make_command_test_state("skill_unknown").await;
    let (tx, mut rx) = test.connect_with_outbox(1);
    drain_events(&mut rx);

    let pid = PlayerId(test.player.id);
    make_admin(&test.state, pid);

    handle_chat_skill_command(&test.state, &tx, pid, &["me", "GEO", "200", "10", "900000"]).await;

    assert!(player_skill_entry(&test.state, pid, 10).is_none());
    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "OK");
    let message = std::str::from_utf8(&events[0].1).unwrap();
    assert!(message.contains("Неизвестный wire/DB-код скилла"));
    assert!(message.contains("U=геология"));
}

#[tokio::test]
async fn skill_codes_help_is_generated_from_wire_codes() {
    let test = make_command_test_state("skill_codes").await;
    let (tx, mut rx) = test.connect_with_outbox(1);
    drain_events(&mut rx);

    let pid = PlayerId(test.player.id);
    make_admin(&test.state, pid);

    handle_chat_skill_command(&test.state, &tx, pid, &["codes"]).await;

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "OK");
    let message = std::str::from_utf8(&events[0].1).unwrap();
    assert!(message.contains("U=Geology"));
    assert!(message.contains("M=Movement"));
    assert!(message.contains("d=Digging"));
    assert!(!message.contains("GEO="));
}

#[tokio::test]
async fn skill_missing_flags_is_explicit_error_without_skill_mutation() {
    let test = make_command_test_state("skill_missing_flags").await;
    let (tx, mut rx) = test.connect_with_outbox(1);
    drain_events(&mut rx);

    let pid = PlayerId(test.player.id);
    make_admin_and_remove_flags(&test.state, pid);

    handle_chat_skill_command(&test.state, &tx, pid, &["me", "U", "200", "10", "900000"]).await;

    assert!(player_skill_entry(&test.state, pid, 10).is_none());
    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "OK");
    let message = std::str::from_utf8(&events[0].1).unwrap();
    assert!(message.contains("Состояние игрока недоступно."));
}

#[tokio::test]
async fn money_missing_flags_is_explicit_error_without_money_mutation() {
    let test = make_command_test_state("money_missing_flags").await;
    let (tx, mut rx) = test.connect_with_outbox(1);
    drain_events(&mut rx);

    let pid = PlayerId(test.player.id);
    make_admin_and_remove_flags(&test.state, pid);
    let before_money = player_money(&test.state, pid);

    handle_chat_money_command(&test.state, &tx, pid, &["50"]);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "OK");
    let message = std::str::from_utf8(&events[0].1).unwrap();
    assert!(message.contains("Состояние игрока недоступно."));
    assert_eq!(player_money(&test.state, pid), before_money);
}

#[tokio::test]
async fn teleport_missing_flags_is_explicit_error_without_tp_packet_or_position_mutation() {
    let test = make_command_test_state("tp_missing_flags").await;
    let (tx, mut rx) = test.connect_with_outbox(1);
    drain_events(&mut rx);

    let pid = PlayerId(test.player.id);
    make_admin_and_remove_flags(&test.state, pid);
    let before_pos = player_pos(&test.state, pid);

    handle_chat_teleport_command(&test.state, &tx, pid, &["12", "12"]);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "OK");
    let message = std::str::from_utf8(&events[0].1).unwrap();
    assert!(message.contains("Состояние игрока недоступно."));
    assert_eq!(player_pos(&test.state, pid), before_pos);
}
