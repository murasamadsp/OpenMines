use super::Database;
use anyhow::Result;
use sqlx::Row;
use std::collections::{HashSet, VecDeque};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoxWrite {
    pub x: i32,
    pub y: i32,
    pub crystals: Option<[i64; 6]>,
}

/// Как `FindEmptyForBox`/смежный выбор в референсе: подобрать координату рядом (BFS).
pub fn pick_box_coord<FValid, FEmpty>(
    x: i32,
    y: i32,
    valid: FValid,
    is_empty: FEmpty,
) -> Option<(i32, i32)>
where
    FValid: Fn(i32, i32) -> bool,
    FEmpty: Fn(i32, i32) -> bool,
{
    if valid(x, y) && is_empty(x, y) {
        return Some((x, y));
    }

    let dirs = [(0, 1), (1, 0), (-1, 0), (0, -1)];
    let mut q = VecDeque::new();
    let mut visited = HashSet::new();

    q.push_back((x, y));
    visited.insert((x, y));

    let mut iterations = 0;
    while let Some((cx, cy)) = q.pop_front() {
        iterations += 1;
        if iterations > 100 {
            break;
        }

        for (dx, dy) in dirs {
            let nx = cx + dx;
            let ny = cy + dy;

            if !valid(nx, ny) {
                continue;
            }

            if is_empty(nx, ny) {
                return Some((nx, ny));
            }

            if visited.insert((nx, ny)) {
                q.push_back((nx, ny));
            }
        }
    }

    valid(x, y).then_some((x, y))
}

impl Database {
    /// Загрузить ВСЕ боксы (один раз на старте → in-memory `box_index`).
    pub async fn load_all_boxes(&self) -> Result<Vec<(i32, i32, [i64; 6])>> {
        let rows = sqlx::query("SELECT x, y, ze, cr, si, be, fi, go FROM boxes")
            .fetch_all(&self.pool)
            .await?;
        let box_rows = rows
            .into_iter()
            .map(|r| {
                (
                    r.get::<i32, _>("x"),
                    r.get::<i32, _>("y"),
                    [
                        r.get::<i64, _>("ze"),
                        r.get::<i64, _>("cr"),
                        r.get::<i64, _>("si"),
                        r.get::<i64, _>("be"),
                        r.get::<i64, _>("fi"),
                        r.get::<i64, _>("go"),
                    ],
                )
            })
            .collect();
        Ok(box_rows)
    }

    pub async fn upsert_box(&self, x: i32, y: i32, crystals: &[i64; 6]) -> Result<()> {
        sqlx::query(
            "INSERT INTO boxes (x, y, ze, cr, si, be, fi, go, cry_green, cry_blue, cry_red, cry_violet, cry_white, cry_cyan)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(x,y) DO UPDATE SET
               ze=excluded.ze,
               cr=excluded.cr,
               si=excluded.si,
               be=excluded.be,
               fi=excluded.fi,
               go=excluded.go,
               cry_green=excluded.cry_green,
               cry_blue=excluded.cry_blue,
               cry_red=excluded.cry_red,
               cry_violet=excluded.cry_violet,
               cry_white=excluded.cry_white,
               cry_cyan=excluded.cry_cyan"
        )
        .bind(x)
        .bind(y)
        .bind(crystals[0])
        .bind(crystals[1])
        .bind(crystals[2])
        .bind(crystals[3])
        .bind(crystals[4])
        .bind(crystals[5])
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_box_at(&self, x: i32, y: i32) -> Result<()> {
        sqlx::query("DELETE FROM boxes WHERE x=?1 AND y=?2")
            .bind(x)
            .bind(y)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn save_boxes_batch(&self, writes: &[BoxWrite]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for write in writes {
            if let Some(crystals) = write.crystals {
                sqlx::query(
                    "INSERT INTO boxes (x, y, ze, cr, si, be, fi, go, cry_green, cry_blue, cry_red, cry_violet, cry_white, cry_cyan)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT(x,y) DO UPDATE SET ze=excluded.ze, cr=excluded.cr, si=excluded.si,
                     be=excluded.be, fi=excluded.fi, go=excluded.go, cry_green=excluded.cry_green,
                     cry_blue=excluded.cry_blue, cry_red=excluded.cry_red, cry_violet=excluded.cry_violet,
                     cry_white=excluded.cry_white, cry_cyan=excluded.cry_cyan",
                )
                .bind(write.x)
                .bind(write.y)
                .bind(crystals[0])
                .bind(crystals[1])
                .bind(crystals[2])
                .bind(crystals[3])
                .bind(crystals[4])
                .bind(crystals[5])
                .execute(&mut *tx)
                .await?;
            } else {
                sqlx::query("DELETE FROM boxes WHERE x=?1 AND y=?2")
                    .bind(write.x)
                    .bind(write.y)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    /// Снести все боксы (выпавшие кристаллы) — для полного регена мира: их позиции
    /// привязаны к старому рельефу. Возвращает число удалённых строк.
    pub async fn delete_all_boxes(&self) -> Result<u64> {
        let res = sqlx::query("DELETE FROM boxes").execute(&self.pool).await?;
        Ok(res.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn box_batch_applies_ordered_upserts_and_deletes_atomically() {
        let path = std::env::temp_dir().join(format!(
            "openmines_box_batch_{}_{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = Database::open(&path).await.unwrap();
        db.save_boxes_batch(&[
            BoxWrite {
                x: 1,
                y: 2,
                crystals: Some([1, 2, 3, 4, 5, 6]),
            },
            BoxWrite {
                x: 1,
                y: 2,
                crystals: None,
            },
            BoxWrite {
                x: 3,
                y: 4,
                crystals: Some([6, 5, 4, 3, 2, 1]),
            },
        ])
        .await
        .unwrap();

        assert_eq!(
            db.load_all_boxes().await.unwrap(),
            vec![(3, 4, [6, 5, 4, 3, 2, 1])]
        );
        drop(db);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }
}
