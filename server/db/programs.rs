use super::Database;
use anyhow::Result;
use rusqlite::{OptionalExtension, params};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ProgramRow {
    pub id: i32,
    pub player_id: i32,
    pub name: String,
    pub code: String,
}

impl Database {
    pub fn list_programs(&self, player_id: i32) -> Result<Vec<ProgramRow>> {
        let rows = self
            .conn
            .lock()
            .prepare("SELECT id, player_id, name, code FROM programs WHERE player_id = ?1")?
            .query_map(params![player_id], |r| {
                Ok(ProgramRow {
                    id: r.get(0)?,
                    player_id: r.get(1)?,
                    name: r.get(2)?,
                    code: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        Ok(rows)
    }

    pub fn get_program(&self, id: i32) -> Result<Option<ProgramRow>> {
        let row = self
            .conn
            .lock()
            .prepare("SELECT id, player_id, name, code FROM programs WHERE id = ?1")?
            .query_row(params![id], |r| {
                Ok(ProgramRow {
                    id: r.get(0)?,
                    player_id: r.get(1)?,
                    name: r.get(2)?,
                    code: r.get(3)?,
                })
            })
            .optional()?;
        Ok(row)
    }

    pub fn insert_program(&self, player_id: i32, name: &str, code: &str) -> Result<i32> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO programs (player_id, name, code) VALUES (?1, ?2, ?3)",
            params![player_id, name, code],
        )?;
        let id = conn.last_insert_rowid();
        drop(conn);
        Ok(i32::try_from(id)?)
    }

    pub fn update_program(&self, id: i32, code: &str) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE programs SET code = ?1 WHERE id = ?2",
            params![code, id],
        )?;
        Ok(())
    }

    pub fn rename_program(&self, id: i32, new_name: &str) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE programs SET name = ?1 WHERE id = ?2",
            params![new_name, id],
        )?;
        Ok(())
    }

    pub fn delete_program(&self, id: i32) -> Result<()> {
        self.conn
            .lock()
            .execute("DELETE FROM programs WHERE id = ?1", params![id])?;
        Ok(())
    }
}
