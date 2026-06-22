use super::Database;
use anyhow::Result;
use sqlx::Row;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct BuildingRow {
    pub id: i32,
    pub type_code: String,
    pub x: i32,
    pub y: i32,
    pub owner_id: i32,
    pub clan_id: i32,
    pub charge: f32,
    pub max_charge: f32,
    pub cost: i32,
    pub hp: i32,
    pub max_hp: i32,
    pub money_inside: i64,
    pub crystals_inside: [i64; 6],
    pub items_inside: HashMap<i32, i32>,
    pub craft_recipe_id: Option<i32>,
    pub craft_num: i32,
    pub craft_end_ts: i64,
    pub clanzone: i32,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct BuildingExtra {
    #[serde(default)]
    pub charge: f32,
    #[serde(default = "default_max_charge")]
    pub max_charge: f32,
    #[serde(default = "default_cost")]
    pub cost: i32,
    #[serde(default = "default_hp")]
    pub hp: i32,
    #[serde(default = "default_hp")]
    pub max_hp: i32,
    #[serde(default)]
    pub money_inside: i64,
    #[serde(default)]
    pub crystals_inside: [i64; 6],
    #[serde(default)]
    pub items_inside: HashMap<i32, i32>,
    /// Crafter: текущий рецепт. None = крафтер простаивает.
    #[serde(default)]
    pub craft_recipe_id: Option<i32>,
    /// Crafter: сколько единиц крафтится.
    #[serde(default)]
    pub craft_num: i32,
    /// Crafter: unix-ts завершения крафта. 0 = простаивает.
    #[serde(default)]
    pub craft_end_ts: i64,
    /// Resp: клановая зона (admin-настройка; хранится, геймплейного эффекта нет — 1:1 C#).
    #[serde(default)]
    pub clanzone: i32,
}

const fn default_max_charge() -> f32 {
    1000.0
}
const fn default_cost() -> i32 {
    10
}
const fn default_hp() -> i32 {
    1000
}

impl Database {
    pub async fn load_all_buildings(&self) -> Result<Vec<BuildingRow>> {
        let rows =
            sqlx::query("SELECT id, type_code, x, y, owner_id, clan_id, data FROM buildings")
                .fetch_all(&self.pool)
                .await?;
        let res = rows
            .into_iter()
            .map(|r| {
                let data_str: Option<String> = r.get("data");
                let extra: BuildingExtra = data_str
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or_default();
                BuildingRow {
                    id: r.get("id"),
                    type_code: r.get("type_code"),
                    x: r.get("x"),
                    y: r.get("y"),
                    owner_id: r.get("owner_id"),
                    clan_id: r.get("clan_id"),
                    charge: extra.charge,
                    max_charge: extra.max_charge,
                    cost: extra.cost,
                    hp: extra.hp,
                    max_hp: extra.max_hp,
                    money_inside: extra.money_inside,
                    crystals_inside: extra.crystals_inside,
                    items_inside: extra.items_inside,
                    craft_recipe_id: extra.craft_recipe_id,
                    craft_num: extra.craft_num,
                    craft_end_ts: extra.craft_end_ts,
                    clanzone: extra.clanzone,
                }
            })
            .collect();
        Ok(res)
    }

    pub async fn insert_building(
        &self,
        type_code: &str,
        x: i32,
        y: i32,
        owner_id: i32,
        clan_id: i32,
        extra: &BuildingExtra,
    ) -> Result<i32> {
        let data_json = serde_json::to_string(extra)?;
        let result = sqlx::query(
            "INSERT INTO buildings (type_code, x, y, owner_id, clan_id, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
        )
        .bind(type_code)
        .bind(x)
        .bind(y)
        .bind(owner_id)
        .bind(clan_id)
        .bind(data_json)
        .execute(&self.pool)
        .await?;
        let id = result.last_insert_rowid();
        i32::try_from(id).map_err(|_| anyhow::anyhow!("building id overflow"))
    }

    pub async fn delete_building(&self, building_id: i32) -> Result<()> {
        sqlx::query("DELETE FROM buildings WHERE id = ?1")
            .bind(building_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Полная очистка зданий (при смене мира / `--regen`), чтобы старые якоря не накладывались на новую карту.
    pub async fn delete_all_buildings(&self) -> Result<u64> {
        let result = sqlx::query("DELETE FROM buildings")
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    #[allow(dead_code)]
    pub async fn update_building_extra(&self, id: i32, extra: &BuildingExtra) -> Result<()> {
        let data_json = serde_json::to_string(extra)?;
        sqlx::query("UPDATE buildings SET data = ?1 WHERE id = ?2")
            .bind(data_json)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Смена координат/типа/владельца здания (см. `update_pack_with_world_sync`).
    #[allow(clippy::too_many_arguments)]
    pub async fn update_building_state(
        &self,
        id: i32,
        type_code: u8,
        x: i32,
        y: i32,
        owner_id: i32,
        clan_id: i32,
        extra: &BuildingExtra,
    ) -> Result<()> {
        let data_json = serde_json::to_string(extra)?;
        let type_code_str = char::from(type_code).to_string();
        sqlx::query(
            "UPDATE buildings SET type_code = ?1, x = ?2, y = ?3, owner_id = ?4, clan_id = ?5, data = ?6 WHERE id = ?7"
        )
        .bind(type_code_str)
        .bind(x)
        .bind(y)
        .bind(owner_id)
        .bind(clan_id)
        .bind(data_json)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn save_building(&self, row: &BuildingRow) -> Result<()> {
        let extra = BuildingExtra {
            charge: row.charge,
            max_charge: row.max_charge,
            cost: row.cost,
            hp: row.hp,
            max_hp: row.max_hp,
            money_inside: row.money_inside,
            crystals_inside: row.crystals_inside,
            items_inside: row.items_inside.clone(),
            craft_recipe_id: row.craft_recipe_id,
            craft_num: row.craft_num,
            craft_end_ts: row.craft_end_ts,
            clanzone: row.clanzone,
        };
        let type_code = row.type_code.chars().next().map_or(b' ', |c| c as u8);
        self.update_building_state(
            row.id,
            type_code,
            row.x,
            row.y,
            row.owner_id,
            row.clan_id,
            &extra,
        )
        .await
    }
}
