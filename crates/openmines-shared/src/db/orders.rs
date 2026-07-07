//! Аукционные ордера (1:1 C# `Sys_Market/Order.cs` + `MarketSystem` CRUD).
//! Доменная логика (`Bet`/`CheckReady`) и GUI — в game/net слое; здесь только
//! персистентность. Методы пока вызываются доменом/тиком аукциона (следующий
//! слайс) — `#[allow(dead_code)]` как в `db/mod.rs` для стейдж-методов.
use super::Database;
use anyhow::{Context as _, Result, bail};
use sqlx::Row;
use std::collections::HashMap;

#[derive(Debug, Clone, sqlx::FromRow)]
// Поля читаются доменом `Bet`/`CheckReady` и GUI аукциона (следующий слайс).
#[allow(dead_code)]
pub struct OrderRow {
    pub id: i32,
    pub initiator_id: i32,
    pub item_id: i32,
    pub num: i32,
    pub cost: i64,
    /// 0 = ставок ещё не было.
    pub buyer_id: i32,
    /// Unix-секунды последней ставки; 0 = ставок не было.
    pub bet_time: i64,
}

#[allow(dead_code)]
impl Database {
    /// Создать ордер (`MarketSystem.CreateOrder`). Возвращает id.
    pub async fn create_order(
        &self,
        initiator_id: i32,
        item_id: i32,
        num: i32,
        cost: i64,
    ) -> Result<i32> {
        let row = sqlx::query(
            "INSERT INTO orders (initiator_id, item_id, num, cost) VALUES (?1, ?2, ?3, ?4) \
             RETURNING id",
        )
        .bind(initiator_id)
        .bind(item_id)
        .bind(num)
        .bind(cost)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get("id"))
    }

    pub async fn get_order(&self, id: i32) -> Result<Option<OrderRow>> {
        let order = sqlx::query_as::<_, OrderRow>(
            "SELECT id, initiator_id, item_id, num, cost, buyer_id, bet_time \
             FROM orders WHERE id = ?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(order)
    }

    /// Ордера по типу предмета, отсортированы по `cost` (как `MarketSystem.GetItems`).
    pub async fn list_orders_by_item(&self, item_id: i32) -> Result<Vec<OrderRow>> {
        let orders = sqlx::query_as::<_, OrderRow>(
            "SELECT id, initiator_id, item_id, num, cost, buyer_id, bet_time \
             FROM orders WHERE item_id = ?1 ORDER BY cost",
        )
        .bind(item_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(orders)
    }

    /// Сводка для item-грида (`MarketSystem.Items`): `(item_id, count, min_cost)`.
    pub async fn order_counts_by_item(&self) -> Result<Vec<(i32, i64, i64)>> {
        let rows = sqlx::query(
            "SELECT item_id, COUNT(*) AS cnt, MIN(cost) AS min_cost \
             FROM orders GROUP BY item_id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| {
                (
                    r.get::<i32, _>("item_id"),
                    r.get::<i64, _>("cnt"),
                    r.get::<i64, _>("min_cost"),
                )
            })
            .collect())
    }

    /// Записать ставку (`Order.Bet`): новая цена/покупатель/время.
    pub async fn update_order_bet(
        &self,
        id: i32,
        cost: i64,
        buyer_id: i32,
        bet_time: i64,
    ) -> Result<()> {
        let result =
            sqlx::query("UPDATE orders SET cost = ?1, buyer_id = ?2, bet_time = ?3 WHERE id = ?4")
                .bind(cost)
                .bind(buyer_id)
                .bind(bet_time)
                .bind(id)
                .execute(&self.pool)
                .await?;
        if result.rows_affected() != 1 {
            bail!(
                "update order bet id={id} affected {} rows",
                result.rows_affected()
            );
        }
        Ok(())
    }

    /// CAS-вариант: обновляет ставку ТОЛЬКО если текущие `buyer_id` и `cost`
    /// совпадают с ожидаемыми. Возвращает количество затронутых строк (0 = гонка).
    /// Атомарен на уровне `SQLite` (serialized writes) — предотвращает двойной рефанд
    /// при одновременных ставках двух игроков на один ордер.
    pub async fn try_update_order_bet_cas(
        &self,
        id: i32,
        new_cost: i64,
        new_buyer_id: i32,
        bet_time: i64,
        expected_buyer_id: i32,
        expected_cost: i64,
    ) -> Result<u64> {
        let res = sqlx::query(
            "UPDATE orders SET cost = ?1, buyer_id = ?2, bet_time = ?3 \
             WHERE id = ?4 AND buyer_id = ?5 AND cost = ?6",
        )
        .bind(new_cost)
        .bind(new_buyer_id)
        .bind(bet_time)
        .bind(id)
        .bind(expected_buyer_id)
        .bind(expected_cost)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }

    pub async fn delete_order(&self, id: i32) -> Result<bool> {
        let result = sqlx::query("DELETE FROM orders WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Атомарно финализировать ордер, когда обе стороны offline.
    ///
    /// В одной SQLite-транзакции:
    /// - проверяет, что ордер всё ещё равен переданному snapshot;
    /// - начисляет предмет покупателю;
    /// - начисляет деньги продавцу, если он не NPC (`initiator_id != 0`);
    /// - удаляет ордер.
    ///
    /// `Ok(false)` = ордер уже изменён/удалён другим путём; начисления не сделаны.
    pub async fn finalize_order_offline(&self, order: &OrderRow) -> Result<bool> {
        let mut tx = self.pool.begin().await?;

        let current = sqlx::query_as::<_, OrderRow>(
            "SELECT id, initiator_id, item_id, num, cost, buyer_id, bet_time \
             FROM orders WHERE id = ?1",
        )
        .bind(order.id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(current) = current else {
            tx.rollback().await?;
            return Ok(false);
        };
        if current.initiator_id != order.initiator_id
            || current.item_id != order.item_id
            || current.num != order.num
            || current.cost != order.cost
            || current.buyer_id != order.buyer_id
            || current.bet_time != order.bet_time
        {
            tx.rollback().await?;
            return Ok(false);
        }

        let buyer_row = sqlx::query("SELECT inventory FROM players WHERE id = ?1")
            .bind(order.buyer_id)
            .fetch_optional(&mut *tx)
            .await?;
        let Some(buyer_row) = buyer_row else {
            bail!(
                "buyer id={}: missing player for auction finalization",
                order.buyer_id
            );
        };
        let inv_str: String = buyer_row.try_get("inventory")?;
        let mut inventory: HashMap<i32, i32> =
            serde_json::from_str(&inv_str).with_context(|| {
                format!(
                    "buyer id={}: parse inventory JSON for auction finalization",
                    order.buyer_id
                )
            })?;
        *inventory.entry(order.item_id).or_insert(0) += order.num;
        let inventory_json = serde_json::to_string(&inventory)?;
        let buyer_update = sqlx::query("UPDATE players SET inventory = ?1 WHERE id = ?2")
            .bind(inventory_json)
            .bind(order.buyer_id)
            .execute(&mut *tx)
            .await?;
        if buyer_update.rows_affected() != 1 {
            bail!(
                "buyer id={}: inventory finalization affected {} rows",
                order.buyer_id,
                buyer_update.rows_affected()
            );
        }

        if order.initiator_id != 0 {
            let seller_update = sqlx::query("UPDATE players SET money = money + ?1 WHERE id = ?2")
                .bind(order.cost)
                .bind(order.initiator_id)
                .execute(&mut *tx)
                .await?;
            if seller_update.rows_affected() != 1 {
                bail!(
                    "seller id={}: money finalization affected {} rows",
                    order.initiator_id,
                    seller_update.rows_affected()
                );
            }
        }

        let delete = sqlx::query("DELETE FROM orders WHERE id = ?1")
            .bind(order.id)
            .execute(&mut *tx)
            .await?;
        if delete.rows_affected() != 1 {
            bail!(
                "order id={}: finalization delete affected {} rows",
                order.id,
                delete.rows_affected()
            );
        }

        tx.commit().await?;
        Ok(true)
    }

    /// Ордера, готовые к финализации (`Order.CheckReady`): была ставка
    /// (`buyer_id > 0`) и с момента последней прошло ≥ 5 мин (`bet_time <= cutoff`).
    pub async fn list_ready_orders(&self, cutoff_time: i64) -> Result<Vec<OrderRow>> {
        let orders = sqlx::query_as::<_, OrderRow>(
            "SELECT id, initiator_id, item_id, num, cost, buyer_id, bet_time \
             FROM orders WHERE buyer_id > 0 AND bet_time > 0 AND bet_time <= ?1",
        )
        .bind(cutoff_time)
        .fetch_all(&self.pool)
        .await?;
        Ok(orders)
    }

    /// Все ордера — для отмены+рефанда при полном регене мира.
    pub async fn all_orders(&self) -> Result<Vec<OrderRow>> {
        let orders = sqlx::query_as::<_, OrderRow>(
            "SELECT id, initiator_id, item_id, num, cost, buyer_id, bet_time FROM orders",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(orders)
    }

    /// Снести все ордера. Возвращает число удалённых строк.
    pub async fn delete_all_orders(&self) -> Result<u64> {
        let res = sqlx::query("DELETE FROM orders")
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::Database;

    async fn temp_database(name: &str) -> Database {
        let path = std::env::temp_dir().join(format!("{name}_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        Database::open(path).await.unwrap()
    }

    #[tokio::test]
    async fn finalize_order_offline_credits_players_and_deletes_order_atomically() {
        let database = temp_database("order_finalize_offline").await;
        let seller = database.create_player("seller", "p", "hs").await.unwrap();
        let buyer = database.create_player("buyer", "p", "hb").await.unwrap();
        let order_id = database.create_order(seller.id, 7, 3, 150).await.unwrap();
        database
            .update_order_bet(order_id, 200, buyer.id, 10)
            .await
            .unwrap();
        let order = database.get_order(order_id).await.unwrap().unwrap();

        let finalized = database.finalize_order_offline(&order).await.unwrap();

        assert!(finalized);
        assert!(database.get_order(order_id).await.unwrap().is_none());
        let seller_after = database.get_player_by_id(seller.id).await.unwrap().unwrap();
        let buyer_after = database.get_player_by_id(buyer.id).await.unwrap().unwrap();
        assert_eq!(seller_after.money, seller.money + 200);
        assert_eq!(buyer_after.inventory.get(&7), Some(&3));
    }

    #[tokio::test]
    async fn finalize_order_offline_stale_snapshot_does_not_credit_or_delete() {
        let database = temp_database("order_finalize_stale").await;
        let seller = database
            .create_player("seller-stale", "p", "hs")
            .await
            .unwrap();
        let buyer = database
            .create_player("buyer-stale", "p", "hb")
            .await
            .unwrap();
        let order_id = database.create_order(seller.id, 7, 3, 150).await.unwrap();
        database
            .update_order_bet(order_id, 200, buyer.id, 10)
            .await
            .unwrap();
        let stale = database.get_order(order_id).await.unwrap().unwrap();
        database
            .update_order_bet(order_id, 250, buyer.id, 11)
            .await
            .unwrap();

        let finalized = database.finalize_order_offline(&stale).await.unwrap();

        assert!(!finalized);
        assert!(database.get_order(order_id).await.unwrap().is_some());
        let seller_after = database.get_player_by_id(seller.id).await.unwrap().unwrap();
        let buyer_after = database.get_player_by_id(buyer.id).await.unwrap().unwrap();
        assert_eq!(seller_after.money, seller.money);
        assert!(!buyer_after.inventory.contains_key(&7));
    }

    #[tokio::test]
    async fn update_order_bet_rejects_missing_order() {
        let database = temp_database("order_update_missing").await;
        let err = database
            .update_order_bet(999, 200, 1, 10)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("update order bet id=999"));
    }
}
