pub mod boxes;
pub mod buildings;
pub mod chats;
pub mod clans;
pub mod players;
pub mod programs;
pub mod provider;

pub use boxes::*;
pub use buildings::*;
pub use clans::*;
pub use players::*;
pub use programs::ProgramRow;

use anyhow::Result;
use parking_lot::Mutex;
use rusqlite::Connection;
use std::path::Path;

pub struct Database {
    pub(crate) conn: Mutex<Connection>,
}

#[allow(clippy::significant_drop_tightening)]
#[allow(dead_code)]
impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path.as_ref())?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA cache_size=10000;",
        )?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.init_tables()?;
        db.migrate()?;
        Ok(db)
    }

    fn init_tables(&self) -> Result<()> {
        {
            let conn = self.conn.lock();
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS players (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                passwd TEXT NOT NULL DEFAULT '',
                hash TEXT NOT NULL,
                x INTEGER NOT NULL DEFAULT 10,
                y INTEGER NOT NULL DEFAULT 10,
                dir INTEGER NOT NULL DEFAULT 0,
                health INTEGER NOT NULL DEFAULT 100,
                max_health INTEGER NOT NULL DEFAULT 100,
                money INTEGER NOT NULL DEFAULT 1000,
                creds INTEGER NOT NULL DEFAULT 0,
                skin INTEGER NOT NULL DEFAULT 0,
                auto_dig INTEGER NOT NULL DEFAULT 0,
                cry_green INTEGER NOT NULL DEFAULT 0,
                cry_blue INTEGER NOT NULL DEFAULT 0,
                cry_red INTEGER NOT NULL DEFAULT 0,
                cry_violet INTEGER NOT NULL DEFAULT 0,
                cry_white INTEGER NOT NULL DEFAULT 0,
                cry_cyan INTEGER NOT NULL DEFAULT 0,
                clan_id INTEGER DEFAULT NULL,
                clan_rank INTEGER NOT NULL DEFAULT 0,
                resp_x INTEGER DEFAULT NULL,
                resp_y INTEGER DEFAULT NULL,
                inventory TEXT NOT NULL DEFAULT '{}',
                skills TEXT NOT NULL DEFAULT '{}',
                role INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS chats (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                tag TEXT NOT NULL UNIQUE,
                name TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS chat_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chat_tag TEXT NOT NULL,
                player_name TEXT NOT NULL,
                message TEXT NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );

            -- Reference-compatible chat lines (GLine) table.
            -- Kept alongside chat_messages for backward compatibility.
            CREATE TABLE IF NOT EXISTS lines (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                playerid INTEGER NOT NULL DEFAULT 0,
                message TEXT NOT NULL DEFAULT '',
                chat_tag TEXT NOT NULL DEFAULT 'FED'
            );

            CREATE TABLE IF NOT EXISTS buildings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                type_code TEXT NOT NULL,
                x INTEGER NOT NULL,
                y INTEGER NOT NULL,
                owner_id INTEGER NOT NULL DEFAULT 0,
                clan_id INTEGER NOT NULL DEFAULT 0,
                data TEXT DEFAULT NULL
            );

            INSERT OR IGNORE INTO chats (tag, name) VALUES ('FED', 'Федеральный чат');
            INSERT OR IGNORE INTO chats (tag, name) VALUES ('DNO', 'Дно');

            CREATE TABLE IF NOT EXISTS clans (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                abr TEXT NOT NULL,
                owner_id INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS clan_requests (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                clan_id INTEGER NOT NULL,
                player_id INTEGER NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS clan_invites (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                clan_id INTEGER NOT NULL,
                player_id INTEGER NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(clan_id, player_id)
            );

            CREATE TABLE IF NOT EXISTS programs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                player_id INTEGER NOT NULL,
                name TEXT NOT NULL,
                code TEXT NOT NULL,
                FOREIGN KEY(player_id) REFERENCES players(id)
            );

            -- Boxes (Cell 90): crystal storage like reference `boxes` table
            CREATE TABLE IF NOT EXISTS boxes (
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
            );",
            )?;
        }
        Ok(())
    }

    /// Add columns to existing DBs that were created before Phase 2
    #[allow(clippy::too_many_lines)]
    fn migrate(&self) -> Result<()> {
        let mut did_migrate = false;
        let conn = self.conn.lock();

        let has_programs_table = conn
            .prepare("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='programs'")?
            .query_row([], |r| r.get::<_, i32>(0))
            .map(|c| c > 0)
            .unwrap_or(false);
        drop(conn);

        if !has_programs_table {
            let conn = self.conn.lock();
            conn.execute(
                "CREATE TABLE programs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    player_id INTEGER NOT NULL,
                    name TEXT NOT NULL,
                    code TEXT NOT NULL,
                    FOREIGN KEY(player_id) REFERENCES players(id)
                )",
                [],
            )?;
            did_migrate = true;
            tracing::info!("Migrated DB: added programs table");
        }

        // boxes table (Cell 90 crystal storage)
        let conn = self.conn.lock();
        let has_boxes_table = conn
            .prepare("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='boxes'")?
            .query_row([], |r| r.get::<_, i32>(0))
            .map(|c| c > 0)
            .unwrap_or(false);
        drop(conn);
        if !has_boxes_table {
            let conn = self.conn.lock();
            conn.execute(
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
                [],
            )?;
            did_migrate = true;
            tracing::info!("Migrated DB: added boxes table");
        }

        // Reference-compatible `boxes` crystal columns: ze/cr/si/be/fi/go
        {
            let conn = self.conn.lock();
            let has_ze = conn
                .prepare("SELECT COUNT(*) FROM pragma_table_info('boxes') WHERE name='ze'")?
                .query_row([], |r| r.get::<_, i32>(0))
                .map(|c| c > 0)
                .unwrap_or(false);
            let has_cr = conn
                .prepare("SELECT COUNT(*) FROM pragma_table_info('boxes') WHERE name='cr'")?
                .query_row([], |r| r.get::<_, i32>(0))
                .map(|c| c > 0)
                .unwrap_or(false);
            let has_si = conn
                .prepare("SELECT COUNT(*) FROM pragma_table_info('boxes') WHERE name='si'")?
                .query_row([], |r| r.get::<_, i32>(0))
                .map(|c| c > 0)
                .unwrap_or(false);
            let has_be = conn
                .prepare("SELECT COUNT(*) FROM pragma_table_info('boxes') WHERE name='be'")?
                .query_row([], |r| r.get::<_, i32>(0))
                .map(|c| c > 0)
                .unwrap_or(false);
            let has_fi = conn
                .prepare("SELECT COUNT(*) FROM pragma_table_info('boxes') WHERE name='fi'")?
                .query_row([], |r| r.get::<_, i32>(0))
                .map(|c| c > 0)
                .unwrap_or(false);
            let has_go = conn
                .prepare("SELECT COUNT(*) FROM pragma_table_info('boxes') WHERE name='go'")?
                .query_row([], |r| r.get::<_, i32>(0))
                .map(|c| c > 0)
                .unwrap_or(false);
            drop(conn);

            if !has_ze {
                self.conn.lock().execute(
                    "ALTER TABLE boxes ADD COLUMN ze INTEGER NOT NULL DEFAULT 0",
                    [],
                )?;
                did_migrate = true;
                tracing::info!("Migrated DB: added boxes.ze");
            }
            if !has_cr {
                self.conn.lock().execute(
                    "ALTER TABLE boxes ADD COLUMN cr INTEGER NOT NULL DEFAULT 0",
                    [],
                )?;
                did_migrate = true;
                tracing::info!("Migrated DB: added boxes.cr");
            }
            if !has_si {
                self.conn.lock().execute(
                    "ALTER TABLE boxes ADD COLUMN si INTEGER NOT NULL DEFAULT 0",
                    [],
                )?;
                did_migrate = true;
                tracing::info!("Migrated DB: added boxes.si");
            }
            if !has_be {
                self.conn.lock().execute(
                    "ALTER TABLE boxes ADD COLUMN be INTEGER NOT NULL DEFAULT 0",
                    [],
                )?;
                did_migrate = true;
                tracing::info!("Migrated DB: added boxes.be");
            }
            if !has_fi {
                self.conn.lock().execute(
                    "ALTER TABLE boxes ADD COLUMN fi INTEGER NOT NULL DEFAULT 0",
                    [],
                )?;
                did_migrate = true;
                tracing::info!("Migrated DB: added boxes.fi");
            }
            if !has_go {
                self.conn.lock().execute(
                    "ALTER TABLE boxes ADD COLUMN go INTEGER NOT NULL DEFAULT 0",
                    [],
                )?;
                did_migrate = true;
                tracing::info!("Migrated DB: added boxes.go");
            }

            // Best-effort backfill from legacy cry_* columns if present.
            let conn = self.conn.lock();
            let has_legacy = conn
                .prepare("SELECT COUNT(*) FROM pragma_table_info('boxes') WHERE name='cry_green'")?
                .query_row([], |r| r.get::<_, i32>(0))
                .map(|c| c > 0)
                .unwrap_or(false);
            drop(conn);
            if has_legacy {
                let conn = self.conn.lock();
                let updated = conn.execute(
                    "UPDATE boxes SET
                        ze = CASE WHEN ze = 0 THEN cry_green ELSE ze END,
                        cr = CASE WHEN cr = 0 THEN cry_blue ELSE cr END,
                        si = CASE WHEN si = 0 THEN cry_red ELSE si END,
                        be = CASE WHEN be = 0 THEN cry_violet ELSE be END,
                        fi = CASE WHEN fi = 0 THEN cry_white ELSE fi END,
                        go = CASE WHEN go = 0 THEN cry_cyan ELSE go END
                    WHERE cry_green != 0 OR cry_blue != 0 OR cry_red != 0 OR cry_violet != 0 OR cry_white != 0 OR cry_cyan != 0",
                    [],
                )?;
                if updated > 0 {
                    did_migrate = true;
                    tracing::info!(
                        "Migrated DB: backfilled boxes ze/cr/si/be/fi/go from legacy cry_* for {updated} rows"
                    );
                }
            }
        }

        // Reference-compatible `lines` table for chat history.
        {
            let conn = self.conn.lock();
            let has_lines_table = conn
                .prepare("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='lines'")?
                .query_row([], |r| r.get::<_, i32>(0))
                .map(|c| c > 0)
                .unwrap_or(false);
            drop(conn);
            if !has_lines_table {
                let conn = self.conn.lock();
                conn.execute(
                    "CREATE TABLE lines (
                        id INTEGER PRIMARY KEY AUTOINCREMENT,
                        playerid INTEGER NOT NULL DEFAULT 0,
                        message TEXT NOT NULL DEFAULT '',
                        chat_tag TEXT NOT NULL DEFAULT 'FED'
                    )",
                    [],
                )?;
                did_migrate = true;
                tracing::info!("Migrated DB: added lines table");
            }
        }

        let conn = self.conn.lock();
        let has_inventory: bool = conn
            .prepare("SELECT COUNT(*) FROM pragma_table_info('players') WHERE name='inventory'")?
            .query_row([], |r| r.get::<_, i32>(0))
            .map(|c| c > 0)
            .unwrap_or(false);
        let has_skills: bool = conn
            .prepare("SELECT COUNT(*) FROM pragma_table_info('players') WHERE name='skills'")?
            .query_row([], |r: &rusqlite::Row| r.get::<_, i32>(0))
            .map(|c| c > 0)
            .unwrap_or(false);
        drop(conn);

        if !has_inventory {
            let conn = self.conn.lock();
            conn.execute(
                "ALTER TABLE players ADD COLUMN inventory TEXT NOT NULL DEFAULT '{}'",
                [],
            )?;
            did_migrate = true;
            tracing::info!("Migrated DB: added inventory column");
        }
        if !has_skills {
            let conn = self.conn.lock();
            conn.execute(
                "ALTER TABLE players ADD COLUMN skills TEXT NOT NULL DEFAULT '{}'",
                [],
            )?;
            did_migrate = true;
            tracing::info!("Migrated DB: added skills column");
        }

        {
            let conn = self.conn.lock();
            let has_role = conn
                .prepare("SELECT COUNT(*) FROM pragma_table_info('players') WHERE name='role'")?
                .query_row([], |r| r.get::<_, i32>(0))
                .map(|c| c > 0)
                .unwrap_or(false);
            let has_staff_role = conn
                .prepare(
                    "SELECT COUNT(*) FROM pragma_table_info('players') WHERE name='staff_role'",
                )?
                .query_row([], |r| r.get::<_, i32>(0))
                .map(|c| c > 0)
                .unwrap_or(false);
            let has_is_admin = conn
                .prepare("SELECT COUNT(*) FROM pragma_table_info('players') WHERE name='is_admin'")?
                .query_row([], |r| r.get::<_, i32>(0))
                .map(|c| c > 0)
                .unwrap_or(false);
            let has_is_moderator = conn
                .prepare(
                    "SELECT COUNT(*) FROM pragma_table_info('players') WHERE name='is_moderator'",
                )?
                .query_row([], |r| r.get::<_, i32>(0))
                .map(|c| c > 0)
                .unwrap_or(false);
            drop(conn);

            if has_staff_role && !has_role {
                let conn = self.conn.lock();
                conn.execute("ALTER TABLE players RENAME COLUMN staff_role TO role", [])?;
                did_migrate = true;
                tracing::info!("Migrated DB: renamed staff_role -> role");
            }

            let has_role = {
                let conn = self.conn.lock();
                conn.prepare("SELECT COUNT(*) FROM pragma_table_info('players') WHERE name='role'")?
                    .query_row([], |r| r.get::<_, i32>(0))
                    .map(|c| c > 0)
                    .unwrap_or(false)
            };
            if !has_role {
                let conn = self.conn.lock();
                conn.execute(
                    "ALTER TABLE players ADD COLUMN role INTEGER NOT NULL DEFAULT 0",
                    [],
                )?;
                did_migrate = true;
                tracing::info!("Migrated DB: added role column");
            }

            let has_clan_rank = {
                let conn = self.conn.lock();
                conn.prepare(
                    "SELECT COUNT(*) FROM pragma_table_info('players') WHERE name='clan_rank'",
                )?
                .query_row([], |r| r.get::<_, i32>(0))
                .map(|c| c > 0)
                .unwrap_or(false)
            };
            if !has_clan_rank {
                let conn = self.conn.lock();
                conn.execute(
                    "ALTER TABLE players ADD COLUMN clan_rank INTEGER NOT NULL DEFAULT 0",
                    [],
                )?;
                did_migrate = true;
                tracing::info!("Migrated DB: added clan_rank column");
            }

            if has_is_admin || has_is_moderator {
                let conn = self.conn.lock();
                conn.execute(
                    "UPDATE players SET role = 2 WHERE COALESCE(is_admin, 0) != 0",
                    [],
                )?;
                conn.execute(
                    "UPDATE players SET role = 1 WHERE COALESCE(is_moderator, 0) != 0 AND role != 2",
                    [],
                )?;
                if has_is_admin {
                    match conn.execute("ALTER TABLE players DROP COLUMN is_admin", []) {
                        Ok(_) => {
                            did_migrate = true;
                            tracing::info!("Migrated DB: dropped is_admin");
                        }
                        Err(e) => tracing::warn!("Migrated DB: could not DROP is_admin: {e}"),
                    }
                }
                if has_is_moderator {
                    match conn.execute("ALTER TABLE players DROP COLUMN is_moderator", []) {
                        Ok(_) => {
                            did_migrate = true;
                            tracing::info!("Migrated DB: dropped is_moderator");
                        }
                        Err(e) => tracing::warn!("Migrated DB: could not DROP is_moderator: {e}"),
                    }
                }
            }
        }

        // Bump Movement skill to level 60 for all existing players with lower level
        {
            let conn = self.conn.lock();
            let updated = conn.execute(
                "UPDATE players SET skills = json_set(skills, '$.M.level', 60) WHERE json_extract(skills, '$.M.level') < 60",
                [],
            ).unwrap_or(0);
            if updated > 0 {
                tracing::info!("Migrated DB: bumped Movement skill to 60 for {updated} players");
            }
        }

        if did_migrate {
            tracing::info!("DB migration complete");
        }
        Ok(())
    }
}
