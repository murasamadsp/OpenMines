//! Аукцион — финализация ордеров (1:1 C# `Order.CheckReady`, зовётся
//! `ServerTime` каждую 1с). Фоновый луп (как `lifecycle`): через 5 мин после
//! последней ставки лот закрывается — предмет покупателю, деньги инициатору.
//!
//! Начисление офлайн-игроку идёт прямо в БД; online — через ECS
//! (`modify_player`) с пакетами `P$`/`IN`. Луп редкий (5с) и обрабатывает
//! единицы ордеров, поэтому кратковременный `modify_player` из фонового таска
//! не создаёт контеншна (тот же приём, что в `player_dirty_flush_loop`).
//!
//! `Order.Bet` + GUI аукциона (item-грид/list/card/input) — отдельный слайс.
use crate::db::orders::OrderRow;
use crate::game::player::{PlayerId, PlayerInventory, PlayerStats};
use crate::game::{GameEvent, GameState, PlayerFlags};
use crate::net::presentation::PresentationSender;
use crate::net::session::outbound::inventory_sync::send_inventory;
use crate::net::session::wire::{PacketBatch, make_u_packet_bytes};
use crate::protocol::packets::money;
use anyhow::{Result, bail};
use std::sync::Arc;
use tokio::sync::broadcast;

/// C# `TimeSpan.FromMinutes(5)` — таймаут после последней ставки до финализации.
const BET_TIMEOUT_SECS: i64 = 300;

/// Интервал проверки. C# зовёт `CheckReady` каждые 1000мс; здесь 5с — при
/// 5-минутном таймауте разница в точности (≤5с) для игрока незаметна, а
/// БД-скан реже. Документированное отклонение.
const CHECK_INTERVAL_SECS: u64 = 5;

pub fn now_unix() -> i64 {
    crate::time::now_unix()
}

pub fn spawn_auction_finalize_loop(
    state: Arc<GameState>,
    presentation: PresentationSender,
    mut shutdown: broadcast::Receiver<()>,
) {
    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(std::time::Duration::from_secs(CHECK_INTERVAL_SECS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = shutdown.recv() => break,
            }
            finalize_ready_orders(&state, &presentation).await;
        }
    });
}

/// `Order.CheckReady` по всем готовым ордерам: удалить лот, отдать предмет
/// покупателю, деньги — инициатору (id 0 = NPC-ордер, без выплаты, как C#
/// `GetPlayer(0)` → null).
async fn finalize_ready_orders(state: &Arc<GameState>, presentation: &PresentationSender) {
    let cutoff = now_unix() - BET_TIMEOUT_SECS;
    let ready = match state.db.list_ready_orders(cutoff).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(error = ?e, "Failed to list ready orders");
            return;
        }
    };
    for o in ready {
        finalize_ready_order(state, presentation, &o).await;
    }
}

async fn finalize_ready_order(
    state: &Arc<GameState>,
    presentation: &PresentationSender,
    order: &OrderRow,
) {
    let buyer_id = PlayerId(order.buyer_id);
    let seller_id = PlayerId(order.initiator_id);
    let buyer_online = state.is_player_connected(buyer_id);
    let seller_online = order.initiator_id != 0 && state.is_player_connected(seller_id);
    if !buyer_online && !seller_online {
        finalize_offline_ready_order(state, order).await;
        return;
    }

    if order.initiator_id != 0
        && let Err(e) =
            credit_money_typed(state, presentation, order.initiator_id.into(), order.cost).await
    {
        tracing::error!(
            order_id = order.id,
            initiator_id = order.initiator_id,
            cost = order.cost,
            error = ?e,
            "Auction finalization stopped: initiator money credit failed"
        );
        return;
    }
    if let Err(e) = credit_inventory_typed(
        state,
        presentation,
        order.buyer_id.into(),
        order.item_id,
        order.num,
    )
    .await
    {
        tracing::error!(
            order_id = order.id,
            buyer_id = order.buyer_id,
            item_id = order.item_id,
            count = order.num,
            error = ?e,
            "Auction finalization stopped: buyer inventory credit failed"
        );
        if order.initiator_id != 0
            && let Err(rollback_err) =
                credit_money_typed(state, presentation, order.initiator_id.into(), -order.cost)
                    .await
        {
            tracing::error!(
                order_id = order.id,
                initiator_id = order.initiator_id,
                cost = order.cost,
                error = ?rollback_err,
                "Auction finalization rollback failed after buyer inventory credit failure"
            );
        }
        return;
    }
    match state.db.delete_order(order.id).await {
        Ok(true) => {}
        Ok(false) => {
            tracing::error!(
                order_id = order.id,
                "Finalized order delete affected 0 rows"
            );
            return;
        }
        Err(e) => {
            tracing::error!(order_id = order.id, error = ?e, "Failed to delete finalized order");
            return;
        }
    }
    tracing::info!(
        order_id = order.id,
        buyer_id = order.buyer_id,
        item_id = order.item_id,
        count = order.num,
        initiator_id = order.initiator_id,
        cost = order.cost,
        "Order finalized successfully"
    );
}

async fn finalize_offline_ready_order(state: &Arc<GameState>, order: &OrderRow) {
    match state.db.finalize_order_offline(order).await {
        Ok(true) => {
            tracing::info!(
                order_id = order.id,
                buyer_id = order.buyer_id,
                item_id = order.item_id,
                count = order.num,
                initiator_id = order.initiator_id,
                cost = order.cost,
                "Offline order finalized atomically"
            );
        }
        Ok(false) => {
            tracing::warn!(
                order_id = order.id,
                "Offline order finalization skipped: order changed or disappeared"
            );
        }
        Err(e) => {
            tracing::error!(
                order_id = order.id,
                buyer_id = order.buyer_id,
                initiator_id = order.initiator_id,
                error = ?e,
                "Offline order finalization transaction failed"
            );
        }
    }
}

/// Деньги игроку: online → ECS + `P$` (как C# `SendMoney`) + dirty; offline → БД.
pub async fn credit_money(state: &Arc<GameState>, pid: PlayerId, amount: i64) -> Result<()> {
    let packet = credit_money_impl(state, pid, amount).await?;
    if let Some((_, data)) = packet
        && let Some(tx) = state.player_sender(pid)
    {
        let _ = tx.send(data);
    }
    Ok(())
}

async fn credit_money_typed(
    state: &Arc<GameState>,
    presentation: &PresentationSender,
    pid: PlayerId,
    amount: i64,
) -> Result<()> {
    if let Some((session_id, data)) = credit_money_impl(state, pid, amount).await? {
        presentation.publish(GameEvent::SessionBatch {
            session_id,
            player_id: pid,
            packets: vec![data],
        });
    }
    Ok(())
}

async fn credit_money_impl(
    state: &Arc<GameState>,
    pid: PlayerId,
    amount: i64,
) -> Result<Option<(crate::game::SessionId, Vec<u8>)>> {
    let session_id = state.sessions.session_for_player(pid);
    let applied = state.modify_player(pid, |ecs, e| {
        if ecs.get::<PlayerStats>(e).is_none() {
            tracing::error!(player_id = %pid, component = "PlayerStats", "Player component missing for auction money credit");
            return None;
        }
        if ecs.get::<PlayerFlags>(e).is_none() {
            tracing::error!(player_id = %pid, component = "PlayerFlags", "Player component missing for auction money credit");
            return None;
        }
        let mut s = ecs.get_mut::<PlayerStats>(e)?;
        s.money += amount;
        let pair = (s.money, s.creds);
        let mut f = ecs.get_mut::<PlayerFlags>(e)?;
        f.dirty = true;
        Some(pair)
    });
    if let Some(online) = applied {
        let Some((m, c)) = online else {
            bail!("online player {pid}: player state missing for money credit");
        };
        let Some(session_id) = session_id else {
            return Ok(None);
        };
        return Ok(Some((
            session_id,
            make_u_packet_bytes("P$", &money(m, c).1),
        )));
    }
    state.db.add_player_money(pid.into(), amount).await?;
    Ok(None)
}

/// Предмет в инвентарь: online → ECS + `IN` (sync) + dirty; offline → БД.
#[cfg(test)]
async fn credit_inventory(
    state: &Arc<GameState>,
    pid: PlayerId,
    item_id: i32,
    count: i32,
) -> Result<()> {
    let packet = credit_inventory_impl(state, pid, item_id, count).await?;
    if let Some((_, packets)) = packet
        && let Some(tx) = state.player_sender(pid)
    {
        for packet in packets {
            let _ = tx.send(packet);
        }
    }
    Ok(())
}

async fn credit_inventory_typed(
    state: &Arc<GameState>,
    presentation: &PresentationSender,
    pid: PlayerId,
    item_id: i32,
    count: i32,
) -> Result<()> {
    if let Some((session_id, packets)) = credit_inventory_impl(state, pid, item_id, count).await? {
        presentation.publish(GameEvent::SessionBatch {
            session_id,
            player_id: pid,
            packets,
        });
    }
    Ok(())
}

async fn credit_inventory_impl(
    state: &Arc<GameState>,
    pid: PlayerId,
    item_id: i32,
    count: i32,
) -> Result<Option<(crate::game::SessionId, Vec<Vec<u8>>)>> {
    let session_id = state.sessions.session_for_player(pid);
    let applied = state.modify_player(pid, |ecs, e| {
        if ecs.get::<PlayerInventory>(e).is_none() {
            tracing::error!(player_id = %pid, component = "PlayerInventory", "Player component missing for auction inventory credit");
            return None;
        }
        if ecs.get::<PlayerFlags>(e).is_none() {
            tracing::error!(player_id = %pid, component = "PlayerFlags", "Player component missing for auction inventory credit");
            return None;
        }
        let mut inv = ecs.get_mut::<PlayerInventory>(e)?;
        *inv.items.entry(item_id).or_insert(0) += count;
        let batch = PacketBatch::default();
        send_inventory(&batch, &mut inv);
        let packets = batch.into_packets();
        let mut f = ecs.get_mut::<PlayerFlags>(e)?;
        f.dirty = true;
        Some(packets)
    });
    if let Some(online) = applied {
        let Some(packets) = online else {
            bail!("online player {pid}: player state missing for inventory credit");
        };
        let Some(session_id) = session_id else {
            return Ok(None);
        };
        return Ok(Some((session_id, packets)));
    }
    state
        .db
        .add_player_inventory_item(pid.into(), item_id, count)
        .await?;
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ServerTestHarness, drain_events};
    use std::sync::Arc;

    async fn make_credit_test_state(label: &str) -> ServerTestHarness {
        ServerTestHarness::new(&format!("auction_credit_{label}"), "auction-credit-user").await
    }

    fn player_money(state: &Arc<GameState>, pid: PlayerId) -> i64 {
        state
            .query_player_opt(pid, |ecs, entity| {
                let player_stats = ecs.get::<PlayerStats>(entity)?;
                Some(player_stats.money)
            })
            .unwrap()
    }

    fn item_count(state: &Arc<GameState>, pid: PlayerId, item: i32) -> i32 {
        state
            .query_player_opt(pid, |ecs, entity| {
                let inv = ecs.get::<PlayerInventory>(entity)?;
                Some(inv.items.get(&item).copied().unwrap_or(0))
            })
            .unwrap()
    }

    #[tokio::test]
    async fn credit_money_missing_flags_errors_without_money_mutation() {
        let test = make_credit_test_state("money_missing_flags").await;
        let (_tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let before_money = player_money(&test.state, pid);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity).remove::<PlayerFlags>();
        }

        let err = credit_money(&test.state, pid, 100).await.unwrap_err();

        assert!(err.to_string().contains("player state missing"));
        assert_eq!(player_money(&test.state, pid), before_money);
        assert!(drain_events(&mut rx).is_empty());
    }

    #[tokio::test]
    async fn credit_inventory_missing_flags_errors_without_item_mutation() {
        let test = make_credit_test_state("inventory_missing_flags").await;
        let (_tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let before_count = item_count(&test.state, pid, 5);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity).remove::<PlayerFlags>();
        }

        let err = credit_inventory(&test.state, pid, 5, 1).await.unwrap_err();

        assert!(err.to_string().contains("player state missing"));
        assert_eq!(item_count(&test.state, pid, 5), before_count);
        assert!(drain_events(&mut rx).is_empty());
    }

    #[tokio::test]
    async fn typed_money_credit_reaches_current_session_through_presentation() {
        let test = make_credit_test_state("typed_money_credit").await;
        let (_tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let presentation = crate::net::presentation::PresentationRuntime::start(test.state.clone());
        credit_money_typed(
            &test.state,
            &presentation.sender(),
            PlayerId(test.player.id),
            100,
        )
        .await
        .expect("typed auction money credit");

        let frame = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("presentation worker did not deliver auction money credit")
            .expect("auction outbox closed unexpectedly");
        let mut bytes = bytes::BytesMut::from(frame.as_slice());
        let packet = crate::protocol::Packet::try_decode(&mut bytes)
            .expect("valid auction money packet")
            .expect("complete auction money packet");
        assert_eq!(packet.event_name, *b"P$");
        assert_eq!(&packet.payload[..], br#"{"money":1100,"creds":0}"#);
        presentation.shutdown();
    }
}
