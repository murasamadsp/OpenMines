#![allow(
    clippy::missing_errors_doc,
    clippy::must_use_candidate,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::similar_names,
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::struct_excessive_bools,
    clippy::wildcard_imports,
    clippy::manual_let_else,
    clippy::redundant_pub_crate,
    clippy::too_long_first_doc_paragraph
)]

pub mod boxes;
pub mod buildings;
pub mod chats;
pub mod clans;
pub mod events;
pub mod orders;
pub mod players;
pub mod programs;
pub mod provider;

pub use boxes::{BoxWrite, pick_box_coord};
pub use buildings::{BuildingDeleteOutcome, BuildingDeleteWrite, BuildingExtra, BuildingRow};
pub use chats::ChatRow;
pub use clans::{ClanRank, ClanRow};
pub use events::EventRow;
pub use players::{PlayerRow, Role, SkillEntry, SkillSlots};
pub use programs::ProgramRow;

use anyhow::Result;
use sqlx::SqlitePool;
use std::path::Path;

pub struct Database {
    pub pool: SqlitePool,
}

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
        Ok(db)
    }
}

impl Database {
    /// Atomically persists a player withdrawal from a building.
    pub async fn save_resp_profit_batch(
        &self,
        transfers: &[(players::PlayerRow, buildings::BuildingRow)],
    ) -> Result<()> {
        if transfers.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        for (player, building) in transfers {
            let inventory = serde_json::to_string(&player.inventory)?;
            let skills = serde_json::to_string(&player.skills)?;
            let player_result = sqlx::query(
                "UPDATE players SET x=?1, y=?2, dir=?3, health=?4, max_health=?5, money=?6, creds=?7,
                 skin=?8, auto_dig=?9, aggression=?27, cry_green=?10, cry_blue=?11, cry_red=?12,
                 cry_violet=?13, cry_white=?14, cry_cyan=?15, clan_id=?16, passwd=?17,
                 inventory=?18, skills=?19, resp_x=?21, resp_y=?22, clan_rank=?23,
                 last_bonus_at=?24, programmator_running=?25, programmator_snapshot=?26
                 WHERE id=?20",
            )
            .bind(player.x)
            .bind(player.y)
            .bind(player.dir)
            .bind(player.health)
            .bind(player.max_health)
            .bind(player.money)
            .bind(player.creds)
            .bind(player.skin)
            .bind(i32::from(player.auto_dig))
            .bind(player.crystals[0])
            .bind(player.crystals[1])
            .bind(player.crystals[2])
            .bind(player.crystals[3])
            .bind(player.crystals[4])
            .bind(player.crystals[5])
            .bind(player.clan_id)
            .bind(&player.passwd)
            .bind(inventory)
            .bind(skills)
            .bind(player.id)
            .bind(player.resp_x)
            .bind(player.resp_y)
            .bind(player.clan_rank)
            .bind(player.last_bonus_at)
            .bind(i32::from(player.programmator_running))
            .bind(&player.programmator_snapshot)
            .bind(i32::from(player.aggression))
            .execute(&mut *tx)
            .await?;
            if player_result.rows_affected() != 1 {
                anyhow::bail!(
                    "player id={}: save affected {} rows",
                    player.id,
                    player_result.rows_affected()
                );
            }

            let extra = buildings::BuildingExtra {
                charge: building.charge,
                max_charge: building.max_charge,
                cost: building.cost,
                hp: building.hp,
                max_hp: building.max_hp,
                money_inside: building.money_inside,
                crystals_inside: building.crystals_inside,
                items_inside: building.items_inside.clone(),
                craft_recipe_id: building.craft_recipe_id,
                craft_num: building.craft_num,
                craft_end_ts: building.craft_end_ts,
                craft_ready: building.craft_ready,
                clanzone: building.clanzone,
            };
            let data = serde_json::to_string(&extra)?;
            let type_code = building.type_code.chars().next().map_or(' ', |c| c);
            let building_result = sqlx::query(
                "UPDATE buildings SET type_code=?1, x=?2, y=?3, owner_id=?4, clan_id=?5, data=?6 WHERE id=?7",
            )
            .bind(type_code.to_string())
            .bind(building.x)
            .bind(building.y)
            .bind(building.owner_id)
            .bind(building.clan_id)
            .bind(data)
            .bind(building.id)
            .execute(&mut *tx)
            .await?;
            if building_result.rows_affected() != 1 {
                anyhow::bail!(
                    "building id={}: save affected {} rows",
                    building.id,
                    building_result.rows_affected()
                );
            }
        }
        tx.commit().await?;
        Ok(())
    }
}
