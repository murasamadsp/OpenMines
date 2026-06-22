//! Аукционные ордера (1:1 C# `Sys_Market/Order.cs` + `MarketSystem` CRUD).
//! Доменная логика (`Bet`/`CheckReady`) и GUI — в game/net слое; здесь только
//! персистентность. Методы пока вызываются доменом/тиком аукциона (следующий
//! слайс) — `#[allow(dead_code)]` как в `db/mod.rs` для стейдж-методов.
use super::Database;
use anyhow::Result;
use sqlx::Row;

#[derive(Debug, Clone)]
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

fn row_to_order(row: &sqlx::sqlite::SqliteRow) -> OrderRow {
    OrderRow {
        id: row.get("id"),
        initiator_id: row.get("initiator_id"),
        item_id: row.get("item_id"),
        num: row.get("num"),
        cost: row.get("cost"),
        buyer_id: row.get("buyer_id"),
        bet_time: row.get("bet_time"),
    }
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
        let row = sqlx::query(
            "SELECT id, initiator_id, item_id, num, cost, buyer_id, bet_time \
             FROM orders WHERE id = ?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_order))
    }

    /// Ордера по типу предмета, отсортированы по `cost` (как `MarketSystem.GetItems`).
    pub async fn list_orders_by_item(&self, item_id: i32) -> Result<Vec<OrderRow>> {
        let rows = sqlx::query(
            "SELECT id, initiator_id, item_id, num, cost, buyer_id, bet_time \
             FROM orders WHERE item_id = ?1 ORDER BY cost",
        )
        .bind(item_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(row_to_order).collect())
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
        sqlx::query("UPDATE orders SET cost = ?1, buyer_id = ?2, bet_time = ?3 WHERE id = ?4")
            .bind(cost)
            .bind(buyer_id)
            .bind(bet_time)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_order(&self, id: i32) -> Result<()> {
        sqlx::query("DELETE FROM orders WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Ордера, готовые к финализации (`Order.CheckReady`): была ставка
    /// (`buyer_id > 0`) и с момента последней прошло ≥ 5 мин (`bet_time <= cutoff`).
    pub async fn list_ready_orders(&self, cutoff_time: i64) -> Result<Vec<OrderRow>> {
        let rows = sqlx::query(
            "SELECT id, initiator_id, item_id, num, cost, buyer_id, bet_time \
             FROM orders WHERE buyer_id > 0 AND bet_time > 0 AND bet_time <= ?1",
        )
        .bind(cutoff_time)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(row_to_order).collect())
    }
}
