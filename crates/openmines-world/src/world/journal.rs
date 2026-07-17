use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::io::{Read as _, Seek as _, Write as _};
use std::path::{Path, PathBuf};

use super::super::{JOURNAL_MAGIC, JOURNAL_RECORD_LEN};

#[derive(Debug, Clone, Copy)]
pub struct JournalRecord {
    pub x: i32,
    pub y: i32,
    pub foreground: u8,
    pub road: u8,
    pub durability: f32,
}

pub struct WorldJournal {
    pub path: PathBuf,
    pub file: std::fs::File,
}

impl WorldJournal {
    pub fn open(path: PathBuf) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(&path)
            .with_context(|| format!("open world journal {}", path.display()))?;
        Ok(Self { path, file })
    }

    pub fn read_records(path: &Path) -> Result<Vec<JournalRecord>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut bytes = Vec::new();
        OpenOptions::new()
            .read(true)
            .open(path)
            .with_context(|| format!("read world journal {}", path.display()))?
            .read_to_end(&mut bytes)?;

        let full_len = bytes.len() - (bytes.len() % JOURNAL_RECORD_LEN);
        let mut records = Vec::with_capacity(full_len / JOURNAL_RECORD_LEN);
        for chunk in bytes[..full_len].chunks_exact(JOURNAL_RECORD_LEN) {
            if chunk[0..4] != JOURNAL_MAGIC {
                anyhow::bail!("corrupt world journal {}: bad magic", path.display());
            }
            records.push(JournalRecord {
                x: i32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]),
                y: i32::from_le_bytes([chunk[8], chunk[9], chunk[10], chunk[11]]),
                foreground: chunk[12],
                road: chunk[13],
                durability: f32::from_le_bytes([chunk[14], chunk[15], chunk[16], chunk[17]]),
            });
        }
        Ok(records)
    }

    pub fn append(&mut self, record: JournalRecord) -> Result<()> {
        let mut bytes = [0u8; JOURNAL_RECORD_LEN];
        bytes[0..4].copy_from_slice(&JOURNAL_MAGIC);
        bytes[4..8].copy_from_slice(&record.x.to_le_bytes());
        bytes[8..12].copy_from_slice(&record.y.to_le_bytes());
        bytes[12] = record.foreground;
        bytes[13] = record.road;
        bytes[14..18].copy_from_slice(&record.durability.to_le_bytes());
        self.file.write_all(&bytes)?;
        Ok(())
    }

    pub fn checkpoint(&mut self) -> Result<()> {
        self.file.set_len(0)?;
        self.file.rewind()?;
        tracing::debug!(path = %self.path.display(), "World journal checkpointed");
        Ok(())
    }
}
