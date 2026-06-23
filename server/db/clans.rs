use super::Database;
use anyhow::{Result, bail};
use sqlx::Row;

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
    /// `id` клана = номер иконки (1..=218), как в C# `Clan.id == icon`.
    /// Клиент рендерит `ClanSprite.sprites[id-1]` (cS) и `sprites[clan-1]` в HB-боте.
    pub id: i32,
    pub name: String,
    pub abr: String,
    pub owner_id: i32,
}

impl Database {
    /// Выбрать свободный `id` клана из пула 1..=218 (= номер иконки, C# `Clan.id == icon`).
    /// `None`, если все 218 заняты — создание клана отклоняется (1:1 C# проверка
    /// `db.clans.FirstOrDefault(i => i.id == icon)`).
    pub async fn pick_clan_id(&self) -> Result<Option<i32>> {
        use rand::Rng;
        let used: Vec<i32> = sqlx::query("SELECT id FROM clans")
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(|r| r.get::<i32, _>("id"))
            .collect();
        if used.len() >= 218 {
            return Ok(None);
        }
        let mut rng = rand::rng();
        loop {
            let candidate: i32 = rng.random_range(1..=218);
            if !used.contains(&candidate) {
                return Ok(Some(candidate));
            }
        }
    }

    #[allow(clippy::significant_drop_tightening)]
    pub async fn create_clan(&self, id: i32, name: &str, abr: &str, owner_id: i32) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("INSERT INTO clans (id, name, abr, owner_id) VALUES (?1, ?2, ?3, ?4)")
            .bind(id)
            .bind(name)
            .bind(abr)
            .bind(owner_id)
            .execute(&mut *tx)
            .await?;

        sqlx::query("UPDATE players SET clan_id = ?1, clan_rank = ?2 WHERE id = ?3")
            .bind(id)
            .bind(ClanRank::Leader as i32)
            .bind(owner_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn get_clan(&self, id: i32) -> Result<Option<ClanRow>> {
        let row = sqlx::query("SELECT id, name, abr, owner_id FROM clans WHERE id = ?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        let clan = row.map(|r| ClanRow {
            id: r.get("id"),
            name: r.get("name"),
            abr: r.get("abr"),
            owner_id: r.get("owner_id"),
        });
        Ok(clan)
    }

    pub async fn get_clan_members(&self, clan_id: i32) -> Result<Vec<(i32, String, i32)>> {
        let rows = sqlx::query("SELECT id, name, clan_rank FROM players WHERE clan_id = ?1")
            .bind(clan_id)
            .fetch_all(&self.pool)
            .await?;
        let members = rows
            .into_iter()
            .map(|r| {
                (
                    r.get::<i32, _>("id"),
                    r.get::<String, _>("name"),
                    r.get::<i32, _>("clan_rank"),
                )
            })
            .collect();
        Ok(members)
    }

    pub async fn list_clans(&self) -> Result<Vec<ClanRow>> {
        let rows = sqlx::query("SELECT id, name, abr, owner_id FROM clans")
            .fetch_all(&self.pool)
            .await?;
        let clans = rows
            .into_iter()
            .map(|r| ClanRow {
                id: r.get("id"),
                name: r.get("name"),
                abr: r.get("abr"),
                owner_id: r.get("owner_id"),
            })
            .collect();
        Ok(clans)
    }

    #[allow(clippy::significant_drop_tightening)]
    pub async fn delete_clan(&self, id: i32) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("UPDATE players SET clan_id = NULL, clan_rank = 0 WHERE clan_id = ?1")
            .bind(id)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM clan_requests WHERE clan_id = ?1")
            .bind(id)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM clan_invites WHERE clan_id = ?1")
            .bind(id)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM clans WHERE id = ?1")
            .bind(id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn add_clan_request(&self, clan_id: i32, player_id: i32) -> Result<()> {
        sqlx::query("INSERT OR IGNORE INTO clan_requests (clan_id, player_id) VALUES (?1, ?2)")
            .bind(clan_id)
            .bind(player_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_clan_requests(&self, clan_id: i32) -> Result<Vec<(i32, String)>> {
        let rows = sqlx::query(
            "SELECT cr.player_id, p.name FROM clan_requests cr
             JOIN players p ON p.id = cr.player_id
             WHERE cr.clan_id = ?1",
        )
        .bind(clan_id)
        .fetch_all(&self.pool)
        .await?;
        let requests = rows
            .into_iter()
            .map(|r| (r.get::<i32, _>("player_id"), r.get::<String, _>("name")))
            .collect();
        Ok(requests)
    }

    #[allow(clippy::significant_drop_tightening)]
    pub async fn accept_clan_request(&self, clan_id: i32, player_id: i32) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        let result = sqlx::query(
            "UPDATE players SET clan_id = ?1, clan_rank = ?2
             WHERE id = ?3 AND EXISTS (
                 SELECT 1 FROM clan_requests WHERE clan_id = ?1 AND player_id = ?3
             )",
        )
        .bind(clan_id)
        .bind(ClanRank::Member as i32)
        .bind(player_id)
        .execute(&mut *tx)
        .await?;

        if result.rows_affected() == 0 {
            tx.rollback().await?;
            bail!("no pending request from player {player_id} for clan {clan_id}");
        }

        sqlx::query("DELETE FROM clan_requests WHERE clan_id = ?1 AND player_id = ?2")
            .bind(clan_id)
            .bind(player_id)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM clan_invites WHERE player_id = ?1")
            .bind(player_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn decline_clan_request(&self, clan_id: i32, player_id: i32) -> Result<()> {
        sqlx::query("DELETE FROM clan_requests WHERE clan_id = ?1 AND player_id = ?2")
            .bind(clan_id)
            .bind(player_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn add_clan_invite(&self, clan_id: i32, player_id: i32) -> Result<()> {
        sqlx::query("INSERT OR IGNORE INTO clan_invites (clan_id, player_id) VALUES (?1, ?2)")
            .bind(clan_id)
            .bind(player_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_player_invites(&self, player_id: i32) -> Result<Vec<(i32, String)>> {
        let rows = sqlx::query(
            "SELECT ci.clan_id, c.name FROM clan_invites ci
             JOIN clans c ON c.id = ci.clan_id
             WHERE ci.player_id = ?1",
        )
        .bind(player_id)
        .fetch_all(&self.pool)
        .await?;
        let invites = rows
            .into_iter()
            .map(|r| (r.get::<i32, _>("clan_id"), r.get::<String, _>("name")))
            .collect();
        Ok(invites)
    }

    #[allow(dead_code)]
    pub async fn accept_clan_invite(&self, clan_id: i32, player_id: i32) -> Result<()> {
        self.accept_clan_request(clan_id, player_id).await
    }

    pub async fn decline_clan_invite(&self, clan_id: i32, player_id: i32) -> Result<()> {
        sqlx::query("DELETE FROM clan_invites WHERE clan_id = ?1 AND player_id = ?2")
            .bind(clan_id)
            .bind(player_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_clan_rank(&self, player_id: i32, clan_id: i32, rank: ClanRank) -> Result<()> {
        sqlx::query("UPDATE players SET clan_rank = ?1 WHERE id = ?2 AND clan_id = ?3")
            .bind(rank as i32)
            .bind(player_id)
            .bind(clan_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn leave_clan(&self, player_id: i32) -> Result<()> {
        sqlx::query("UPDATE players SET clan_id = NULL, clan_rank = 0 WHERE id = ?1")
            .bind(player_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn kick_from_clan(&self, player_id: i32) -> Result<()> {
        self.leave_clan(player_id).await
    }

    pub async fn get_used_clan_ids(&self) -> Result<Vec<i32>> {
        let rows = sqlx::query("SELECT id FROM clans")
            .fetch_all(&self.pool)
            .await?;
        let ids = rows.into_iter().map(|r| r.get::<i32, _>("id")).collect();
        Ok(ids)
    }
}
