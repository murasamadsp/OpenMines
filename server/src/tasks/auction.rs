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
use crate::game::{GameState, PlayerFlags};
use crate::net::session::outbound::inventory_sync::send_inventory;
use crate::net::session::wire::send_u_packet;
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
        finalize_ready_order(state, &o).await;
    }
}

async fn finalize_ready_order(state: &Arc<GameState>, order: &OrderRow) {
    let buyer_id = PlayerId(order.buyer_id);
    let seller_id = PlayerId(order.initiator_id);
    let buyer_online = state.is_player_connected(buyer_id);
    let seller_online = order.initiator_id != 0 && state.is_player_connected(seller_id);
    if !buyer_online && !seller_online {
        finalize_offline_ready_order(state, order).await;
        return;
    }

    if order.initiator_id != 0
        && let Err(e) = credit_money(state, order.initiator_id.into(), order.cost).await
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
    if let Err(e) = credit_inventory(state, order.buyer_id.into(), order.item_id, order.num).await {
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
                credit_money(state, order.initiator_id.into(), -order.cost).await
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
    let tx = state.player_sender(pid);
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
        if let Some(tx) = tx {
            send_u_packet(&tx, "P$", &money(m, c).1);
        }
        return Ok(());
    }
    state.db.add_player_money(pid.into(), amount).await
}

/// Предмет в инвентарь: online → ECS + `IN` (sync) + dirty; offline → БД.
pub async fn credit_inventory(
    state: &Arc<GameState>,
    pid: PlayerId,
    item_id: i32,
    count: i32,
) -> Result<()> {
    let tx = state.player_sender(pid);
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
        if let Some(t) = &tx {
            send_inventory(t, &mut inv);
        }
        let mut f = ecs.get_mut::<PlayerFlags>(e)?;
        f.dirty = true;
        Some(())
    });
    if let Some(online) = applied {
        if online.is_none() {
            bail!("online player {pid}: player state missing for inventory credit");
        }
        return Ok(());
    }
    state
        .db
        .add_player_inventory_item(pid.into(), item_id, count)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::mpsc::UnboundedReceiver;

    struct AuctionCreditTestState {
        state: Arc<GameState>,
        player: crate::db::PlayerRow,
        db_path: std::path::PathBuf,
        world_name: String,
        dir: std::path::PathBuf,
    }

    impl AuctionCreditTestState {
        fn cleanup(&self) {
            let _ = std::fs::remove_file(&self.db_path);
            let _ = std::fs::remove_file(self.db_path.with_extension("db-wal"));
            let _ = std::fs::remove_file(self.db_path.with_extension("db-shm"));
            let _ = std::fs::remove_file(self.dir.join(format!("{}_v2.map", self.world_name)));
            let _ =
                std::fs::remove_file(self.dir.join(format!("{}_durability.map", self.world_name)));
        }
    }

    async fn make_credit_test_state(label: &str) -> AuctionCreditTestState {
        let dir = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_path = dir.join(format!(
            "auction_credit_{label}_{}_{}.db",
            std::process::id(),
            nonce
        ));
        let _ = std::fs::remove_file(&db_path);

        let database = crate::db::Database::open(&db_path).await.unwrap();
        let player = database
            .create_player("auction-credit-user", "p", "h")
            .await
            .unwrap();

        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();
        let world_name = format!(
            "auction_credit_world_{label}_{}_{}",
            std::process::id(),
            nonce
        );
        let world = crate::world::World::new(&world_name, 2, 2, cell_defs, &dir).unwrap();
        let config = crate::config::Config {
            world_name: world_name.clone(),
            port: 8090,
            world_chunks_w: 2,
            world_chunks_h: 2,
            data_dir: dir.to_string_lossy().to_string(),
            logging: crate::config::LoggingConfig::runtime_baseline(),
            cron: crate::config::CronConfig::runtime_baseline(),
            gameplay: crate::config::GameplayConfig::runtime_baseline(),
        };
        let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config)
            .await
            .unwrap();

        AuctionCreditTestState {
            state,
            player,
            db_path,
            world_name,
            dir,
        }
    }

    fn drain_events(rx: &mut UnboundedReceiver<Vec<u8>>) -> Vec<(String, Vec<u8>)> {
        let mut events = Vec::new();
        while let Ok(frame) = rx.try_recv() {
            let mut buf = BytesMut::from(&frame[..]);
            let packet = crate::protocol::Packet::try_decode(&mut buf)
                .expect("valid packet")
                .expect("decoded packet");
            events.push((packet.event_str().to_owned(), packet.payload.to_vec()));
        }
        events
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
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
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

        test.cleanup();
    }

    #[tokio::test]
    async fn credit_inventory_missing_flags_errors_without_item_mutation() {
        let test = make_credit_test_state("inventory_missing_flags").await;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
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

        test.cleanup();
    }
}
