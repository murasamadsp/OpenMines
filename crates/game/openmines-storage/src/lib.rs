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
