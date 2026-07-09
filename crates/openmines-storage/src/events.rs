use crate::Database;
use anyhow::Result;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct EventRow {
    pub id: String,
    pub title: String,
    pub starts_at: i64,
    pub ends_at: i64,
    pub config_json: String,
}

impl Database {
    /// Loads all active events from the database.
    pub async fn load_all_events(&self) -> Result<Vec<EventRow>> {
        let events = sqlx::query_as::<_, EventRow>(
            "SELECT id, title, starts_at, ends_at, config_json FROM active_events",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(events)
    }

    /// Saves or updates an active event.
    pub async fn save_event(&self, e: &EventRow) -> Result<()> {
        sqlx::query(
            "INSERT INTO active_events (id, title, starts_at, ends_at, config_json) \
             VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(id) DO UPDATE SET \
             title = excluded.title, \
             starts_at = excluded.starts_at, \
             ends_at = excluded.ends_at, \
             config_json = excluded.config_json",
        )
        .bind(&e.id)
        .bind(&e.title)
        .bind(e.starts_at)
        .bind(e.ends_at)
        .bind(&e.config_json)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Deletes an active event by ID.
    pub async fn delete_event(&self, id: &str) -> Result<bool> {
        let res = sqlx::query("DELETE FROM active_events WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }
}
