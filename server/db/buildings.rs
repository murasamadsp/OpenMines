use super::Database;
use anyhow::Result;
use rusqlite::params;
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
    pub fn load_all_buildings(&self) -> Result<Vec<BuildingRow>> {
        let rows = self
            .conn
            .lock()
            .prepare("SELECT id, type_code, x, y, owner_id, clan_id, data FROM buildings")?
            .query_map([], |r| {
                let data_str: Option<String> = r.get(6).ok();
                let extra: BuildingExtra = data_str
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or_default();
                Ok(BuildingRow {
                    id: r.get(0)?,
                    type_code: r.get(1)?,
                    x: r.get(2)?,
                    y: r.get(3)?,
                    owner_id: r.get(4)?,
                    clan_id: r.get(5)?,
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
                })
            })?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        Ok(rows)
    }

    pub fn insert_building(
        &self,
        type_code: &str,
        x: i32,
        y: i32,
        owner_id: i32,
        clan_id: i32,
        extra: &BuildingExtra,
    ) -> Result<i32> {
        let data_json = serde_json::to_string(extra)?;
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO buildings (type_code, x, y, owner_id, clan_id, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![type_code, x, y, owner_id, clan_id, data_json],
        )?;
        let id = conn.last_insert_rowid();
        drop(conn);
        i32::try_from(id).map_err(|_| anyhow::anyhow!("building id overflow"))
    }

    pub fn delete_building(&self, building_id: i32) -> Result<()> {
        self.conn
            .lock()
            .execute("DELETE FROM buildings WHERE id = ?1", params![building_id])?;
        Ok(())
    }

    /// Полная очистка зданий (при смене мира / `--regen`), чтобы старые якоря не накладывались на новую карту.
    pub fn delete_all_buildings(&self) -> Result<u64> {
        let n = self.conn.lock().execute("DELETE FROM buildings", [])?;
        Ok(u64::try_from(n).unwrap_or(u64::MAX))
    }

    pub fn update_building_extra(&self, id: i32, extra: &BuildingExtra) -> Result<()> {
        let data_json = serde_json::to_string(extra)?;
        self.conn.lock().execute(
            "UPDATE buildings SET data = ?1 WHERE id = ?2",
            params![data_json, id],
        )?;
        Ok(())
    }

    /// Смена координат/типа/владельца здания (см. `update_pack_with_world_sync`).
    #[allow(clippy::too_many_arguments)]
    pub fn update_building_state(
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
        self.conn.lock().execute(
            "UPDATE buildings SET type_code = ?1, x = ?2, y = ?3, owner_id = ?4, clan_id = ?5, data = ?6 WHERE id = ?7",
            params![type_code_str.as_str(), x, y, owner_id, clan_id, data_json, id],
        )?;
        Ok(())
    }
}
