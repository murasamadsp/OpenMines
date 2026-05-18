use super::Database;
use anyhow::Result;
use rusqlite::params;

/// Строка истории чата:
/// `(id, player_name, message, created_at, player_id, color, clan_id)`.
/// Псевдоним вместо 7-кортежа в сигнатуре (читаемость; не подавление линта).
pub type ChatRow = (i64, String, String, i64, i32, i32, i32);

impl Database {
    /// Вставляет сообщение, разрешая цвет автора ОДИН раз и фиксируя его
    /// в строке (снимок на момент отправки). Возвращает
    /// `(rowid, color)` — `rowid` = `GCMessage.id` клиента (дедуп
    /// `LastIDs`), `color` нужен live-пути чтобы live/stored/история
    /// несли ОДИН цвет (реф-баг 1/10 — `docs/CLIENT_PROTOCOL_GAPS.md` §1).
    ///
    /// `player_id <= 0` (system/console) → `color = 50` (клиент
    /// `colorFromCode` спец-кейсит 50 = системный серый). Иначе — снимок
    /// `players.chat_color` (0..=19, цикл `Cset`/`mC`); orphan id → 10.
    pub fn add_chat_message(
        &self,
        tag: &str,
        name: &str,
        msg: &str,
        player_id: i32,
    ) -> Result<(i64, i32)> {
        let conn = self.conn.lock();
        let color = if player_id <= 0 {
            50
        } else {
            conn.query_row(
                "SELECT COALESCE(chat_color, 0) FROM players WHERE id = ?1",
                params![player_id],
                |r| r.get::<_, i32>(0),
            )
            .unwrap_or(10)
        };
        conn.execute(
            "INSERT INTO chat_messages (chat_tag, player_name, message, player_id, color)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![tag, name, msg, player_id, color],
        )?;
        Ok((conn.last_insert_rowid(), color))
    }

    /// Теги приватных каналов (`_a_b`) с участием `player_id`, по убыванию
    /// последней активности. Приват — НЕ в `server_reference` (нет модели
    /// ЛС вообще); см. `docs/CLIENT_PROTOCOL_GAPS.md` §6.
    pub fn private_chat_tags(&self, player_id: i32) -> Result<Vec<String>> {
        let a = format!("\\_{player_id}\\_%");
        let b = format!("\\_%\\_{player_id}");
        let tags = self
            .conn
            .lock()
            .prepare(
                "SELECT chat_tag, MAX(id) AS last FROM chat_messages
                 WHERE chat_tag LIKE ?1 ESCAPE '\\' OR chat_tag LIKE ?2 ESCAPE '\\'
                 GROUP BY chat_tag ORDER BY last DESC",
            )?
            .query_map(params![a, b], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        Ok(tags)
    }

    /// Циклически инкрементит `players.chat_color` `(c+1) % 20`, сохраняет,
    /// возвращает НОВОЕ значение. Изолировано от `save_player`/`row_to_player`
    /// (отдельная колонка, отдельный get/set — без переиндексации позиционного
    /// маппинга). Косметика (`mC`); нет в `server_reference` —
    /// `docs/CLIENT_PROTOCOL_GAPS.md` §5.
    pub fn cycle_chat_color(&self, player_id: i32) -> Result<i32> {
        let conn = self.conn.lock();
        let cur: i32 = conn
            .query_row(
                "SELECT COALESCE(chat_color, 0) FROM players WHERE id = ?1",
                params![player_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let next = (cur + 1).rem_euclid(20);
        conn.execute(
            "UPDATE players SET chat_color = ?1 WHERE id = ?2",
            params![next, player_id],
        )?;
        drop(conn);
        Ok(next)
    }

    /// `(id, player_name, message, created_at, player_id, color, clan_id)`
    /// в порядке возрастания `id` (старые → новые). `id` обязателен для
    /// wire `mU` и дедупа клиента. `clan_id` резолвится `LEFT JOIN players`
    /// (текущий клан автора = 1:1 C# `line.player.cid`, динамический;
    /// `0` = нет клана / orphan `player_id`). См.
    /// `docs/CLIENT_PROTOCOL_GAPS.md` §1.
    pub fn get_recent_chat_messages(&self, tag: &str, limit: usize) -> Result<Vec<ChatRow>> {
        #[allow(clippy::cast_possible_wrap)]
        let mut result = self
            .conn
            .lock()
            .prepare(
                "SELECT cm.id, cm.player_name, cm.message, cm.created_at,
                        cm.player_id, cm.color, COALESCE(p.clan_id, 0)
                 FROM chat_messages cm
                 LEFT JOIN players p ON p.id = cm.player_id
                 WHERE cm.chat_tag = ?1
                 ORDER BY cm.id DESC LIMIT ?2",
            )?
            .query_map(params![tag, limit as i64], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            })?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;

        result.reverse();
        Ok(result)
    }
}
