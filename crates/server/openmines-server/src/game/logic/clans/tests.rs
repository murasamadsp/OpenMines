use super::*;
use crate::game::{PlayerFlags, PlayerStats};
use crate::test_support::{ServerTestHarness, drain_events};

async fn make_clan_test_state(label: &str) -> ServerTestHarness {
    ServerTestHarness::new(&format!("clans_{label}"), "clan-user").await
}

#[tokio::test]
async fn clan_create_missing_flags_is_explicit_error_without_db_mutation() {
    let test = make_clan_test_state("create_missing_flags").await;
    let (tx, mut rx) = test.connect_with_outbox(1);
    drain_events(&mut rx);

    let pid = PlayerId(test.player.id);
    let entity = test.state.get_player_entity(pid).unwrap();
    {
        let mut ecs = test.state.ecs.write();
        let mut stats = ecs.get_mut::<PlayerStats>(entity).unwrap();
        stats.creds = 1_000;
        ecs.entity_mut(entity).remove::<PlayerFlags>();
    }

    handle_clan_create(&test.state, &tx, pid, "NoFlags", "NFL").await;

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "OK");
    let message = std::str::from_utf8(&events[0].1).unwrap();
    assert!(message.contains("Состояние игрока недоступно."));
    assert!(test.state.db.list_clans().await.unwrap().is_empty());
}

#[tokio::test]
async fn invite_accept_missing_flags_is_explicit_error_without_db_mutation() {
    let test = make_clan_test_state("invite_missing_flags").await;
    let owner = test.create_player("clan-owner").await;
    test.state
        .db
        .create_clan(1, "Owner Clan", "OWN", owner.id)
        .await
        .unwrap();
    test.state
        .db
        .add_clan_invite(1, test.player.id)
        .await
        .unwrap();

    let (tx, mut rx) = test.connect_with_outbox(1);
    drain_events(&mut rx);

    let pid = PlayerId(test.player.id);
    let entity = test.state.get_player_entity(pid).unwrap();
    {
        let mut ecs = test.state.ecs.write();
        ecs.entity_mut(entity).remove::<PlayerFlags>();
    }

    handle_clan_invite_accept(&test.state, &tx, pid, 1).await;

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "OK");
    let message = std::str::from_utf8(&events[0].1).unwrap();
    assert!(message.contains("Состояние игрока недоступно."));
    assert_eq!(
        test.state
            .db
            .get_player_invites(test.player.id)
            .await
            .unwrap(),
        vec![(1, "Owner Clan".to_string())]
    );
}

#[tokio::test]
async fn leave_missing_flags_is_explicit_error_without_db_mutation() {
    let test = make_clan_test_state("leave_missing_flags").await;
    let owner = test.create_player("leave-owner").await;
    test.state
        .db
        .create_clan(1, "Leave Clan", "LVC", owner.id)
        .await
        .unwrap();
    test.state
        .db
        .add_clan_request(1, test.player.id)
        .await
        .unwrap();
    test.state
        .db
        .accept_clan_request(1, test.player.id)
        .await
        .unwrap();
    let player = test
        .state
        .db
        .get_player_by_id(test.player.id)
        .await
        .unwrap()
        .unwrap();

    let (tx, mut rx) = test.connect_player_with_outbox(&player, 1);
    drain_events(&mut rx);

    let pid = PlayerId(player.id);
    let entity = test.state.get_player_entity(pid).unwrap();
    {
        let mut ecs = test.state.ecs.write();
        ecs.entity_mut(entity).remove::<PlayerFlags>();
    }

    handle_clan_leave(&test.state, &tx, pid).await;

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "OK");
    let message = std::str::from_utf8(&events[0].1).unwrap();
    assert!(message.contains("Состояние игрока недоступно."));
    let db_player = test
        .state
        .db
        .get_player_by_id(player.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(db_player.clan_id, Some(1));
}

#[tokio::test]
async fn owner_cannot_dissolve_populated_clan() {
    let test = make_clan_test_state("owner_populated_leave").await;
    let member = test.create_player("clan-member").await;
    test.state
        .db
        .create_clan(1, "Populated Clan", "POP", test.player.id)
        .await
        .unwrap();
    test.state.db.add_clan_request(1, member.id).await.unwrap();
    test.state
        .db
        .accept_clan_request(1, member.id)
        .await
        .unwrap();
    let owner = test
        .state
        .db
        .get_player_by_id(test.player.id)
        .await
        .unwrap()
        .unwrap();

    let (tx, mut rx) = test.connect_player_with_outbox(&owner, 1);
    drain_events(&mut rx);

    handle_clan_leave(&test.state, &tx, PlayerId(owner.id)).await;

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "OK");
    let message = std::str::from_utf8(&events[0].1).unwrap();
    assert!(message.contains("Сначала передайте лидерство"));

    assert!(test.state.db.get_clan(1).await.unwrap().is_some());
    let db_owner = test
        .state
        .db
        .get_player_by_id(owner.id)
        .await
        .unwrap()
        .unwrap();
    let db_member = test
        .state
        .db
        .get_player_by_id(member.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(db_owner.clan_id, Some(1));
    assert_eq!(db_member.clan_id, Some(1));
}

#[tokio::test]
async fn clan_kick_offline_player_succeeds_in_db() {
    let test = make_clan_test_state("kick_offline").await;
    let mut owner = test.create_player("clan-owner").await;
    let target = test.create_player("clan-member").await;

    test.state
        .db
        .create_clan(1, "Kick Clan", "KCK", owner.id)
        .await
        .unwrap();
    test.state.db.add_clan_request(1, target.id).await.unwrap();
    test.state
        .db
        .accept_clan_request(1, target.id)
        .await
        .unwrap();

    owner.clan_id = Some(1);
    owner.clan_rank = crate::db::ClanRank::Leader as i32;

    let (tx, mut rx) = test.connect_player_with_outbox(&owner, 1);
    drain_events(&mut rx);

    handle_clan_kick(&test.state, &tx, PlayerId(owner.id), target.id).await;

    let db_target = test
        .state
        .db
        .get_player_by_id(target.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(db_target.clan_id, None);
}

#[tokio::test]
async fn clan_promote_offline_player_succeeds_in_db() {
    let test = make_clan_test_state("promote_offline").await;
    let mut owner = test.create_player("clan-owner").await;
    let target = test.create_player("clan-member").await;

    test.state
        .db
        .create_clan(1, "Promote Clan", "PRM", owner.id)
        .await
        .unwrap();
    test.state.db.add_clan_request(1, target.id).await.unwrap();
    test.state
        .db
        .accept_clan_request(1, target.id)
        .await
        .unwrap();

    owner.clan_id = Some(1);
    owner.clan_rank = crate::db::ClanRank::Leader as i32;

    let (tx, mut rx) = test.connect_player_with_outbox(&owner, 1);
    drain_events(&mut rx);

    handle_clan_promote(&test.state, &tx, PlayerId(owner.id), target.id).await;

    let db_target = test
        .state
        .db
        .get_player_by_id(target.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(db_target.clan_rank, crate::db::ClanRank::Officer as i32);
}

#[tokio::test]
async fn clan_accept_offline_player_succeeds_in_db() {
    let test = make_clan_test_state("accept_offline").await;
    let mut owner = test.create_player("clan-owner").await;
    let target = test.create_player("clan-member").await;

    test.state
        .db
        .create_clan(1, "Accept Clan", "ACC", owner.id)
        .await
        .unwrap();
    test.state.db.add_clan_request(1, target.id).await.unwrap();

    owner.clan_id = Some(1);
    owner.clan_rank = crate::db::ClanRank::Leader as i32;

    let (tx, mut rx) = test.connect_player_with_outbox(&owner, 1);
    drain_events(&mut rx);

    handle_clan_accept(&test.state, &tx, PlayerId(owner.id), target.id).await;

    let db_target = test
        .state
        .db
        .get_player_by_id(target.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(db_target.clan_id, Some(1));
}

#[tokio::test]
async fn clan_create_with_toctou_prevention_does_not_go_negative() {
    let test = make_clan_test_state("create_toctou").await;
    let (tx, mut rx) = test.connect_with_outbox(1);
    drain_events(&mut rx);

    let pid = PlayerId(test.player.id);
    // Задаем игроку ровно 999 кредитов
    test.state.modify_player(pid, |ecs, entity| {
        let mut s = ecs.get_mut::<PlayerStats>(entity)?;
        s.creds = 999;
        Some(())
    });

    handle_clan_create(&test.state, &tx, pid, "ToctouClan", "TTC").await;

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "OK");
    let msg = std::str::from_utf8(&events[0].1).unwrap();
    assert!(msg.contains("Недостаточно кредитов"));

    let current_creds = test
        .state
        .query_player(pid, |ecs, entity| {
            ecs.get::<PlayerStats>(entity).map(|s| s.creds)
        })
        .unwrap();
    assert_eq!(current_creds, Some(999)); // Баланс не изменился и не ушел в минус
}
