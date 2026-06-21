use super::Database;
use anyhow::Result;
use sqlx::Row;

#[allow(dead_code)]
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
        sqlx::query("UPDATE programs SET code = ?1 WHERE id = ?2")
            .bind(code)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn rename_program(&self, id: i32, new_name: &str) -> Result<()> {
        sqlx::query("UPDATE programs SET name = ?1 WHERE id = ?2")
            .bind(new_name)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_program(&self, id: i32) -> Result<()> {
        sqlx::query("DELETE FROM programs WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Save (upsert) program: update code if program exists, otherwise insert.
    pub async fn save_program(&self, player_id: i32, prog_id: i32, code: &str) -> Result<()> {
        let result = sqlx::query("UPDATE programs SET code = ?1 WHERE id = ?2 AND player_id = ?3")
            .bind(code)
            .bind(prog_id)
            .bind(player_id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            sqlx::query("INSERT OR IGNORE INTO programs (id, player_id, name, code) VALUES (?1, ?2, ?3, ?4)")
                .bind(prog_id)
                .bind(player_id)
                .bind("program")
                .bind(code)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    /// Delete program owned by player.
    pub async fn delete_program_owned(&self, player_id: i32, prog_id: i32) -> Result<()> {
        sqlx::query("DELETE FROM programs WHERE id = ?1 AND player_id = ?2")
            .bind(prog_id)
            .bind(player_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
