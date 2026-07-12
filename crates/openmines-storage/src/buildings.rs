use super::Database;
use anyhow::{Context as _, Result, bail};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct BuildingRow {
    pub id: i32,
    pub type_code: String,
    pub x: i32,
    pub y: i32,
    pub owner_id: i32,
    pub clan_id: i32,
    pub charge: i32,
    pub max_charge: i32,
    pub cost: i32,
    pub hp: i32,
    pub max_hp: i32,
    pub money_inside: i64,
    pub crystals_inside: [i64; 6],
    pub items_inside: HashMap<i32, i32>,
    pub craft_recipe_id: Option<i32>,
    pub craft_num: i32,
    pub craft_end_ts: i64,
    pub craft_ready: bool,
    pub clanzone: i32,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BuildingDeleteWrite {
    pub building_id: i32,
    pub x: i32,
    pub y: i32,
    pub clear_resp_bindings: bool,
    pub box_write: Option<crate::BoxWrite>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum BuildingDeleteOutcome {
    Deleted { cleared_resp_bindings: u64 },
    IdentityMismatch,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BuildingExtra {
    #[serde(deserialize_with = "json_int::i32")]
    pub charge: i32,
    #[serde(deserialize_with = "json_int::i32")]
    pub max_charge: i32,
    #[serde(deserialize_with = "json_int::i32")]
    pub cost: i32,
    #[serde(deserialize_with = "json_int::i32")]
    pub hp: i32,
    #[serde(deserialize_with = "json_int::i32")]
    pub max_hp: i32,
    #[serde(deserialize_with = "json_int::i64")]
    pub money_inside: i64,
    pub crystals_inside: [i64; 6],
    pub items_inside: HashMap<i32, i32>,
    /// Crafter: текущий рецепт. None = крафтер простаивает.
    pub craft_recipe_id: Option<i32>,
    /// Crafter: сколько единиц крафтится.
    #[serde(deserialize_with = "json_int::i32")]
    pub craft_num: i32,
    /// Crafter: unix-ts завершения крафта. 0 = простаивает.
    #[serde(deserialize_with = "json_int::i64")]
    pub craft_end_ts: i64,
    /// Crafter: completion resend already emitted. Mirrors C# `Crafter.ready`.
    pub craft_ready: bool,
    /// Resp: клановая зона (admin-настройка; хранится, геймплейного эффекта нет — 1:1 C#).
    #[serde(deserialize_with = "json_int::i32")]
    pub clanzone: i32,
}

mod json_int {
    use num_traits::ToPrimitive as _;
    use serde::Deserialize as _;

    pub fn i32<'de, D>(deserializer: D) -> Result<i32, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        parse_i64(&value)
            .and_then(|v| i32::try_from(v).ok())
            .ok_or_else(|| {
                serde::de::Error::custom(format!("expected integer-compatible i32, got {value}"))
            })
    }

    pub fn i64<'de, D>(deserializer: D) -> Result<i64, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        parse_i64(&value).ok_or_else(|| {
            serde::de::Error::custom(format!("expected integer-compatible i64, got {value}"))
        })
    }

    fn parse_i64(value: &serde_json::Value) -> Option<i64> {
        match value {
            serde_json::Value::Number(n) => n
                .as_i64()
                .or_else(|| n.as_u64().and_then(|v| i64::try_from(v).ok()))
                .or_else(|| {
                    let f = n.as_f64()?;
                    if f.is_finite() && f.fract().abs() < f64::EPSILON {
                        f.to_i64()
                    } else {
                        None
                    }
                }),
            _ => None,
        }
    }
}

/// Преобразует строку sqlx в `BuildingRow` (переиспользуется в нескольких методах).
fn parse_building_row(r: &sqlx::sqlite::SqliteRow) -> Result<BuildingRow> {
    use sqlx::Row as _;
    let id: i32 = r.try_get("id")?;
    let data_str: Option<String> = r.try_get("data")?;
    let data_str = data_str
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("building id={id}: data is missing"))?;
    let extra: BuildingExtra = serde_json::from_str(data_str)
        .with_context(|| format!("building id={id}: parse data JSON"))?;
    Ok(BuildingRow {
        id,
        type_code: r.try_get("type_code")?,
        x: r.try_get("x")?,
        y: r.try_get("y")?,
        owner_id: r.try_get("owner_id")?,
        clan_id: r.try_get("clan_id")?,
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
        craft_ready: extra.craft_ready,
        clanzone: extra.clanzone,
    })
}

impl Database {
    pub async fn load_all_buildings(&self) -> Result<Vec<BuildingRow>> {
        let rows =
            sqlx::query("SELECT id, type_code, x, y, owner_id, clan_id, data FROM buildings")
                .fetch_all(&self.pool)
                .await?;
        rows.iter().map(parse_building_row).collect()
    }

    /// Загрузить только здания конкретного владельца (`WHERE owner_id = ?`).
    /// PB-4: заменяет `load_all_buildings()` + in-memory filter в `handle_my_buildings_list`.
    pub async fn load_buildings_by_owner(&self, owner_id: i32) -> Result<Vec<BuildingRow>> {
        let rows = sqlx::query(
            "SELECT id, type_code, x, y, owner_id, clan_id, data FROM buildings WHERE owner_id = ?1",
        )
        .bind(owner_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(parse_building_row).collect()
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

    pub async fn apply_building_delete(
        &self,
        write: &BuildingDeleteWrite,
    ) -> Result<BuildingDeleteOutcome> {
        let mut tx = self.pool.begin().await?;
        let deleted = sqlx::query("DELETE FROM buildings WHERE id = ?1 AND x = ?2 AND y = ?3")
            .bind(write.building_id)
            .bind(write.x)
            .bind(write.y)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if deleted != 1 {
            let conflicting_identity = sqlx::query_scalar::<_, i64>(
                "SELECT EXISTS(SELECT 1 FROM buildings WHERE id = ?1 OR (x = ?2 AND y = ?3))",
            )
            .bind(write.building_id)
            .bind(write.x)
            .bind(write.y)
            .fetch_one(&mut *tx)
            .await?
                != 0;
            if conflicting_identity {
                tx.rollback().await?;
                return Ok(BuildingDeleteOutcome::IdentityMismatch);
            }
        }

        let cleared_resp_bindings = if write.clear_resp_bindings {
            sqlx::query(
                "UPDATE players SET resp_x = NULL, resp_y = NULL WHERE resp_x = ?1 AND resp_y = ?2",
            )
            .bind(write.x)
            .bind(write.y)
            .execute(&mut *tx)
            .await?
            .rows_affected()
        } else {
            0
        };
        if let Some(box_write) = &write.box_write {
            crate::boxes::apply_box_write(&mut tx, box_write).await?;
        }
        tx.commit().await?;
        Ok(BuildingDeleteOutcome::Deleted {
            cleared_resp_bindings,
        })
    }

    /// Полная очистка зданий (при смене мира / `--regen`), чтобы старые якоря не накладывались на новую карту.
    pub async fn delete_all_buildings(&self) -> Result<u64> {
        let result = sqlx::query("DELETE FROM buildings")
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    pub async fn update_building_extra(&self, id: i32, extra: &BuildingExtra) -> Result<()> {
        let data_json = serde_json::to_string(extra)?;
        let result = sqlx::query("UPDATE buildings SET data = ?1 WHERE id = ?2")
            .bind(data_json)
            .bind(id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() != 1 {
            bail!(
                "building id={id}: update extra affected {} rows",
                result.rows_affected()
            );
        }
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
        let result = sqlx::query(
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
        if result.rows_affected() != 1 {
            bail!(
                "building id={id}: update state affected {} rows",
                result.rows_affected()
            );
        }
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
            craft_ready: row.craft_ready,
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

    pub async fn save_buildings_batch(&self, buildings: &[BuildingRow]) -> Result<()> {
        if buildings.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        for row in buildings {
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
                craft_ready: row.craft_ready,
                clanzone: row.clanzone,
            };
            let type_code = row.type_code.chars().next().map_or(b' ', |c| c as u8);
            let data_json = serde_json::to_string(&extra)?;
            let type_code_str = char::from(type_code).to_string();
            let result = sqlx::query(
                "UPDATE buildings SET type_code = ?1, x = ?2, y = ?3, owner_id = ?4, clan_id = ?5, data = ?6 WHERE id = ?7"
            )
            .bind(type_code_str)
            .bind(row.x)
            .bind(row.y)
            .bind(row.owner_id)
            .bind(row.clan_id)
            .bind(data_json)
            .bind(row.id)
            .execute(&mut *tx)
            .await?;
            if result.rows_affected() != 1 {
                bail!(
                    "building id={}: update state affected {} rows",
                    row.id,
                    result.rows_affected()
                );
            }
        }
        tx.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{BuildingDeleteOutcome, BuildingDeleteWrite, BuildingExtra, Database};
    use std::collections::HashMap;

    async fn temp_database(name: &str) -> Database {
        let path = std::env::temp_dir().join(format!("{name}_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        Database::open(path).await.unwrap()
    }

    #[tokio::test]
    async fn load_all_buildings_rejects_missing_data() {
        let database = temp_database("building_missing_data").await;
        sqlx::query(
            "INSERT INTO buildings (type_code, x, y, owner_id, clan_id, data) VALUES ('R', 1, 2, 0, 0, NULL)",
        )
        .execute(&database.pool)
        .await
        .unwrap();

        let err = database.load_all_buildings().await.unwrap_err();
        assert!(err.to_string().contains("data is missing"));
    }

    #[tokio::test]
    async fn load_all_buildings_rejects_incomplete_data() {
        let database = temp_database("building_incomplete_data").await;
        sqlx::query(
            "INSERT INTO buildings (type_code, x, y, owner_id, clan_id, data) VALUES ('R', 1, 2, 0, 0, '{\"hp\":100}')",
        )
        .execute(&database.pool)
        .await
        .unwrap();

        let err = database.load_all_buildings().await.unwrap_err();
        assert!(err.to_string().contains("parse data JSON"));
    }

    #[tokio::test]
    async fn load_all_buildings_accepts_complete_data() {
        let database = temp_database("building_complete_data").await;
        let extra = BuildingExtra {
            charge: 0,
            max_charge: 100,
            cost: 10,
            hp: 100,
            max_hp: 100,
            money_inside: 0,
            crystals_inside: [0; 6],
            items_inside: HashMap::new(),
            craft_recipe_id: None,
            craft_num: 0,
            craft_end_ts: 0,
            craft_ready: false,
            clanzone: 0,
        };
        database
            .insert_building("R", 1, 2, 0, 0, &extra)
            .await
            .unwrap();

        let buildings = database.load_all_buildings().await.unwrap();
        assert_eq!(buildings.len(), 1);
        assert_eq!(buildings[0].hp, 100);
    }

    #[test]
    fn building_extra_rejects_float_charge_state() {
        let json = r#"{
            "charge": 0.5,
            "max_charge": 100,
            "cost": 10,
            "hp": 100,
            "max_hp": 100,
            "money_inside": 0,
            "crystals_inside": [0,0,0,0,0,0],
            "items_inside": {},
            "craft_recipe_id": null,
            "craft_num": 0,
            "craft_end_ts": 0,
            "craft_ready": false,
            "clanzone": 0
        }"#;
        assert!(serde_json::from_str::<BuildingExtra>(json).is_err());
    }

    #[test]
    fn building_extra_accepts_integer_legacy_float_charge_state() {
        let json = r#"{
            "charge": 0.0,
            "max_charge": 100.0,
            "cost": 10,
            "hp": 100,
            "max_hp": 100,
            "money_inside": 0,
            "crystals_inside": [0,0,0,0,0,0],
            "items_inside": {},
            "craft_recipe_id": null,
            "craft_num": 0,
            "craft_end_ts": 0,
            "craft_ready": false,
            "clanzone": 0
        }"#;
        let extra = serde_json::from_str::<BuildingExtra>(json).unwrap();
        assert_eq!(extra.charge, 0);
        assert_eq!(extra.max_charge, 100);
    }

    #[tokio::test]
    async fn update_rejects_missing_building() {
        let database = temp_database("building_missing_update").await;
        let extra = BuildingExtra {
            charge: 0,
            max_charge: 100,
            cost: 10,
            hp: 100,
            max_hp: 100,
            money_inside: 0,
            crystals_inside: [0; 6],
            items_inside: HashMap::new(),
            craft_recipe_id: None,
            craft_num: 0,
            craft_end_ts: 0,
            craft_ready: false,
            clanzone: 0,
        };

        let extra_err = database
            .update_building_extra(999_999, &extra)
            .await
            .unwrap_err();
        let state_err = database
            .update_building_state(999_999, b'R', 1, 2, 3, 4, &extra)
            .await
            .unwrap_err();

        assert!(
            extra_err
                .to_string()
                .contains("update extra affected 0 rows")
        );
        assert!(
            state_err
                .to_string()
                .contains("update state affected 0 rows")
        );
    }

    async fn building_delete_fixture(name: &str) -> (Database, BuildingExtra, i32, i32) {
        let database = temp_database(name).await;
        let extra = BuildingExtra {
            charge: 100,
            max_charge: 1000,
            cost: 10,
            hp: 1000,
            max_hp: 1000,
            money_inside: 0,
            crystals_inside: [0; 6],
            items_inside: HashMap::new(),
            craft_recipe_id: None,
            craft_num: 0,
            craft_end_ts: 0,
            craft_ready: false,
            clanzone: 0,
        };
        let building_id = database
            .insert_building("R", 10, 20, 1, 0, &extra)
            .await
            .unwrap();
        let player = database
            .create_player("typed-delete-bound", "p", "h")
            .await
            .unwrap();
        database
            .update_player_resp(player.id, Some(10), Some(20))
            .await
            .unwrap();
        (database, extra, building_id, player.id)
    }

    fn test_box_write() -> crate::BoxWrite {
        crate::BoxWrite {
            x: 10,
            y: 20,
            crystals: Some([1, 2, 3, 4, 5, 6]),
        }
    }

    #[tokio::test]
    async fn typed_building_delete_rejects_identity_mismatch_without_side_effects() {
        let (database, _, building_id, player_id) =
            building_delete_fixture("typed_building_delete_mismatch").await;
        let mismatch = database
            .apply_building_delete(&BuildingDeleteWrite {
                building_id,
                x: 11,
                y: 20,
                clear_resp_bindings: true,
                box_write: Some(test_box_write()),
            })
            .await
            .unwrap();
        assert_eq!(mismatch, BuildingDeleteOutcome::IdentityMismatch);
        assert_eq!(database.load_all_buildings().await.unwrap().len(), 1);
        let still_bound = database.get_player_by_id(player_id).await.unwrap().unwrap();
        assert_eq!(
            (still_bound.resp_x, still_bound.resp_y),
            (Some(10), Some(20))
        );
        assert!(database.load_all_boxes().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn typed_building_delete_is_atomic_with_box_and_idempotent() {
        let (database, _, building_id, player_id) =
            building_delete_fixture("typed_building_delete_atomic").await;
        let write = BuildingDeleteWrite {
            building_id,
            x: 10,
            y: 20,
            clear_resp_bindings: true,
            box_write: Some(test_box_write()),
        };
        let deleted = database.apply_building_delete(&write).await.unwrap();
        assert_eq!(
            deleted,
            BuildingDeleteOutcome::Deleted {
                cleared_resp_bindings: 1
            }
        );
        assert!(database.load_all_buildings().await.unwrap().is_empty());
        let cleared = database.get_player_by_id(player_id).await.unwrap().unwrap();
        assert_eq!((cleared.resp_x, cleared.resp_y), (None, None));
        assert_eq!(
            database.load_all_boxes().await.unwrap(),
            vec![(10, 20, [1, 2, 3, 4, 5, 6])]
        );

        let replayed = database.apply_building_delete(&write).await.unwrap();
        assert_eq!(
            replayed,
            BuildingDeleteOutcome::Deleted {
                cleared_resp_bindings: 0
            }
        );
        assert_eq!(
            database.load_all_boxes().await.unwrap(),
            vec![(10, 20, [1, 2, 3, 4, 5, 6])]
        );
    }

    #[tokio::test]
    async fn idempotent_delete_replay_rejects_coordinate_replacement() {
        let (database, extra, building_id, _) =
            building_delete_fixture("typed_building_delete_replacement").await;
        let write = BuildingDeleteWrite {
            building_id,
            x: 10,
            y: 20,
            clear_resp_bindings: true,
            box_write: None,
        };
        assert!(matches!(
            database.apply_building_delete(&write).await.unwrap(),
            BuildingDeleteOutcome::Deleted { .. }
        ));
        let replacement_id = database
            .insert_building("R", 10, 20, 2, 0, &extra)
            .await
            .unwrap();
        let replacement_conflict = database.apply_building_delete(&write).await.unwrap();
        assert_eq!(
            replacement_conflict,
            BuildingDeleteOutcome::IdentityMismatch
        );
        assert!(
            database
                .load_all_buildings()
                .await
                .unwrap()
                .iter()
                .any(|building| building.id == replacement_id)
        );
    }
}
