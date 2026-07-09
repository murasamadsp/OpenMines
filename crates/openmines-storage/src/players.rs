use super::{Database, programs::ProgramRow};
use anyhow::{Context as _, Result, bail};
pub use openmines_core::{ClanRank, Role};
use sqlx::Row;
use std::collections::HashMap;

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
    pub aggression: bool,
    pub crystals: [i64; 6],
    pub clan_id: Option<i32>,
    pub resp_x: Option<i32>,
    pub resp_y: Option<i32>,
    pub inventory: HashMap<i32, i32>,
    pub skills: SkillSlots,
    pub role: i32,
    pub selected_program_id: Option<i32>,
    pub selected_program: Option<ProgramRow>,
    pub programmator_running: bool,
    pub programmator_snapshot: Option<String>,
    pub clan_rank: i32,
    /// Время последнего клейма ежедневного бонуса (`GDon`), unix-секунды; 0 = ни разу.
    pub last_bonus_at: i64,
}

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

fn parse_inventory(player_id: i32, raw: &str) -> Result<HashMap<i32, i32>> {
    serde_json::from_str(raw)
        .with_context(|| format!("player id={player_id}: parse inventory JSON"))
}

fn parse_skills(player_id: i32, raw: &str) -> Result<SkillSlots> {
    serde_json::from_str::<SkillSlots>(raw)
        .or_else(|new_err| {
            serde_json::from_str::<HashMap<String, SkillState>>(raw)
                .map(migrate_old_skills)
                .map_err(|legacy_err| {
                    anyhow::anyhow!(
                        "player id={player_id}: parse skills JSON failed: SkillSlots={new_err}; legacy={legacy_err}"
                    )
                })
        })
}

fn row_to_player(r: &sqlx::sqlite::SqliteRow) -> Result<PlayerRow> {
    let id: i32 = r.try_get("id")?;
    let inv_str: String = r.try_get("inventory")?;
    let skills_str: String = r.try_get("skills")?;
    let auto_dig_val: i32 = r.try_get("auto_dig")?;
    let aggression_val: i32 = r.try_get("aggression")?;
    let programmator_running_val: i32 = r.try_get("programmator_running")?;
    let selected_program_id = r.try_get("selected_program_id")?;
    let selected_program_name: Option<String> = r.try_get("selected_program_name")?;
    let selected_program_code: Option<String> = r.try_get("selected_program_code")?;
    let selected_program = match (
        selected_program_id,
        selected_program_name,
        selected_program_code,
    ) {
        (Some(program_id), Some(name), Some(code)) => Some(ProgramRow {
            id: program_id,
            player_id: id,
            name,
            code,
        }),
        _ => None,
    };
    Ok(PlayerRow {
        id,
        name: r.try_get("name")?,
        passwd: r.try_get("passwd")?,
        hash: r.try_get("hash")?,
        x: r.try_get("x")?,
        y: r.try_get("y")?,
        dir: r.try_get("dir")?,
        health: r.try_get("health")?,
        max_health: r.try_get("max_health")?,
        money: r.try_get("money")?,
        creds: r.try_get("creds")?,
        skin: r.try_get("skin")?,
        auto_dig: auto_dig_val != 0,
        aggression: aggression_val != 0,
        crystals: [
            r.try_get("cry_green")?,
            r.try_get("cry_blue")?,
            r.try_get("cry_red")?,
            r.try_get("cry_violet")?,
            r.try_get("cry_white")?,
            r.try_get("cry_cyan")?,
        ],
        clan_id: r.try_get("clan_id")?,
        inventory: parse_inventory(id, &inv_str)?,
        skills: parse_skills(id, &skills_str)?,
        resp_x: r.try_get("resp_x")?,
        resp_y: r.try_get("resp_y")?,
        role: r.try_get("role")?,
        selected_program_id,
        selected_program,
        programmator_running: programmator_running_val != 0,
        programmator_snapshot: r.try_get("programmator_snapshot")?,
        clan_rank: r.try_get("clan_rank")?,
        last_bonus_at: r.try_get("last_bonus_at")?,
    })
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
            aggression: false,
            crystals: [0; 6],
            clan_id: None,
            clan_rank: 0,
            resp_x: None,
            resp_y: None,
            inventory: HashMap::new(),
            skills: default_skills(),
            role: 0,
            selected_program_id: None,
            selected_program: None,
            programmator_running: false,
            programmator_snapshot: None,
            last_bonus_at: 0,
        })
    }

    pub async fn update_player_passwd(&self, player_id: i32, passwd: &str) -> Result<()> {
        let result = sqlx::query("UPDATE players SET passwd = ?1 WHERE id = ?2")
            .bind(passwd)
            .bind(player_id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() != 1 {
            bail!(
                "player id={player_id}: update passwd affected {} rows",
                result.rows_affected()
            );
        }
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

    pub async fn set_selected_program(
        &self,
        player_id: i32,
        program_id: Option<i32>,
    ) -> Result<bool> {
        let result = sqlx::query("UPDATE players SET selected_program_id = ?1 WHERE id = ?2")
            .bind(program_id)
            .bind(player_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn get_player_by_id(&self, id: i32) -> Result<Option<PlayerRow>> {
        let row = sqlx::query(
            "SELECT players.id AS id, players.name AS name, passwd, hash, x, y, dir, health, max_health, money, creds, skin, auto_dig, aggression,
                    cry_green, cry_blue, cry_red, cry_violet, cry_white, cry_cyan, clan_id,
                    inventory, skills, resp_x, resp_y, role, selected_program_id,
                    sp.name AS selected_program_name, sp.code AS selected_program_code,
                    programmator_running, programmator_snapshot, clan_rank, last_bonus_at
             FROM players
             LEFT JOIN programs sp ON sp.id = players.selected_program_id AND sp.player_id = players.id
             WHERE players.id = ?1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|r| row_to_player(&r)).transpose()
    }

    pub async fn get_player_by_name(&self, name: &str) -> Result<Option<PlayerRow>> {
        let name = name.trim();
        if name.is_empty() {
            return Ok(None);
        }
        let row = sqlx::query(
            "SELECT players.id AS id, players.name AS name, passwd, hash, x, y, dir, health, max_health, money, creds, skin, auto_dig, aggression,
                    cry_green, cry_blue, cry_red, cry_violet, cry_white, cry_cyan, clan_id,
                    inventory, skills, resp_x, resp_y, role, selected_program_id,
                    sp.name AS selected_program_name, sp.code AS selected_program_code,
                    programmator_running, programmator_snapshot, clan_rank, last_bonus_at
             FROM players
             LEFT JOIN programs sp ON sp.id = players.selected_program_id AND sp.player_id = players.id
             WHERE lower(trim(players.name)) = lower(?1) ORDER BY players.id LIMIT 1"
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|r| row_to_player(&r)).transpose()
    }

    pub async fn save_player(&self, p: &PlayerRow) -> Result<()> {
        let inv_json = serde_json::to_string(&p.inventory)?;
        let skills_json = serde_json::to_string(&p.skills)?;
        let result = sqlx::query(
            "UPDATE players SET x=?1, y=?2, dir=?3, health=?4, max_health=?5, money=?6, creds=?7,
             skin=?8, auto_dig=?9, aggression=?27, cry_green=?10, cry_blue=?11, cry_red=?12, cry_violet=?13,
             cry_white=?14, cry_cyan=?15, clan_id=?16, passwd=?17, inventory=?18, skills=?19,
             resp_x=?21, resp_y=?22, clan_rank=?23, last_bonus_at=?24, programmator_running=?25,
             programmator_snapshot=?26
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
        .bind(i32::from(p.programmator_running))
        .bind(&p.programmator_snapshot)
        .bind(i32::from(p.aggression))
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            bail!(
                "player id={}: save affected {} rows",
                p.id,
                result.rows_affected()
            );
        }
        Ok(())
    }

    pub async fn save_players_batch(&self, players: &[PlayerRow]) -> Result<()> {
        if players.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        for p in players {
            let inv_json = serde_json::to_string(&p.inventory)?;
            let skills_json = serde_json::to_string(&p.skills)?;
            let result = sqlx::query(
                "UPDATE players SET x=?1, y=?2, dir=?3, health=?4, max_health=?5, money=?6, creds=?7,
                 skin=?8, auto_dig=?9, aggression=?27, cry_green=?10, cry_blue=?11, cry_red=?12, cry_violet=?13,
                 cry_white=?14, cry_cyan=?15, clan_id=?16, passwd=?17, inventory=?18, skills=?19,
                 resp_x=?21, resp_y=?22, clan_rank=?23, last_bonus_at=?24, programmator_running=?25,
                 programmator_snapshot=?26
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
            .bind(i32::from(p.programmator_running))
            .bind(&p.programmator_snapshot)
            .bind(i32::from(p.aggression))
            .execute(&mut *tx)
            .await?;
            if result.rows_affected() != 1 {
                bail!(
                    "player id={}: save affected {} rows",
                    p.id,
                    result.rows_affected()
                );
            }
        }
        tx.commit().await?;
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

    pub async fn update_player_resp(
        &self,
        player_id: i32,
        resp_x: Option<i32>,
        resp_y: Option<i32>,
    ) -> Result<()> {
        let result = sqlx::query("UPDATE players SET resp_x = ?1, resp_y = ?2 WHERE id = ?3")
            .bind(resp_x)
            .bind(resp_y)
            .bind(player_id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() != 1 {
            bail!(
                "player id={player_id}: update resp affected {} rows",
                result.rows_affected()
            );
        }
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
        let result = sqlx::query("UPDATE players SET money = money + ?1 WHERE id = ?2")
            .bind(amount)
            .bind(id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() != 1 {
            bail!(
                "player id={id}: add money affected {} rows",
                result.rows_affected()
            );
        }
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
            bail!("player id={id}: missing player for inventory credit");
        };
        let inv_str: String = row.try_get("inventory")?;
        let mut inv: HashMap<i32, i32> = serde_json::from_str(&inv_str)
            .with_context(|| format!("player id={id}: parse inventory JSON"))?;
        *inv.entry(item_id).or_insert(0) += count;
        let new_str = serde_json::to_string(&inv)?;
        let result = sqlx::query("UPDATE players SET inventory = ?1 WHERE id = ?2")
            .bind(new_str)
            .bind(id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() != 1 {
            bail!(
                "player id={id}: inventory credit affected {} rows",
                result.rows_affected()
            );
        }
        Ok(())
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
    async fn get_player_rejects_invalid_inventory_json() {
        let database = temp_database("player_bad_inventory").await;
        let player = database.create_player("bad-inv", "p", "h").await.unwrap();
        sqlx::query("UPDATE players SET inventory = ?1 WHERE id = ?2")
            .bind("{bad")
            .bind(player.id)
            .execute(&database.pool)
            .await
            .unwrap();

        let err = database.get_player_by_id(player.id).await.unwrap_err();
        assert!(err.to_string().contains("parse inventory JSON"));
    }

    #[tokio::test]
    async fn get_player_rejects_invalid_skills_json() {
        let database = temp_database("player_bad_skills").await;
        let player = database
            .create_player("bad-skills", "p", "h")
            .await
            .unwrap();
        sqlx::query("UPDATE players SET skills = ?1 WHERE id = ?2")
            .bind("{bad")
            .bind(player.id)
            .execute(&database.pool)
            .await
            .unwrap();

        let err = database.get_player_by_id(player.id).await.unwrap_err();
        assert!(err.to_string().contains("parse skills JSON failed"));
    }

    #[tokio::test]
    async fn get_player_migrates_legacy_skills_json_explicitly() {
        let database = temp_database("player_legacy_skills").await;
        let player = database
            .create_player("legacy-skills", "p", "h")
            .await
            .unwrap();
        sqlx::query("UPDATE players SET skills = ?1 WHERE id = ?2")
            .bind(r#"{"M":{"level":2,"exp":1.5},"__slots":{"level":25,"exp":0.0}}"#)
            .bind(player.id)
            .execute(&database.pool)
            .await
            .unwrap();

        let loaded = database.get_player_by_id(player.id).await.unwrap().unwrap();
        assert_eq!(loaded.skills.total_slots, 25);
        assert_eq!(loaded.skills.find("M").unwrap().level, 2);
    }

    #[tokio::test]
    async fn add_player_money_rejects_missing_player() {
        let database = temp_database("player_money_missing").await;

        let err = database.add_player_money(999_999, 10).await.unwrap_err();

        assert!(err.to_string().contains("add money affected 0 rows"));
    }

    #[tokio::test]
    async fn add_player_inventory_item_rejects_missing_player() {
        let database = temp_database("player_inventory_missing").await;

        let err = database
            .add_player_inventory_item(999_999, 1, 1)
            .await
            .unwrap_err();

        assert!(
            err.to_string()
                .contains("missing player for inventory credit")
        );
    }

    #[tokio::test]
    async fn save_player_rejects_missing_player() {
        let database = temp_database("player_save_missing").await;
        let mut player = database
            .create_player("save-missing", "p", "h")
            .await
            .unwrap();
        player.id = 999_999;

        let err = database.save_player(&player).await.unwrap_err();

        assert!(err.to_string().contains("save affected 0 rows"));
    }

    #[tokio::test]
    async fn update_player_passwd_rejects_missing_player() {
        let database = temp_database("player_passwd_missing").await;

        let err = database
            .update_player_passwd(999_999, "new-passwd")
            .await
            .unwrap_err();

        assert!(err.to_string().contains("update passwd affected 0 rows"));
    }

    #[tokio::test]
    async fn update_player_resp_rejects_missing_player() {
        let database = temp_database("player_resp_missing").await;

        let err = database
            .update_player_resp(999_999, Some(1), Some(2))
            .await
            .unwrap_err();

        assert!(err.to_string().contains("update resp affected 0 rows"));
    }
}
