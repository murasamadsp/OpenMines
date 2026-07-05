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
        tag: &str,
        name: &str,
        msg: &str,
        player_id: i32,
    ) -> Result<(i64, i32)> {
        let color = if player_id <= 0 {
            50
        } else {
            sqlx::query("SELECT chat_color FROM players WHERE id = ?1")
                .bind(player_id)
                .fetch_one(&self.pool)
                .await
                .with_context(|| format!("load chat_color for player id={player_id}"))?
                .try_get::<i32, _>("chat_color")?
        };

        let result = sqlx::query(
            "INSERT INTO chat_messages (chat_tag, player_name, message, player_id, color)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(tag)
        .bind(name)
        .bind(msg)
        .bind(player_id)
        .bind(color)
        .execute(&self.pool)
        .await?;

        Ok((result.last_insert_rowid(), color))
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

    /// `(id, player_name, message, created_at, player_id, color, clan_id)`
    /// в порядке возрастания `id` (старые → новые).
    pub async fn get_recent_chat_messages(&self, tag: &str, limit: usize) -> Result<Vec<ChatRow>> {
        #[allow(clippy::cast_possible_wrap)]
        let rows = sqlx::query(
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

        let mut result: Vec<ChatRow> = rows
            .into_iter()
            .map(|r| {
                (
                    r.get::<i64, _>("id"),
                    r.get::<String, _>("player_name"),
                    r.get::<String, _>("message"),
                    r.get::<i64, _>("created_at"),
                    r.get::<i32, _>("player_id"),
                    r.get::<i32, _>("color"),
                    r.get::<i32, _>("clan_id"),
                )
            })
            .collect();

        result.reverse();
        Ok(result)
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
    async fn add_chat_message_rejects_missing_player_color() {
        let database = temp_database("chat_missing_player_color").await;
        let err = database
            .add_chat_message("FED", "ghost", "msg", 12345)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("load chat_color"));
    }

    #[tokio::test]
    async fn cycle_chat_color_rejects_missing_player() {
        let database = temp_database("chat_cycle_missing_player").await;
        let err = database.cycle_chat_color(12345).await.unwrap_err();
        assert!(err.to_string().contains("load chat_color"));
    }

    #[tokio::test]
    async fn system_chat_message_keeps_system_color_without_player_row() {
        let database = temp_database("chat_system_color").await;
        let (_id, color) = database
            .add_chat_message("FED", "system", "msg", 0)
            .await
            .unwrap();
        assert_eq!(color, 50);
    }
}
