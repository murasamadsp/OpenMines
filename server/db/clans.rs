use super::Database;
use anyhow::Result;
use rusqlite::{OptionalExtension, params};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i32)]
#[allow(dead_code)]
pub enum ClanRank {
    None = 0,
    Member = 10,
    Officer = 50,
    Leader = 100,
}

impl ClanRank {
    #[must_use]
    pub const fn from_db(v: i32) -> Self {
        match v {
            100 => Self::Leader,
            50 => Self::Officer,
            10 => Self::Member,
            _ => Self::None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClanRow {
    pub id: i32,
    pub name: String,
    pub abr: String,
    pub owner_id: i32,
}

impl Database {
    pub fn create_clan(&self, id: i32, name: &str, abr: &str, owner_id: i32) -> Result<()> {
        {
            let conn = self.conn.lock();
            conn.execute(
                "INSERT INTO clans (id, name, abr, owner_id) VALUES (?1, ?2, ?3, ?4)",
                params![id, name, abr, owner_id],
            )?;
            conn.execute(
                "UPDATE players SET clan_id = ?1, clan_rank = ?2 WHERE id = ?3",
                params![id, ClanRank::Leader as i32, owner_id],
            )?;
        }
        Ok(())
    }

    pub fn get_clan(&self, id: i32) -> Result<Option<ClanRow>> {
        let row = self
            .conn
            .lock()
            .prepare_cached("SELECT id, name, abr, owner_id FROM clans WHERE id = ?1")?
            .query_row(params![id], |r| {
                Ok(ClanRow {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    abr: r.get(2)?,
                    owner_id: r.get(3)?,
                })
            })
            .optional()?;
        Ok(row)
    }

    pub fn get_clan_members(&self, clan_id: i32) -> Result<Vec<(i32, String, i32)>> {
        let rows = self
            .conn
            .lock()
            .prepare("SELECT id, name, clan_rank FROM players WHERE clan_id = ?1")?
            .query_map(params![clan_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        Ok(rows)
    }

    pub fn list_clans(&self) -> Result<Vec<ClanRow>> {
        let rows = self
            .conn
            .lock()
            .prepare("SELECT id, name, abr, owner_id FROM clans")?
            .query_map([], |r| {
                Ok(ClanRow {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    abr: r.get(2)?,
                    owner_id: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        Ok(rows)
    }

    pub fn delete_clan(&self, id: i32) -> Result<()> {
        {
            let conn = self.conn.lock();
            conn.execute(
                "UPDATE players SET clan_id = NULL, clan_rank = 0 WHERE clan_id = ?1",
                params![id],
            )?;
            conn.execute("DELETE FROM clan_requests WHERE clan_id = ?1", params![id])?;
            conn.execute("DELETE FROM clan_invites WHERE clan_id = ?1", params![id])?;
            conn.execute("DELETE FROM clans WHERE id = ?1", params![id])?;
        }
        Ok(())
    }

    pub fn add_clan_request(&self, clan_id: i32, player_id: i32) -> Result<()> {
        self.conn.lock().execute(
            "INSERT OR IGNORE INTO clan_requests (clan_id, player_id) VALUES (?1, ?2)",
            params![clan_id, player_id],
        )?;
        Ok(())
    }

    pub fn get_clan_requests(&self, clan_id: i32) -> Result<Vec<(i32, String)>> {
        let rows = self
            .conn
            .lock()
            .prepare(
                "SELECT cr.player_id, p.name FROM clan_requests cr
                 JOIN players p ON p.id = cr.player_id
                 WHERE cr.clan_id = ?1",
            )?
            .query_map(params![clan_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        Ok(rows)
    }

    pub fn accept_clan_request(&self, clan_id: i32, player_id: i32) -> Result<()> {
        {
            let conn = self.conn.lock();
            conn.execute(
                "UPDATE players SET clan_id = ?1, clan_rank = ?2 WHERE id = ?3",
                params![clan_id, ClanRank::Member as i32, player_id],
            )?;
            conn.execute(
                "DELETE FROM clan_requests WHERE clan_id = ?1 AND player_id = ?2",
                params![clan_id, player_id],
            )?;
            conn.execute(
                "DELETE FROM clan_invites WHERE player_id = ?1",
                params![player_id],
            )?;
        }
        Ok(())
    }

    pub fn decline_clan_request(&self, clan_id: i32, player_id: i32) -> Result<()> {
        self.conn.lock().execute(
            "DELETE FROM clan_requests WHERE clan_id = ?1 AND player_id = ?2",
            params![clan_id, player_id],
        )?;
        Ok(())
    }

    pub fn add_clan_invite(&self, clan_id: i32, player_id: i32) -> Result<()> {
        self.conn.lock().execute(
            "INSERT OR IGNORE INTO clan_invites (clan_id, player_id) VALUES (?1, ?2)",
            params![clan_id, player_id],
        )?;
        Ok(())
    }

    pub fn get_player_invites(&self, player_id: i32) -> Result<Vec<(i32, String)>> {
        let rows = self
            .conn
            .lock()
            .prepare(
                "SELECT ci.clan_id, c.name FROM clan_invites ci
                 JOIN clans c ON c.id = ci.clan_id
                 WHERE ci.player_id = ?1",
            )?
            .query_map(params![player_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        Ok(rows)
    }

    #[allow(dead_code)]
    pub fn accept_clan_invite(&self, clan_id: i32, player_id: i32) -> Result<()> {
        self.accept_clan_request(clan_id, player_id)
    }

    pub fn decline_clan_invite(&self, clan_id: i32, player_id: i32) -> Result<()> {
        self.conn.lock().execute(
            "DELETE FROM clan_invites WHERE clan_id = ?1 AND player_id = ?2",
            params![clan_id, player_id],
        )?;
        Ok(())
    }

    pub fn set_clan_rank(&self, player_id: i32, rank: ClanRank) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE players SET clan_rank = ?1 WHERE id = ?2",
            params![rank as i32, player_id],
        )?;
        Ok(())
    }

    pub fn leave_clan(&self, player_id: i32) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE players SET clan_id = NULL, clan_rank = 0 WHERE id = ?1",
            params![player_id],
        )?;
        Ok(())
    }

    pub fn kick_from_clan(&self, player_id: i32) -> Result<()> {
        self.leave_clan(player_id)
    }

    pub fn get_used_clan_ids(&self) -> Result<Vec<i32>> {
        let rows = self
            .conn
            .lock()
            .prepare("SELECT id FROM clans")?
            .query_map([], |r| r.get(0))?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        Ok(rows)
    }
}
