use super::{Database, clans::ClanRank};
use anyhow::{Result, bail};
use sqlx::Row;
use std::collections::HashMap;

/// Роль игрока (`players.role`). Новые уровни — только новыми числами, старые не переиспользовать.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i32)]
pub enum Role {
    Player = 0,
    Moderator = 1,
    Admin = 2,
}

#[allow(dead_code)]
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
    pub const fn is_moderator_effective(self) -> bool {
        matches!(self, Self::Moderator | Self::Admin)
    }
}

/// Default skills for a new player — basic digging, movement, building, mining, health, repair
const fn default_total_slots() -> i32 {
    20
}

/// Дефолтные скиллы нового игрока
#[must_use]
pub fn default_skills() -> SkillSlots {
    let mut skills = HashMap::new();
    skills.insert(
        0,
        SkillEntry {
            code: "m".into(),
            level: 1,
            exp: 0.0,
        },
    ); // MineGeneral
    skills.insert(
        1,
        SkillEntry {
            code: "d".into(),
            level: 1,
            exp: 0.0,
        },
    ); // Digging
    skills.insert(
        2,
        SkillEntry {
            code: "M".into(),
            level: 1,
            exp: 0.0,
        },
    ); // Movement
    skills.insert(
        3,
        SkillEntry {
            code: "l".into(),
            level: 1,
            exp: 0.0,
        },
    ); // Health
    SkillSlots {
        skills,
        total_slots: 20,
    }
}

/// Скилл в слоте (1:1 C# `Skill`: код типа + уровень + опыт).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillEntry {
    pub code: String,
    pub level: i32,
    pub exp: f32,
}

/// Слотовая модель скиллов — 1:1 C# `PlayerSkills`
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillSlots {
    pub skills: HashMap<i32, SkillEntry>,
    #[serde(default = "default_total_slots")]
    pub total_slots: i32,
}

impl SkillSlots {
    #[must_use]
    pub fn find(&self, code: &str) -> Option<&SkillEntry> {
        self.skills.values().find(|e| e.code == code)
    }
    pub fn find_mut(&mut self, code: &str) -> Option<&mut SkillEntry> {
        self.skills.values_mut().find(|e| e.code == code)
    }
    #[must_use]
    pub fn lvl_summary(&self) -> i32 {
        self.skills.values().map(|e| e.level).sum()
    }
}

/// Старый формат БД
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillState {
    pub level: i32,
    pub exp: f32,
}

#[must_use]
fn migrate_old_skills(old: HashMap<String, SkillState>) -> SkillSlots {
    let mut total = 20;
    let mut entries: Vec<(String, SkillState)> = Vec::new();
    for (code, st) in old {
        if code == "__slots" {
            total = st.level.max(20);
            continue;
        }
        entries.push((code, st));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut skills = HashMap::new();
    for (i, (code, st)) in entries.into_iter().enumerate() {
        let slot = i32::try_from(i).unwrap_or(0);
        skills.insert(
            slot,
            SkillEntry {
                code,
                level: st.level,
                exp: st.exp,
            },
        );
    }
    SkillSlots {
        skills,
        total_slots: total,
    }
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
    pub skills: SkillSlots,
    pub role: i32,
    pub clan_rank: i32,
    /// Время последнего клейма ежедневного бонуса (`GDon`), unix-секунды; 0 = ни разу.
    pub last_bonus_at: i64,
}

#[allow(dead_code)]
impl PlayerRow {
    #[inline]
    #[must_use]
    pub const fn as_role(&self) -> Role {
        Role::from_db(self.role)
    }

    #[inline]
    #[must_use]
    pub const fn as_clan_rank(&self) -> ClanRank {
        ClanRank::from_db(self.clan_rank)
    }
}

fn row_to_player(r: &sqlx::sqlite::SqliteRow) -> PlayerRow {
    let inv_str: String = r.try_get("inventory").unwrap_or_default();
    let skills_str: String = r.try_get("skills").unwrap_or_default();
    let auto_dig_val: i32 = r.try_get("auto_dig").unwrap_or(0);
    PlayerRow {
        id: r.try_get("id").unwrap(),
        name: r.try_get("name").unwrap(),
        passwd: r.try_get("passwd").unwrap(),
        hash: r.try_get("hash").unwrap(),
        x: r.try_get("x").unwrap(),
        y: r.try_get("y").unwrap(),
        dir: r.try_get("dir").unwrap(),
        health: r.try_get("health").unwrap(),
        max_health: r.try_get("max_health").unwrap(),
        money: r.try_get("money").unwrap(),
        creds: r.try_get("creds").unwrap(),
        skin: r.try_get("skin").unwrap(),
        auto_dig: auto_dig_val != 0,
        crystals: [
            r.try_get("cry_green").unwrap(),
            r.try_get("cry_blue").unwrap(),
            r.try_get("cry_red").unwrap(),
            r.try_get("cry_violet").unwrap(),
            r.try_get("cry_white").unwrap(),
            r.try_get("cry_cyan").unwrap(),
        ],
        clan_id: r.try_get("clan_id").unwrap_or(None),
        inventory: serde_json::from_str(&inv_str).unwrap_or_default(),
        skills: serde_json::from_str::<SkillSlots>(&skills_str)
            .or_else(|_| {
                serde_json::from_str::<HashMap<String, SkillState>>(&skills_str)
                    .map(migrate_old_skills)
            })
            .unwrap_or_else(|_| default_skills()),
        resp_x: r.try_get("resp_x").unwrap_or(None),
        resp_y: r.try_get("resp_y").unwrap_or(None),
        role: r.try_get("role").unwrap_or(0),
        clan_rank: r.try_get("clan_rank").unwrap_or(0),
        last_bonus_at: r.try_get("last_bonus_at").unwrap_or(0),
    }
}

impl Database {
    pub async fn create_player(&self, name: &str, passwd: &str, hash: &str) -> Result<PlayerRow> {
        let name = name.trim();
        if name.is_empty() {
            bail!("empty player name");
        }
        let skills_json = serde_json::to_string(&default_skills())?;

        let result =
            sqlx::query("INSERT INTO players (name, passwd, hash, skills) VALUES (?1, ?2, ?3, ?4)")
                .bind(name)
                .bind(passwd)
                .bind(hash)
                .bind(skills_json)
                .execute(&self.pool)
                .await?;

        let id = i32::try_from(result.last_insert_rowid())
            .map_err(|_| anyhow::anyhow!("player id overflow"))?;

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
            last_bonus_at: 0,
        })
    }

    pub async fn update_player_passwd(&self, player_id: i32, passwd: &str) -> Result<()> {
        sqlx::query("UPDATE players SET passwd = ?1 WHERE id = ?2")
            .bind(passwd)
            .bind(player_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_player_role(&self, player_id: i32, role: Role) -> Result<bool> {
        let result = sqlx::query("UPDATE players SET role = ?1 WHERE id = ?2")
            .bind(role as i32)
            .bind(player_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn get_player_by_id(&self, id: i32) -> Result<Option<PlayerRow>> {
        let row = sqlx::query(
            "SELECT id, name, passwd, hash, x, y, dir, health, max_health, money, creds, skin, auto_dig,
                    cry_green, cry_blue, cry_red, cry_violet, cry_white, cry_cyan, clan_id,
                    inventory, skills, resp_x, resp_y, role, clan_rank, last_bonus_at
             FROM players WHERE id = ?1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| row_to_player(&r)))
    }

    pub async fn get_player_by_name(&self, name: &str) -> Result<Option<PlayerRow>> {
        let name = name.trim();
        if name.is_empty() {
            return Ok(None);
        }
        let row = sqlx::query(
            "SELECT id, name, passwd, hash, x, y, dir, health, max_health, money, creds, skin, auto_dig,
                    cry_green, cry_blue, cry_red, cry_violet, cry_white, cry_cyan, clan_id,
                    inventory, skills, resp_x, resp_y, role, clan_rank, last_bonus_at
             FROM players WHERE lower(trim(name)) = lower(?1) ORDER BY id LIMIT 1"
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| row_to_player(&r)))
    }

    pub async fn save_player(&self, p: &PlayerRow) -> Result<()> {
        let inv_json = serde_json::to_string(&p.inventory)?;
        let skills_json = serde_json::to_string(&p.skills)?;
        sqlx::query(
            "UPDATE players SET x=?1, y=?2, dir=?3, health=?4, max_health=?5, money=?6, creds=?7,
             skin=?8, auto_dig=?9, cry_green=?10, cry_blue=?11, cry_red=?12, cry_violet=?13,
             cry_white=?14, cry_cyan=?15, clan_id=?16, passwd=?17, inventory=?18, skills=?19,
             resp_x=?21, resp_y=?22, clan_rank=?23, last_bonus_at=?24
             WHERE id=?20",
        )
        .bind(p.x)
        .bind(p.y)
        .bind(p.dir)
        .bind(p.health)
        .bind(p.max_health)
        .bind(p.money)
        .bind(p.creds)
        .bind(p.skin)
        .bind(i32::from(p.auto_dig))
        .bind(p.crystals[0])
        .bind(p.crystals[1])
        .bind(p.crystals[2])
        .bind(p.crystals[3])
        .bind(p.crystals[4])
        .bind(p.crystals[5])
        .bind(p.clan_id)
        .bind(&p.passwd)
        .bind(inv_json)
        .bind(skills_json)
        .bind(p.id)
        .bind(p.resp_x)
        .bind(p.resp_y)
        .bind(p.clan_rank)
        .bind(p.last_bonus_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Сброс позиции и точки респавна ВСЕХ игроков на спавн `(x, y)`. Вызывается
    /// при `--regen`: после регена рельеф полностью новый, а старые `x/y` (и
    /// `resp_x/resp_y`) указывают внутрь сгенерированных блоков → игрок логинится
    /// (или респавнится после смерти) внутри камня и мгновенно умирает («покорёженный
    /// спавн»). Прогресс (инвентарь/скиллы/деньги) НЕ трогаем — только координаты.
    pub async fn reset_all_players_to_spawn(&self, x: i32, y: i32) -> Result<u64> {
        let res = sqlx::query("UPDATE players SET x=?1, y=?2, dir=0, resp_x=?1, resp_y=?2")
            .bind(x)
            .bind(y)
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected())
    }

    pub async fn player_name_exists(&self, name: &str) -> Result<bool> {
        let name = name.trim();
        if name.is_empty() {
            return Ok(false);
        }
        let row = sqlx::query("SELECT COUNT(*) FROM players WHERE lower(trim(name)) = lower(?1)")
            .bind(name)
            .fetch_one(&self.pool)
            .await?;
        let count: i32 = row.get(0);
        Ok(count > 0)
    }

    #[allow(dead_code)]
    pub async fn update_player_resp(
        &self,
        player_id: i32,
        resp_x: Option<i32>,
        resp_y: Option<i32>,
    ) -> Result<()> {
        sqlx::query("UPDATE players SET resp_x = ?1, resp_y = ?2 WHERE id = ?3")
            .bind(resp_x)
            .bind(resp_y)
            .bind(player_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn add_money_to_all(&self, amount: i64) -> Result<usize> {
        let result = sqlx::query("UPDATE players SET money = money + ?1")
            .bind(amount)
            .execute(&self.pool)
            .await?;
        Ok(usize::try_from(result.rows_affected()).unwrap_or(usize::MAX))
    }

    /// Начислить деньги офлайн-игроку напрямую в БД (online — через ECS).
    /// Используется финализацией аукциона (`net::auction`).
    pub async fn add_player_money(&self, id: i32, amount: i64) -> Result<()> {
        sqlx::query("UPDATE players SET money = money + ?1 WHERE id = ?2")
            .bind(amount)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Добавить предмет в инвентарь офлайн-игрока (read-modify-write JSON,
    /// формат 1:1 с online-сохранением: `HashMap<i32,i32>`). Online — через ECS.
    pub async fn add_player_inventory_item(&self, id: i32, item_id: i32, count: i32) -> Result<()> {
        let Some(row) = sqlx::query("SELECT inventory FROM players WHERE id = ?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
        else {
            return Ok(());
        };
        let inv_str: String = row.try_get("inventory").unwrap_or_default();
        let mut inv: HashMap<i32, i32> = serde_json::from_str(&inv_str).unwrap_or_default();
        *inv.entry(item_id).or_insert(0) += count;
        let new_str = serde_json::to_string(&inv)?;
        sqlx::query("UPDATE players SET inventory = ?1 WHERE id = ?2")
            .bind(new_str)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
