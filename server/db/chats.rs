use super::Database;
use anyhow::Result;
use rusqlite::params;

impl Database {
    pub fn add_chat_message(&self, tag: &str, name: &str, msg: &str) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO chat_messages (chat_tag, player_name, message) VALUES (?1, ?2, ?3)",
            params![tag, name, msg],
        )?;
        Ok(())
    }

    pub fn get_recent_chat_messages(
        &self,
        tag: &str,
        limit: usize,
    ) -> Result<Vec<(String, String, i64)>> {
        #[allow(clippy::cast_possible_wrap)]
        let mut result = self
            .conn
            .lock()
            .prepare(
                "SELECT player_name, message, created_at FROM chat_messages
             WHERE chat_tag = ?1
             ORDER BY id DESC LIMIT ?2",
            )?
            .query_map(params![tag, limit as i64], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;

        result.reverse();
        Ok(result)
    }
}
