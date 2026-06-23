pub mod boxes;
pub mod buildings;
pub mod chats;
pub mod clans;
pub mod orders;
pub mod players;
pub mod programs;
pub mod provider;

pub use boxes::*;
pub use buildings::*;
pub use chats::ChatRow;
pub use clans::*;
pub use players::*;
pub use programs::ProgramRow;

use anyhow::Result;
use sqlx::SqlitePool;
use std::path::Path;

pub struct Database {
    pub(crate) pool: SqlitePool,
}

#[allow(dead_code)]
impl Database {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;

        let path_str = path
            .as_ref()
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid DB path"))?;
        let connection_str = format!("sqlite://{path_str}");
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }

        let options = SqliteConnectOptions::from_str(&connection_str)?
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;

        // Run migrations
        sqlx::migrate!("./migrations").run(&pool).await?;

        let db = Self { pool };
        db.migrate().await?;
        Ok(db)
    }

    async fn table_exists(&self, table_name: &str) -> bool {
        use sqlx::Row;
        let row = sqlx::query("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1")
            .bind(table_name)
            .fetch_one(&self.pool)
            .await;
        row.is_ok_and(|r| {
            let count: i32 = r.get(0);
            count > 0
        })
    }

    async fn column_exists(&self, table_name: &str, col_name: &str) -> bool {
        use sqlx::Row;
        let query_str =
            format!("SELECT COUNT(*) FROM pragma_table_info('{table_name}') WHERE name=?1");
        let row = sqlx::query(&query_str)
            .bind(col_name)
            .fetch_one(&self.pool)
            .await;
        row.is_ok_and(|r| {
            let count: i32 = r.get(0);
            count > 0
        })
    }

    #[allow(clippy::too_many_lines)]
    async fn migrate(&self) -> Result<()> {
        let mut did_migrate = false;

        let has_programs_table = self.table_exists("programs").await;
        if !has_programs_table {
            sqlx::query(
                "CREATE TABLE programs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    player_id INTEGER NOT NULL,
                    name TEXT NOT NULL,
                    code TEXT NOT NULL,
                    FOREIGN KEY(player_id) REFERENCES players(id)
                )",
            )
            .execute(&self.pool)
            .await?;
            did_migrate = true;
            tracing::info!("Migrated DB: added programs table");
        }

        let has_boxes_table = self.table_exists("boxes").await;
        if !has_boxes_table {
            sqlx::query(
                "CREATE TABLE boxes (
                    x INTEGER NOT NULL,
                    y INTEGER NOT NULL,
                    ze INTEGER NOT NULL DEFAULT 0,
                    cr INTEGER NOT NULL DEFAULT 0,
                    si INTEGER NOT NULL DEFAULT 0,
                    be INTEGER NOT NULL DEFAULT 0,
                    fi INTEGER NOT NULL DEFAULT 0,
                    go INTEGER NOT NULL DEFAULT 0,
                    cry_green INTEGER NOT NULL DEFAULT 0,
                    cry_blue INTEGER NOT NULL DEFAULT 0,
                    cry_red INTEGER NOT NULL DEFAULT 0,
                    cry_violet INTEGER NOT NULL DEFAULT 0,
                    cry_white INTEGER NOT NULL DEFAULT 0,
                    cry_cyan INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY (x, y)
                )",
            )
            .execute(&self.pool)
            .await?;
            did_migrate = true;
            tracing::info!("Migrated DB: added boxes table");
        }

        // Reference-compatible `boxes` crystal columns: ze/cr/si/be/fi/go
        {
            let crystal_cols = ["ze", "cr", "si", "be", "fi", "go"];
            for name in crystal_cols {
                if !self.column_exists("boxes", name).await {
                    let alter_query =
                        format!("ALTER TABLE boxes ADD COLUMN {name} INTEGER NOT NULL DEFAULT 0");
                    sqlx::query(&alter_query).execute(&self.pool).await?;
                    did_migrate = true;
                    tracing::info!("Migrated DB: added boxes.{name}");
                }
            }

            let has_legacy = self.column_exists("boxes", "cry_green").await;
            if has_legacy {
                let updated = sqlx::query(
                    "UPDATE boxes SET
                        ze = CASE WHEN ze = 0 THEN cry_green ELSE ze END,
                        cr = CASE WHEN cr = 0 THEN cry_blue ELSE cr END,
                        si = CASE WHEN si = 0 THEN cry_red ELSE si END,
                        be = CASE WHEN be = 0 THEN cry_violet ELSE be END,
                        fi = CASE WHEN fi = 0 THEN cry_white ELSE fi END,
                        go = CASE WHEN go = 0 THEN cry_cyan ELSE go END
                    WHERE cry_green != 0 OR cry_blue != 0 OR cry_red != 0 OR cry_violet != 0 OR cry_white != 0 OR cry_cyan != 0"
                )
                .execute(&self.pool)
                .await?;
                if updated.rows_affected() > 0 {
                    did_migrate = true;
                    tracing::info!(
                        "Migrated DB: backfilled boxes ze/cr/si/be/fi/go from legacy cry_* for {} rows",
                        updated.rows_affected()
                    );
                }
            }
        }

        // Reference-compatible `lines` table for chat history.
        {
            let has_lines_table = self.table_exists("lines").await;
            if !has_lines_table {
                sqlx::query(
                    "CREATE TABLE lines (
                        id INTEGER PRIMARY KEY AUTOINCREMENT,
                        playerid INTEGER NOT NULL DEFAULT 0,
                        message TEXT NOT NULL DEFAULT '',
                        chat_tag TEXT NOT NULL DEFAULT 'FED'
                    )",
                )
                .execute(&self.pool)
                .await?;
                did_migrate = true;
                tracing::info!("Migrated DB: added lines table");
            }
        }

        if !self.column_exists("players", "inventory").await {
            sqlx::query("ALTER TABLE players ADD COLUMN inventory TEXT NOT NULL DEFAULT '{}'")
                .execute(&self.pool)
                .await?;
            did_migrate = true;
            tracing::info!("Migrated DB: added inventory column");
        }
        if !self.column_exists("players", "skills").await {
            sqlx::query("ALTER TABLE players ADD COLUMN skills TEXT NOT NULL DEFAULT '{}'")
                .execute(&self.pool)
                .await?;
            did_migrate = true;
            tracing::info!("Migrated DB: added skills column");
        }

        {
            let has_role = self.column_exists("players", "role").await;
            let has_staff_role = self.column_exists("players", "staff_role").await;
            let has_is_admin = self.column_exists("players", "is_admin").await;
            let has_is_moderator = self.column_exists("players", "is_moderator").await;

            if has_staff_role && !has_role {
                sqlx::query("ALTER TABLE players RENAME COLUMN staff_role TO role")
                    .execute(&self.pool)
                    .await?;
                did_migrate = true;
                tracing::info!("Migrated DB: renamed staff_role -> role");
            }

            let has_role = self.column_exists("players", "role").await;
            if !has_role {
                sqlx::query("ALTER TABLE players ADD COLUMN role INTEGER NOT NULL DEFAULT 0")
                    .execute(&self.pool)
                    .await?;
                did_migrate = true;
                tracing::info!("Migrated DB: added role column");
            }

            let has_clan_rank = self.column_exists("players", "clan_rank").await;
            if !has_clan_rank {
                sqlx::query("ALTER TABLE players ADD COLUMN clan_rank INTEGER NOT NULL DEFAULT 0")
                    .execute(&self.pool)
                    .await?;
                did_migrate = true;
                tracing::info!("Migrated DB: added clan_rank column");
            }

            if has_is_admin || has_is_moderator {
                sqlx::query("UPDATE players SET role = 2 WHERE COALESCE(is_admin, 0) != 0")
                    .execute(&self.pool)
                    .await?;
                sqlx::query("UPDATE players SET role = 1 WHERE COALESCE(is_moderator, 0) != 0 AND role != 2")
                    .execute(&self.pool)
                    .await?;
                if has_is_admin {
                    match sqlx::query("ALTER TABLE players DROP COLUMN is_admin")
                        .execute(&self.pool)
                        .await
                    {
                        Ok(_) => {
                            did_migrate = true;
                            tracing::info!("Migrated DB: dropped is_admin");
                        }
                        Err(e) => tracing::warn!("Migrated DB: could not DROP is_admin: {e}"),
                    }
                }
                if has_is_moderator {
                    match sqlx::query("ALTER TABLE players DROP COLUMN is_moderator")
                        .execute(&self.pool)
                        .await
                    {
                        Ok(_) => {
                            did_migrate = true;
                            tracing::info!("Migrated DB: dropped is_moderator");
                        }
                        Err(e) => tracing::warn!("Migrated DB: could not DROP is_moderator: {e}"),
                    }
                }
            }
        }

        if !self.column_exists("players", "chat_color").await {
            sqlx::query("ALTER TABLE players ADD COLUMN chat_color INTEGER NOT NULL DEFAULT 0")
                .execute(&self.pool)
                .await?;
            did_migrate = true;
            tracing::info!("Migrated DB: added chat_color column");
        }

        for (col, ddl) in [
            (
                "player_id",
                "ALTER TABLE chat_messages ADD COLUMN player_id INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "color",
                "ALTER TABLE chat_messages ADD COLUMN color INTEGER NOT NULL DEFAULT 10",
            ),
        ] {
            if !self.column_exists("chat_messages", col).await {
                sqlx::query(ddl).execute(&self.pool).await?;
                did_migrate = true;
                tracing::info!("Migrated DB: added chat_messages.{col} column");
            }
        }

        {
            let backfilled = sqlx::query(
                "UPDATE chat_messages SET player_id = (
                     SELECT p.id FROM players p WHERE p.name = chat_messages.player_name
                 )
                 WHERE player_id = 0
                   AND EXISTS (
                     SELECT 1 FROM players p WHERE p.name = chat_messages.player_name
                 )",
            )
            .execute(&self.pool)
            .await;
            match backfilled {
                Ok(res) if res.rows_affected() > 0 => {
                    did_migrate = true;
                    tracing::info!(
                        "Migrated DB: backfilled chat_messages.player_id for {} legacy rows",
                        res.rows_affected()
                    );
                }
                _ => {}
            }
        }

        {
            let updated = sqlx::query(
                "UPDATE players SET skills = json_set(skills, '$.M.level', 60) WHERE json_extract(skills, '$.M.level') < 60"
            )
            .execute(&self.pool)
            .await;
            match updated {
                Ok(res) if res.rows_affected() > 0 => {
                    tracing::info!(
                        "Migrated DB: bumped Movement skill to 60 for {} players",
                        res.rows_affected()
                    );
                }
                _ => {}
            }
        }

        if did_migrate {
            tracing::info!("DB migration complete");
        }
        Ok(())
    }
}
