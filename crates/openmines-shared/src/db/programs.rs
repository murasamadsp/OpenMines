use super::Database;
use anyhow::{Result, bail};
use sqlx::Row;

#[derive(Debug, Clone)]
pub struct ProgramRow {
    pub id: i32,
    pub player_id: i32,
    pub name: String,
    pub code: String,
}

impl Database {
    pub async fn list_programs(&self, player_id: i32) -> Result<Vec<ProgramRow>> {
        let rows =
            sqlx::query("SELECT id, player_id, name, code FROM programs WHERE player_id = ?1")
                .bind(player_id)
                .fetch_all(&self.pool)
                .await?;
        let programs = rows
            .into_iter()
            .map(|r| ProgramRow {
                id: r.get("id"),
                player_id: r.get("player_id"),
                name: r.get("name"),
                code: r.get("code"),
            })
            .collect();
        Ok(programs)
    }

    pub async fn get_program(&self, id: i32) -> Result<Option<ProgramRow>> {
        let row = sqlx::query("SELECT id, player_id, name, code FROM programs WHERE id = ?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        let program = row.map(|r| ProgramRow {
            id: r.get("id"),
            player_id: r.get("player_id"),
            name: r.get("name"),
            code: r.get("code"),
        });
        Ok(program)
    }

    pub async fn insert_program(&self, player_id: i32, name: &str, code: &str) -> Result<i32> {
        let result =
            sqlx::query("INSERT INTO programs (player_id, name, code) VALUES (?1, ?2, ?3)")
                .bind(player_id)
                .bind(name)
                .bind(code)
                .execute(&self.pool)
                .await?;
        let id = result.last_insert_rowid();
        Ok(i32::try_from(id)?)
    }

    pub async fn update_program(&self, id: i32, code: &str) -> Result<()> {
        let result = sqlx::query("UPDATE programs SET code = ?1 WHERE id = ?2")
            .bind(code)
            .bind(id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() != 1 {
            bail!(
                "program id={id}: update affected {} rows",
                result.rows_affected()
            );
        }
        Ok(())
    }

    pub async fn rename_program(&self, id: i32, new_name: &str) -> Result<()> {
        let result = sqlx::query("UPDATE programs SET name = ?1 WHERE id = ?2")
            .bind(new_name)
            .bind(id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() != 1 {
            bail!(
                "program id={id}: rename affected {} rows",
                result.rows_affected()
            );
        }
        Ok(())
    }

    pub async fn delete_program(&self, id: i32) -> Result<()> {
        let result = sqlx::query("DELETE FROM programs WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() != 1 {
            bail!(
                "program id={id}: delete affected {} rows",
                result.rows_affected()
            );
        }
        Ok(())
    }

    /// Save program source for an existing owned program.
    pub async fn save_program(&self, player_id: i32, prog_id: i32, code: &str) -> Result<()> {
        if prog_id <= 0 {
            bail!("program id={prog_id}: invalid client program id");
        }
        let result = sqlx::query("UPDATE programs SET code = ?1 WHERE id = ?2 AND player_id = ?3")
            .bind(code)
            .bind(prog_id)
            .bind(player_id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            bail!("program id={prog_id}: save affected 0 rows");
        }
        Ok(())
    }

    /// Delete program owned by player.
    pub async fn delete_program_owned(&self, player_id: i32, prog_id: i32) -> Result<bool> {
        let result = sqlx::query("DELETE FROM programs WHERE id = ?1 AND player_id = ?2")
            .bind(prog_id)
            .bind(player_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() == 1)
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
    async fn update_rename_delete_reject_missing_program() {
        let database = temp_database("program_missing").await;

        let update_err = database.update_program(999_999, "code").await.unwrap_err();
        let rename_err = database.rename_program(999_999, "name").await.unwrap_err();
        let delete_err = database.delete_program(999_999).await.unwrap_err();

        assert!(update_err.to_string().contains("update affected 0 rows"));
        assert!(rename_err.to_string().contains("rename affected 0 rows"));
        assert!(delete_err.to_string().contains("delete affected 0 rows"));
    }

    #[tokio::test]
    async fn save_program_rejects_missing_or_foreign_existing_id() {
        let database = temp_database("program_foreign_save").await;
        let owner = database
            .create_player("program-owner", "p", "h1")
            .await
            .unwrap();
        let foreign = database
            .create_player("program-foreign", "p", "h2")
            .await
            .unwrap();
        let program_id = database
            .insert_program(owner.id, "main", "old")
            .await
            .unwrap();

        let err = database
            .save_program(foreign.id, program_id, "new")
            .await
            .unwrap_err();

        assert!(err.to_string().contains("save affected 0 rows"));
        let saved = database.get_program(program_id).await.unwrap().unwrap();
        assert_eq!(saved.player_id, owner.id);
        assert_eq!(saved.code, "old");

        let missing_id = program_id + 10_000;
        let missing_err = database
            .save_program(owner.id, missing_id, "created-from-prog")
            .await
            .unwrap_err();
        assert!(missing_err.to_string().contains("save affected 0 rows"));
        assert!(database.get_program(missing_id).await.unwrap().is_none());
    }
}
