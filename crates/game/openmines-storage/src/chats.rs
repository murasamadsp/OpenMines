use super::Database;
use anyhow::{Context as _, Result, bail};
use sqlx::Row;

/// Строка истории чата:
/// `(id, player_name, message, created_at, player_id, color, clan_id)`.
pub type ChatRow = (i64, String, String, i64, i32, i32, i32);

impl Database {
    /// Вставляет сообщение, разрешая цвет автора ОДИН раз и фиксируя его
    /// в строке (снимок на момент отправки). Возвращает
    /// `(rowid, color)`
    pub async fn add_chat_message(
        &self,
        id: i64,
        tag: &str,
        name: &str,
        msg: &str,
        player_id: i32,
        color: i32,
    ) -> Result<()> {
        let _result = sqlx::query(
            "INSERT INTO chat_messages (id, chat_tag, player_name, message, player_id, color)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(id)
        .bind(tag)
        .bind(name)
        .bind(msg)
        .bind(player_id)
        .bind(color)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Fetches the maximum chat message ID currently in the database.
    pub async fn get_max_chat_id(&self) -> Result<i64> {
        let row = sqlx::query("SELECT MAX(id) as max_id FROM chat_messages")
            .fetch_one(&self.pool)
            .await?;
        let max_id: Option<i64> = row.try_get("max_id")?;
        Ok(max_id.unwrap_or(0))
    }

    /// Теги приватных каналов (`_a_b`) с участием `player_id`, по убыванию
    /// последней активности.
    pub async fn private_chat_tags(&self, player_id: i32) -> Result<Vec<String>> {
        let a = format!("\\_{player_id}\\_%");
        let b = format!("\\_%\\_{player_id}");
        let rows = sqlx::query(
            "SELECT chat_tag, MAX(id) AS last FROM chat_messages
             WHERE chat_tag LIKE ?1 ESCAPE '\\' OR chat_tag LIKE ?2 ESCAPE '\\'
             GROUP BY chat_tag ORDER BY last DESC",
        )
        .bind(a)
        .bind(b)
        .fetch_all(&self.pool)
        .await?;
        let tags = rows
            .into_iter()
            .map(|r| r.get::<String, _>("chat_tag"))
            .collect();
        Ok(tags)
    }

    /// Циклически инкрементит `players.chat_color` `(c+1) % 20`, сохраняет,
    /// возвращает НОВОЕ значение.
    pub async fn cycle_chat_color(&self, player_id: i32) -> Result<i32> {
        let cur = sqlx::query("SELECT chat_color FROM players WHERE id = ?1")
            .bind(player_id)
            .fetch_one(&self.pool)
            .await
            .with_context(|| format!("load chat_color for player id={player_id}"))?
            .try_get::<i32, _>("chat_color")?;
        let next = (cur + 1).rem_euclid(20);
        let result = sqlx::query("UPDATE players SET chat_color = ?1 WHERE id = ?2")
            .bind(next)
            .bind(player_id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() != 1 {
            bail!(
                "update chat_color for player id={player_id} affected {} rows",
                result.rows_affected()
            );
        }
        Ok(next)
    }

    /// Атомарно циклически меняет цвет чата. `None` означает, что игрока нет.
    pub async fn cycle_chat_color_if_present(&self, player_id: i32) -> Result<Option<i32>> {
        let row = sqlx::query(
            "UPDATE players
             SET chat_color = (chat_color + 1) % 20
             WHERE id = ?1
             RETURNING chat_color",
        )
        .bind(player_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| row.try_get("chat_color")).transpose()?)
    }

    /// `(id, player_name, message, created_at, player_id, color, clan_id)`
    /// в порядке возрастания `id` (старые → новые).
    pub async fn get_recent_chat_messages(&self, tag: &str, limit: usize) -> Result<Vec<ChatRow>> {
        #[allow(clippy::cast_possible_wrap)]
        let mut msgs = sqlx::query_as::<_, ChatRow>(
            "SELECT cm.id, cm.player_name, cm.message, cm.created_at,
                    cm.player_id, cm.color, COALESCE(p.clan_id, 0) as clan_id
             FROM chat_messages cm
             LEFT JOIN players p ON p.id = cm.player_id
             WHERE cm.chat_tag = ?1
             ORDER BY cm.id DESC LIMIT ?2",
        )
        .bind(tag)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        msgs.reverse();
        Ok(msgs)
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
    async fn add_chat_message_test() {
        let db = temp_database("empty_db").await;
        db.add_chat_message(10, "FED", "ghost", "msg", 12345, 50)
            .await
            .unwrap();

        let msgs = db.get_recent_chat_messages("FED", 10).await.unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].1, "ghost");
        assert_eq!(msgs[0].2, "msg");
        assert_eq!(msgs[0].4, 12345);
        assert_eq!(msgs[0].5, 50);

        let max_id = db.get_max_chat_id().await.unwrap();
        assert_eq!(max_id, 10);
    }

    #[tokio::test]
    async fn add_chat_message_system_test() {
        let db = temp_database("get_max_chat_id").await;
        db.add_chat_message(1, "FED", "system", "msg", 0, 10)
            .await
            .unwrap();
        let msgs = db.get_recent_chat_messages("FED", 10).await.unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].5, 10);
        let max_id = db.get_max_chat_id().await.unwrap();
        assert_eq!(max_id, 1);
    }

    #[tokio::test]
    async fn system_chat_message_succeeds_without_player_row() {
        let database = temp_database("chat_system_color").await;
        database
            .add_chat_message(1, "FED", "ghost", "msg", 12345, 50)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn cycle_chat_color_rejects_missing_player() {
        let database = temp_database("chat_cycle_missing_player").await;
        let err = database.cycle_chat_color(12345).await.unwrap_err();
        assert!(err.to_string().contains("load chat_color"));
    }
}
