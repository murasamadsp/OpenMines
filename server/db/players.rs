use super::{Database, clans::ClanRank};
use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, params};
use std::collections::HashMap;

/// Роль игрока (`players.role`). Новые уровни — только новыми числами, старые не переиспользовать.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i32)]
pub enum Role {
    Player = 0,
    Moderator = 1,
    Admin = 2,
}

impl Role {
    #[must_use]
    pub const fn from_db(v: i32) -> Self {
        match v {
            2 => Self::Admin,
            1 => Self::Moderator,
            _ => Self::Player,
        }
    }

    #[must_use]
    pub const fn is_admin(self) -> bool {
        matches!(self, Self::Admin)
    }

    /// Модератор или админ (задел под отдельные команды).
    #[must_use]
    #[allow(dead_code)]
    pub const fn is_moderator_effective(self) -> bool {
        matches!(self, Self::Moderator | Self::Admin)
    }
}

/// Default skills for a new player — basic digging, movement, building, mining, health, repair
pub fn default_skills() -> HashMap<String, SkillState> {
    let mut s = HashMap::new();
    // Skill codes from C# SkillType enum comments
    s.insert("d".into(), SkillState { level: 1, exp: 0.0 }); // Digging
    s.insert(
        "M".into(),
        SkillState {
            level: 60,
            exp: 0.0,
        },
    ); // Movement
    s.insert("L".into(), SkillState { level: 1, exp: 0.0 }); // BuildGreen
    s.insert("A".into(), SkillState { level: 1, exp: 0.0 }); // BuildRoad
    s.insert("O".into(), SkillState { level: 1, exp: 0.0 }); // BuildStructure
    s.insert("m".into(), SkillState { level: 1, exp: 0.0 }); // MineGeneral
    s.insert("l".into(), SkillState { level: 1, exp: 0.0 }); // Health
    s.insert("e".into(), SkillState { level: 1, exp: 0.0 }); // Repair
    s
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillState {
    pub level: i32,
    pub exp: f32,
}

#[derive(Debug, Clone)]
pub struct PlayerRow {
    pub id: i32,
    pub name: String,
    pub passwd: String,
    pub hash: String,
    pub x: i32,
    pub y: i32,
    pub dir: i32,
    pub health: i32,
    pub max_health: i32,
    pub money: i64,
    pub creds: i64,
    pub skin: i32,
    pub auto_dig: bool,
    pub crystals: [i64; 6],
    pub clan_id: Option<i32>,
    pub resp_x: Option<i32>,
    pub resp_y: Option<i32>,
    pub inventory: HashMap<i32, i32>,
    pub skills: HashMap<String, SkillState>,
    /// Сырое значение `Role` из БД. Меняется только в БД или через `set_player_role`.
    pub role: i32,
    pub clan_rank: i32,
}

impl PlayerRow {
    #[inline]
    #[must_use]
    pub const fn as_role(&self) -> Role {
        Role::from_db(self.role)
    }

    #[inline]
    #[must_use]
    #[allow(dead_code)]
    pub const fn as_clan_rank(&self) -> ClanRank {
        ClanRank::from_db(self.clan_rank)
    }
}

fn row_to_player(r: &rusqlite::Row) -> PlayerRow {
    let inv_str: String = r.get(20).unwrap_or_default();
    let skills_str: String = r.get(21).unwrap_or_default();
    PlayerRow {
        id: r.get(0).unwrap(),
        name: r.get(1).unwrap(),
        passwd: r.get(2).unwrap(),
        hash: r.get(3).unwrap(),
        x: r.get(4).unwrap(),
        y: r.get(5).unwrap(),
        dir: r.get(6).unwrap(),
        health: r.get(7).unwrap(),
        max_health: r.get(8).unwrap(),
        money: r.get(9).unwrap(),
        creds: r.get(10).unwrap(),
        skin: r.get(11).unwrap(),
        auto_dig: r.get::<_, i32>(12).unwrap() != 0,
        crystals: [
            r.get(13).unwrap(),
            r.get(14).unwrap(),
            r.get(15).unwrap(),
            r.get(16).unwrap(),
            r.get(17).unwrap(),
            r.get(18).unwrap(),
        ],
        clan_id: r.get(19).unwrap(),
        inventory: serde_json::from_str(&inv_str).unwrap_or_default(),
        skills: serde_json::from_str(&skills_str).unwrap_or_else(|_| default_skills()),
        resp_x: r.get(22).unwrap_or(None),
        resp_y: r.get(23).unwrap_or(None),
        role: r.get::<_, i32>(24).unwrap_or(0),
        clan_rank: r.get::<_, i32>(25).unwrap_or(0),
    }
}

impl Database {
    pub fn create_player(&self, name: &str, passwd: &str, hash: &str) -> Result<PlayerRow> {
        let name = name.trim();
        if name.is_empty() {
            bail!("empty player name");
        }
        let skills_json = serde_json::to_string(&default_skills())?;
        let id = {
            let conn = self.conn.lock();
            conn.execute(
                "INSERT INTO players (name, passwd, hash, skills) VALUES (?1, ?2, ?3, ?4)",
                params![name, passwd, hash, skills_json],
            )?;
            i32::try_from(conn.last_insert_rowid())
                .map_err(|_| anyhow::anyhow!("player id overflow"))?
        };
        Ok(PlayerRow {
            id,
            name: name.to_owned(),
            passwd: passwd.to_string(),
            hash: hash.to_string(),
            x: 10,
            y: 10,
            dir: 0,
            health: 100,
            max_health: 100,
            money: 1000,
            creds: 0,
            skin: 0,
            auto_dig: false,
            crystals: [0; 6],
            clan_id: None,
            clan_rank: 0,
            resp_x: None,
            resp_y: None,
            inventory: HashMap::new(),
            skills: default_skills(),
            role: 0,
        })
    }

    /// Явная смена роли. Обычный `save_player` роль не трогает.
    pub fn set_player_role(&self, player_id: i32, role: Role) -> Result<bool> {
        let n = self.conn.lock().execute(
            "UPDATE players SET role = ?1 WHERE id = ?2",
            params![role as i32, player_id],
        )?;
        Ok(n > 0)
    }

    pub fn get_player_by_id(&self, id: i32) -> Result<Option<PlayerRow>> {
        let row = self.conn.lock().prepare_cached(
                "SELECT id, name, passwd, hash, x, y, dir, health, max_health, money, creds, skin, auto_dig,
                    cry_green, cry_blue, cry_red, cry_violet, cry_white, cry_cyan, clan_id,
                    inventory, skills, resp_x, resp_y, role, clan_rank
             FROM players WHERE id = ?1",
            )?
            .query_row(params![id], |r| Ok(row_to_player(r)))
            .optional()?;
        Ok(row)
    }

    pub fn get_player_by_name(&self, name: &str) -> Result<Option<PlayerRow>> {
        let name = name.trim();
        if name.is_empty() {
            return Ok(None);
        }
        let row = self.conn.lock().prepare_cached(
                "SELECT id, name, passwd, hash, x, y, dir, health, max_health, money, creds, skin, auto_dig,
                    cry_green, cry_blue, cry_red, cry_violet, cry_white, cry_cyan, clan_id,
                    inventory, skills, resp_x, resp_y, role, clan_rank
             FROM players WHERE lower(trim(name)) = lower(?1) ORDER BY id LIMIT 1",
            )?
            .query_row(params![name], |r| Ok(row_to_player(r)))
            .optional()?;
        Ok(row)
    }

    pub fn save_player(&self, p: &PlayerRow) -> Result<()> {
        let inv_json = serde_json::to_string(&p.inventory)?;
        let skills_json = serde_json::to_string(&p.skills)?;
        self.conn.lock().execute(
            "UPDATE players SET x=?1, y=?2, dir=?3, health=?4, max_health=?5, money=?6, creds=?7,
             skin=?8, auto_dig=?9, cry_green=?10, cry_blue=?11, cry_red=?12, cry_violet=?13,
             cry_white=?14, cry_cyan=?15, clan_id=?16, passwd=?17, inventory=?18, skills=?19,
             resp_x=?21, resp_y=?22, clan_rank=?23
             WHERE id=?20",
            params![
                p.x,
                p.y,
                p.dir,
                p.health,
                p.max_health,
                p.money,
                p.creds,
                p.skin,
                i32::from(p.auto_dig),
                p.crystals[0],
                p.crystals[1],
                p.crystals[2],
                p.crystals[3],
                p.crystals[4],
                p.crystals[5],
                p.clan_id,
                p.passwd,
                inv_json,
                skills_json,
                p.id,
                p.resp_x,
                p.resp_y,
                p.clan_rank
            ],
        )?;
        Ok(())
    }

    pub fn player_name_exists(&self, name: &str) -> Result<bool> {
        let name = name.trim();
        if name.is_empty() {
            return Ok(false);
        }
        let count: i32 = self.conn.lock().query_row(
            "SELECT COUNT(*) FROM players WHERE lower(trim(name)) = lower(?1)",
            params![name],
            |r| r.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn update_player_resp(
        &self,
        player_id: i32,
        resp_x: Option<i32>,
        resp_y: Option<i32>,
    ) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE players SET resp_x = ?1, resp_y = ?2 WHERE id = ?3",
            params![resp_x, resp_y, player_id],
        )?;
        Ok(())
    }

    pub fn add_money_to_all(&self, amount: i64) -> Result<usize> {
        let updated = self
            .conn
            .lock()
            .execute("UPDATE players SET money = money + ?1", params![amount])?;
        Ok(updated)
    }
}
