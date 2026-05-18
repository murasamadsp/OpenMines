use super::Database;
use anyhow::Result;
use rusqlite::params;

use std::collections::{HashSet, VecDeque};

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

    // C# FindEmptyForBox searches until it finds an empty spot.
    // We add a safety limit of 100 iterations just to prevent infinite loops in weird world edge cases.
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
    /// На hot-path `SQLite` по боксам больше не дёргаем (был фриз: sync `SQLite`
    /// под `ecs.write()` в `standing_cell_hazard_system` каждые 10ms).
    pub fn load_all_boxes(&self) -> Result<Vec<(i32, i32, [i64; 6])>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT x, y, ze, cr, si, be, fi, go FROM boxes")?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i32>(0)?,
                    r.get::<_, i32>(1)?,
                    [
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                    ],
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);
        drop(conn);
        Ok(rows)
    }

    pub fn upsert_box(&self, x: i32, y: i32, crystals: &[i64; 6]) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
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
               cry_cyan=excluded.cry_cyan",
            params![
                x,
                y,
                crystals[0],
                crystals[1],
                crystals[2],
                crystals[3],
                crystals[4],
                crystals[5],
            ],
        )?;
        drop(conn);
        Ok(())
    }

    pub fn delete_box_at(&self, x: i32, y: i32) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM boxes WHERE x=?1 AND y=?2", params![x, y])?;
        drop(conn);
        Ok(())
    }
}
