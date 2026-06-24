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
use crate::game::player::{PlayerId, PlayerInventory, PlayerStats};
use crate::game::{GameState, PlayerFlags};
use crate::net::session::outbound::inventory_sync::send_inventory;
use crate::net::session::wire::send_u_packet;
use crate::protocol::packets::money;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

/// C# `TimeSpan.FromMinutes(5)` — таймаут после последней ставки до финализации.
const BET_TIMEOUT_SECS: i64 = 300;

/// Интервал проверки. C# зовёт `CheckReady` каждые 1000мс; здесь 5с — при
/// 5-минутном таймауте разница в точности (≤5с) для игрока незаметна, а
/// БД-скан реже. Документированное отклонение.
const CHECK_INTERVAL_SECS: u64 = 5;

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

pub fn spawn_auction_finalize_loop(state: Arc<GameState>, mut shutdown: broadcast::Receiver<()>) {
    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(std::time::Duration::from_secs(CHECK_INTERVAL_SECS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = shutdown.recv() => break,
            }
            finalize_ready_orders(&state).await;
        }
    });
}

/// `Order.CheckReady` по всем готовым ордерам: удалить лот, отдать предмет
/// покупателю, деньги — инициатору (id 0 = NPC-ордер, без выплаты, как C#
/// `GetPlayer(0)` → null).
async fn finalize_ready_orders(state: &Arc<GameState>) {
    let cutoff = now_unix() - BET_TIMEOUT_SECS;
    let ready = match state.db.list_ready_orders(cutoff).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(error = ?e, "Failed to list ready orders");
            return;
        }
    };
    for o in ready {
        credit_inventory(state, o.buyer_id, o.item_id, o.num).await;
        if o.initiator_id != 0 {
            credit_money(state, o.initiator_id, o.cost).await;
        }
        if let Err(e) = state.db.delete_order(o.id).await {
            tracing::error!(order_id = o.id, error = ?e, "Failed to delete finalized order");
            continue;
        }
        tracing::info!(
            order_id = o.id,
            buyer_id = o.buyer_id,
            item_id = o.item_id,
            count = o.num,
            initiator_id = o.initiator_id,
            cost = o.cost,
            "Order finalized successfully"
        );
    }
}

/// Деньги игроку: online → ECS + `P$` (как C# `SendMoney`) + dirty; offline → БД.
pub async fn credit_money(state: &Arc<GameState>, pid: PlayerId, amount: i64) {
    let tx = state.player_sessions.get(&pid).map(|t| t.clone());
    let applied = state.modify_player(pid, |ecs, e| {
        let pair = {
            let mut s = ecs.get_mut::<PlayerStats>(e)?;
            s.money += amount;
            (s.money, s.creds)
        };
        if let Some(mut f) = ecs.get_mut::<PlayerFlags>(e) {
            f.dirty = true;
        }
        Some(pair)
    });
    if let Some(online) = applied {
        if let (Some(tx), Some((m, c))) = (tx, online) {
            send_u_packet(&tx, "P$", &money(m, c).1);
        }
    } else if let Err(e) = state.db.add_player_money(pid, amount).await {
        tracing::error!(player_id = pid, amount, error = ?e, "Failed to add player money offline");
    }
}

/// Предмет в инвентарь: online → ECS + `IN` (sync) + dirty; offline → БД.
pub async fn credit_inventory(state: &Arc<GameState>, pid: PlayerId, item_id: i32, count: i32) {
    let tx = state.player_sessions.get(&pid).map(|t| t.clone());
    let applied = state.modify_player(pid, |ecs, e| {
        {
            let mut inv = ecs.get_mut::<PlayerInventory>(e)?;
            *inv.items.entry(item_id).or_insert(0) += count;
            if let Some(t) = &tx {
                send_inventory(t, &mut inv);
            }
        }
        if let Some(mut f) = ecs.get_mut::<PlayerFlags>(e) {
            f.dirty = true;
        }
        Some(())
    });
    if applied.is_none()
        && let Err(e) = state
            .db
            .add_player_inventory_item(pid, item_id, count)
            .await
    {
        tracing::error!(
            player_id = pid,
            item_id,
            count,
            error = ?e,
            "Failed to add player inventory item offline"
        );
    }
}
