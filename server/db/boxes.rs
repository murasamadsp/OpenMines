use super::Database;
use anyhow::Result;
use rusqlite::{OptionalExtension, params};

#[derive(Debug, Clone)]
pub struct BoxRow {
    pub x: i32,
    pub y: i32,
    pub crystals: [i64; 6],
}

const OFFSETS: [(i32, i32); 9] = [
    (0, 0),
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

/// Как `FindEmptyForBox`/смежный выбор в референсе: подобрать координату рядом.
pub fn pick_box_coord<FValid, FEmpty>(x: i32, y: i32, valid: FValid, is_empty: FEmpty) -> Option<(i32, i32)>
where
    FValid: Fn(i32, i32) -> bool,
    FEmpty: Fn(i32, i32) -> bool,
{
    for (dx, dy) in OFFSETS {
        let bx = x + dx;
        let by = y + dy;
        if valid(bx, by) && is_empty(bx, by) {
            return Some((bx, by));
        }
    }
    valid(x, y).then_some((x, y))
}

impl Database {
    pub fn get_box_at(&self, x: i32, y: i32) -> Result<Option<BoxRow>> {
        let conn = self.conn.lock();
        let row = conn
            .prepare(
                "SELECT x, y, ze, cr, si, be, fi, go FROM boxes WHERE x=?1 AND y=?2",
            )?
            .query_row(params![x, y], |r| {
                Ok(BoxRow {
                    x: r.get(0)?,
                    y: r.get(1)?,
                    crystals: [
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                    ],
                })
            })
            .optional()?;
        Ok(row)
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
        Ok(())
    }

    pub fn delete_box_at(&self, x: i32, y: i32) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM boxes WHERE x=?1 AND y=?2", params![x, y])?;
        Ok(())
    }
}

