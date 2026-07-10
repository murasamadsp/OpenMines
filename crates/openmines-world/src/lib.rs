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

pub mod anl;
pub mod cells;
pub use cells::CellType;
pub mod world_cell;
pub use world_cell::WorldCell;
pub mod generator;
pub mod map_format;
mod sector_palette;
mod sectors_gen;

use anyhow::{Context, Result};
use cells::{CellDefs, cell_type};
use map_format::MapStore;
use memmap2::MmapMut;
use parking_lot::{Mutex, RwLock};
use std::fs::{self, OpenOptions};
use std::io::{Read as _, Seek as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const CHUNK_SIZE: u32 = 32;
const JOURNAL_MAGIC: [u8; 4] = *b"OWJ1";
const JOURNAL_RECORD_LEN: usize = 18;

#[derive(Debug, Clone, Copy)]
struct JournalRecord {
    x: i32,
    y: i32,
    foreground: u8,
    road: u8,
    durability: f32,
}

struct WorldJournal {
    path: PathBuf,
    file: std::fs::File,
}

impl WorldJournal {
    fn open(path: PathBuf) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(&path)
            .with_context(|| format!("open world journal {}", path.display()))?;
        Ok(Self { path, file })
    }

    fn read_records(path: &Path) -> Result<Vec<JournalRecord>> {
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

    fn append(&mut self, record: JournalRecord) -> Result<()> {
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

    fn checkpoint(&mut self) -> Result<()> {
        self.file.set_len(0)?;
        self.file.rewind()?;
        tracing::debug!(path = %self.path.display(), "World journal checkpointed");
        Ok(())
    }
}

/// Поддерживаемые типы данных в слоях карты.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerType {
    /// Единственный mmap-слой — durability (f32 на клетку). Клетки теперь
    /// хранятся в клиентском `.map` (см. [`map_format`]), не в старом raw-слое.
    F32,
}

impl LayerType {
    const fn size(self) -> usize {
        match self {
            Self::F32 => 4,
        }
    }
}

/// Универсальный слой карты на базе mmap.
pub struct Layer {
    pub mmap: MmapMut,
    path: PathBuf,
    chunks_h: u32,
    data_type: LayerType,
    /// Маска "грязных" чанков для оптимизации синхронизации и сохранений.
    dirty_mask: Vec<bool>,
    /// Только реально изменённые chunk indexes. Нужен для O(dirty), а не
    /// O(world area), flush и no-dirty fast path.
    dirty_indices: Vec<usize>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LayerFlushStats {
    pub dirty_chunks: usize,
    pub ranges: usize,
    pub bytes: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WorldFlushStats {
    pub durability: LayerFlushStats,
}

impl Layer {
    pub fn open(path: PathBuf, chunks_w: u32, chunks_h: u32, data_type: LayerType) -> Result<Self> {
        let cells_per_chunk = u64::from(CHUNK_SIZE * CHUNK_SIZE);
        let total_bytes =
            u64::from(chunks_w) * u64::from(chunks_h) * cells_per_chunk * (data_type.size() as u64);

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("Failed to open layer file: {}", path.display()))?;

        if file.metadata()?.len() < total_bytes {
            file.set_len(total_bytes)?;
        }

        // SAFETY: Отображение файла в память (memory mapping) безопасно, так как:
        // 1. Длина файла принудительно устанавливается равной `total_bytes` с помощью `file.set_len`,
        //    что гарантирует корректность границ выделенной области памяти и предотвращает SIGBUS при чтении/записи.
        // 2. Файл карты {name}_durability.map является приватным для данного сервера и не модифицируется
        //    параллельно другими процессами или потоками вне логики этого приложения, что гарантирует соблюдение
        //    правил алиасинга Rust.
        let mmap = unsafe { MmapMut::map_mut(&file)? };
        let dirty_count = (chunks_w * chunks_h) as usize;

        Ok(Self {
            mmap,
            path,
            chunks_h,
            data_type,
            dirty_mask: vec![false; dirty_count],
            dirty_indices: Vec::new(),
        })
    }

    #[inline]
    pub const fn cell_offset(&self, x: u32, y: u32) -> usize {
        let cx = x / CHUNK_SIZE;
        let cy = y / CHUNK_SIZE;
        let lx = x % CHUNK_SIZE;
        let ly = y % CHUNK_SIZE;
        let chunk_idx = (cy + self.chunks_h * cx) as usize;
        let cell_in_chunk = (ly + CHUNK_SIZE * lx) as usize;
        let chunk_start = chunk_idx * (CHUNK_SIZE * CHUNK_SIZE) as usize;
        (chunk_start + cell_in_chunk) * self.data_type.size()
    }

    #[inline]
    pub fn mark_dirty(&mut self, x: u32, y: u32) {
        let cx = x / CHUNK_SIZE;
        let cy = y / CHUNK_SIZE;
        let idx = (cy + self.chunks_h * cx) as usize;
        if let Some(v) = self.dirty_mask.get_mut(idx)
            && !*v
        {
            *v = true;
            self.dirty_indices.push(idx);
        }
    }

    fn mark_all_dirty(&mut self) {
        self.dirty_mask.fill(true);
        self.dirty_indices.clear();
        self.dirty_indices.extend(0..self.dirty_mask.len());
    }

    /// Под write-локом слоя: `msync` только contiguous dirty chunk ranges.
    /// Дорогой full-file `.bak` копируется ВНЕ лока (см. `World::flush`),
    /// иначе `fs::copy` ~ГБ держит write-лок секунды и фризит весь сервер.
    pub fn msync_dirty_and_clear(&mut self) -> Result<LayerFlushStats> {
        if self.dirty_indices.is_empty() {
            return Ok(LayerFlushStats::default());
        }

        self.dirty_indices.sort_unstable();
        let chunk_bytes = (CHUNK_SIZE * CHUNK_SIZE) as usize * self.data_type.size();
        let mut ranges = Vec::new();
        let mut start = self.dirty_indices[0];
        let mut end = start + 1;
        for &idx in self.dirty_indices.iter().skip(1) {
            if idx == end {
                end += 1;
            } else {
                ranges.push((start, end));
                start = idx;
                end = idx + 1;
            }
        }
        ranges.push((start, end));

        let m0 = std::time::Instant::now();
        for &(range_start, range_end) in &ranges {
            let offset = range_start * chunk_bytes;
            let len = (range_end - range_start) * chunk_bytes;
            self.mmap.flush_range(offset, len)?;
        }
        let el = m0.elapsed();
        let stats = LayerFlushStats {
            dirty_chunks: self.dirty_indices.len(),
            ranges: ranges.len(),
            bytes: self.dirty_indices.len().saturating_mul(chunk_bytes),
        };
        if el > std::time::Duration::from_millis(50) {
            tracing::warn!(
                target: "tickprof",
                path = ?self.path.file_name().unwrap_or_default(),
                dirty_chunks = stats.dirty_chunks,
                ranges = stats.ranges,
                bytes = stats.bytes,
                elapsed = ?el,
                "LAYER dirty-range msync slow (UNDER write lock)"
            );
        }
        for idx in self.dirty_indices.drain(..) {
            self.dirty_mask[idx] = false;
        }
        Ok(stats)
    }

    /// Путь файла слоя (для бэкапа вне лока).
    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub trait WorldProvider: Send + Sync {
    fn name(&self) -> &str;
    fn chunks_w(&self) -> u32;
    fn chunks_h(&self) -> u32;
    fn cell_defs(&self) -> Arc<CellDefs>;

    fn cells_width(&self) -> u32;
    fn cells_height(&self) -> u32;
    fn valid_coord(&self, x: i32, y: i32) -> bool;
    fn get_cell(&self, x: i32, y: i32) -> u8;
    fn get_cell_typed(&self, x: i32, y: i32) -> CellType;
    fn snapshot_cells_rect(
        &self,
        min_x: i32,
        min_y: i32,
        width: usize,
        height: usize,
    ) -> Vec<Option<CellType>>;
    fn get_solid_cell(&self, x: i32, y: i32) -> u8;
    fn get_road_cell(&self, x: i32, y: i32) -> u8;
    fn set_cell(&self, x: i32, y: i32, cell: u8);
    fn set_cell_typed(&self, x: i32, y: i32, cell: CellType);
    fn get_durability(&self, x: i32, y: i32) -> f32;
    fn set_durability(&self, x: i32, y: i32, d: f32);
    fn read_world_cell(&self, x: i32, y: i32) -> Option<WorldCell>;
    fn write_world_cell(&self, x: i32, y: i32, cell: WorldCell);
    fn destroy(&self, x: i32, y: i32);
    fn damage_cell(&self, x: i32, y: i32, dmg: f32) -> bool;
    fn read_chunk_cells(&self, chunk_x: u32, chunk_y: u32) -> Vec<u8>;
    fn flush(&self) -> anyhow::Result<WorldFlushStats>;
    fn is_empty(&self, x: i32, y: i32) -> bool;
}

pub struct World {
    pub name: String,
    pub chunks_w: u32,
    pub chunks_h: u32,
    /// Клетки в клиент-совместимом формате `.map` (см. [`map_format`] /
    /// `client/Assets/Scripts/MapModel.cs`). Foreground/solid слой.
    cells: RwLock<MapStore>,
    /// Background/road слой. Соответствует C# `World.road`: если foreground
    /// пустой, клиент видит этот байт.
    road: RwLock<MapStore>,
    /// Серверная прочность клеток (`damage_cell`). У клиента понятия
    /// durability нет — это серверное состояние, отдельный mmap f32-слой.
    durability: RwLock<Layer>,
    /// Путь `{name}_v2.map` для инкрементального сохранения (как `MapModel`).
    map_path: PathBuf,
    /// Путь `{name}_road_v2.map` для background/road слоя.
    road_path: PathBuf,
    /// Append-only crash recovery log for world mutations after the last checkpoint.
    journal: Mutex<WorldJournal>,
    pub cell_defs: Arc<CellDefs>,
    /// Счётчик вызовов flush: дорогой `.bak` делаем не каждый flush,
    /// а раз в `BACKUP_EVERY_N_FLUSHES` (msync/save — каждый flush).
    flush_count: std::sync::atomic::AtomicU64,
}

/// Значение пустой/выкопанной клетки (как видит клиент по проводу).
const EMPTY_CELL: u8 = cell_type::EMPTY;

/// При 60s-цикле flush: бэкап ≈ раз в 30 мин (msync остаётся каждые 60s).
const BACKUP_EVERY_N_FLUSHES: u64 = 30;

impl WorldProvider for World {
    #[inline]
    fn name(&self) -> &str {
        &self.name
    }
    #[inline]
    fn chunks_w(&self) -> u32 {
        self.chunks_w
    }
    #[inline]
    fn chunks_h(&self) -> u32 {
        self.chunks_h
    }
    #[inline]
    fn cell_defs(&self) -> Arc<CellDefs> {
        self.cell_defs.clone()
    }
    #[inline]
    fn cells_width(&self) -> u32 {
        self.chunks_w * CHUNK_SIZE
    }
    #[inline]
    fn cells_height(&self) -> u32 {
        self.chunks_h * CHUNK_SIZE
    }

    #[inline]
    fn valid_coord(&self, x: i32, y: i32) -> bool {
        x >= 0
            && y >= 0
            && x.cast_unsigned() < self.cells_width()
            && y.cast_unsigned() < self.cells_height()
    }

    fn get_cell(&self, x: i32, y: i32) -> u8 {
        if !self.valid_coord(x, y) {
            return 0;
        }
        let b = self.cells.read().get_cell(x, y);
        if b != 0 {
            return b;
        }
        let r = self.road.read().get_cell(x, y);
        if r == 0 { EMPTY_CELL } else { r }
    }

    fn get_cell_typed(&self, x: i32, y: i32) -> CellType {
        CellType(self.get_cell(x, y))
    }

    fn snapshot_cells_rect(
        &self,
        min_x: i32,
        min_y: i32,
        width: usize,
        height: usize,
    ) -> Vec<Option<CellType>> {
        let cells = self.cells.read();
        let road = self.road.read();
        let mut out = Vec::with_capacity(width.saturating_mul(height));
        for row in 0..height {
            let Some(y) = i32::try_from(row)
                .ok()
                .and_then(|row| min_y.checked_add(row))
            else {
                out.extend(std::iter::repeat_n(None, width));
                continue;
            };
            for col in 0..width {
                let Some(x) = i32::try_from(col)
                    .ok()
                    .and_then(|col| min_x.checked_add(col))
                else {
                    out.push(None);
                    continue;
                };
                if !self.valid_coord(x, y) {
                    out.push(None);
                    continue;
                }
                let foreground = cells.get_cell(x, y);
                let cell = if foreground != 0 {
                    foreground
                } else {
                    let road_cell = road.get_cell(x, y);
                    if road_cell == 0 {
                        EMPTY_CELL
                    } else {
                        road_cell
                    }
                };
                out.push(Some(CellType(cell)));
            }
        }
        drop(road);
        drop(cells);
        out
    }

    fn get_solid_cell(&self, x: i32, y: i32) -> u8 {
        if !self.valid_coord(x, y) {
            return 0;
        }
        self.cells.read().get_cell(x, y)
    }

    fn get_road_cell(&self, x: i32, y: i32) -> u8 {
        if !self.valid_coord(x, y) || self.get_solid_cell(x, y) != 0 {
            return 0;
        }
        let r = self.road.read().get_cell(x, y);
        if r == 0 { EMPTY_CELL } else { r }
    }

    fn set_cell(&self, x: i32, y: i32, cell: u8) {
        self.set_cell_typed(x, y, CellType(cell));
    }

    fn set_cell_typed(&self, x: i32, y: i32, cell_type: CellType) {
        if !self.valid_coord(x, y) {
            return;
        }
        let prop = self.cell_defs.get_typed(cell_type);
        let durability = if prop.cell_is_empty() {
            0.0f32
        } else {
            prop.durability
        };
        self.write_world_cell(
            x,
            y,
            WorldCell {
                cell_type,
                durability,
            },
        );
    }

    fn get_durability(&self, x: i32, y: i32) -> f32 {
        if !self.valid_coord(x, y) {
            return 0.0;
        }
        let layer = self.durability.read();
        let off = layer.cell_offset(x.cast_unsigned(), y.cast_unsigned());
        let val = f32::from_le_bytes([
            layer.mmap[off],
            layer.mmap[off + 1],
            layer.mmap[off + 2],
            layer.mmap[off + 3],
        ]);
        drop(layer);
        val
    }

    fn set_durability(&self, x: i32, y: i32, d: f32) {
        if !self.valid_coord(x, y) {
            return;
        }
        let mut journal = self.journal.lock();
        let foreground = self.cells.read().get_cell(x, y);
        let road = self.road.read().get_cell(x, y);
        if let Err(err) = journal.append(JournalRecord {
            x,
            y,
            foreground,
            road,
            durability: d,
        }) {
            tracing::error!(x, y, error = ?err, "World journal append failed; durability mutation skipped");
            return;
        }
        let mut layer = self.durability.write();
        let off = layer.cell_offset(x.cast_unsigned(), y.cast_unsigned());
        layer.mmap[off..off + 4].copy_from_slice(&d.to_le_bytes());
        layer.mark_dirty(x.cast_unsigned(), y.cast_unsigned());
        drop(layer);
        drop(journal);
    }

    fn read_world_cell(&self, x: i32, y: i32) -> Option<WorldCell> {
        if !self.valid_coord(x, y) {
            return None;
        }
        let cell_type = CellType(self.get_cell(x, y));
        let durability = self.get_durability(x, y);
        Some(WorldCell {
            cell_type,
            durability,
        })
    }

    fn write_world_cell(&self, x: i32, y: i32, cell: WorldCell) {
        if !self.valid_coord(x, y) {
            return;
        }
        let mut journal = self.journal.lock();
        let (ux, uy) = (x.cast_unsigned(), y.cast_unsigned());
        let is_empty = self.cell_defs.get_typed(cell.cell_type).cell_is_empty();
        let foreground = if is_empty { 0 } else { cell.cell_type.0 };
        let road = if is_empty {
            cell.cell_type.0
        } else {
            self.road.read().get_cell(x, y)
        };
        if let Err(err) = journal.append(JournalRecord {
            x,
            y,
            foreground,
            road,
            durability: cell.durability,
        }) {
            tracing::error!(x, y, error = ?err, "World journal append failed; cell mutation skipped");
            return;
        }
        if is_empty {
            self.cells.write().set_cell(x, y, 0);
            self.road.write().set_cell(x, y, cell.cell_type.0);
        } else {
            self.cells.write().set_cell(x, y, cell.cell_type.0);
        }
        let mut layer = self.durability.write();
        let off = layer.cell_offset(ux, uy);
        layer.mmap[off..off + 4].copy_from_slice(&cell.durability.to_le_bytes());
        layer.mark_dirty(ux, uy);
        drop(layer);
        drop(journal);
    }

    fn destroy(&self, x: i32, y: i32) {
        if !self.valid_coord(x, y) {
            return;
        }
        let mut journal = self.journal.lock();
        if self.is_empty(x, y) {
            return;
        }
        // Выкопанная клетка = EMPTY (как видит клиент по проводу); dur → 0.
        let r = self.road.read().get_cell(x, y);
        let road = if r == 0 { EMPTY_CELL } else { r };
        if let Err(err) = journal.append(JournalRecord {
            x,
            y,
            foreground: 0,
            road,
            durability: 0.0,
        }) {
            tracing::error!(x, y, error = ?err, "World journal append failed; destroy skipped");
            return;
        }
        self.cells.write().set_cell(x, y, 0);
        if road != r {
            self.road.write().set_cell(x, y, EMPTY_CELL);
        }
        self.set_durability_direct(x, y, 0.0);
        drop(journal);
    }

    fn damage_cell(&self, x: i32, y: i32, dmg: f32) -> bool {
        if !self.valid_coord(x, y) {
            return false;
        }
        let mut journal = self.journal.lock();
        let (ux, uy) = (x.cast_unsigned(), y.cast_unsigned());
        let (destroyed, new_durability) = {
            let layer = self.durability.read();
            let off = layer.cell_offset(ux, uy);
            let d = f32::from_le_bytes([
                layer.mmap[off],
                layer.mmap[off + 1],
                layer.mmap[off + 2],
                layer.mmap[off + 3],
            ]);
            drop(layer);
            if d - dmg <= 0.0 {
                (true, 0.0)
            } else {
                (false, d - dmg)
            }
        };
        let foreground = if destroyed {
            0
        } else {
            self.cells.read().get_cell(x, y)
        };
        let raw_road = self.road.read().get_cell(x, y);
        let road = if destroyed && raw_road == 0 {
            EMPTY_CELL
        } else {
            raw_road
        };
        if let Err(err) = journal.append(JournalRecord {
            x,
            y,
            foreground,
            road,
            durability: new_durability,
        }) {
            tracing::error!(x, y, error = ?err, "World journal append failed; damage skipped");
            return false;
        }
        if destroyed {
            self.cells.write().set_cell(x, y, 0);
            if road != raw_road {
                self.road.write().set_cell(x, y, EMPTY_CELL);
            }
            self.set_durability_direct(x, y, 0.0);
        } else {
            self.set_durability_direct(x, y, new_durability);
        }
        drop(journal);
        destroyed
    }

    fn read_chunk_cells(&self, chunk_x: u32, chunk_y: u32) -> Vec<u8> {
        let n = (CHUNK_SIZE * CHUNK_SIZE) as usize;
        if chunk_x >= self.chunks_w || chunk_y >= self.chunks_h {
            return vec![0u8; n];
        }
        let base_x = chunk_x * CHUNK_SIZE;
        let base_y = chunk_y * CHUNK_SIZE;
        let mut res = Vec::with_capacity(n);
        {
            let cells = self.cells.read();
            let road = self.road.read();
            // Порядок байт HB 'M' = как кэширует клиент (`MapBlock.data`,
            // индекс `x + 32*y`): for y:0..32 { for x:0..32 }.
            for y in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    let wx = (base_x + x).cast_signed();
                    let wy = (base_y + y).cast_signed();
                    let b = cells.get_cell(wx, wy);
                    if b != 0 {
                        res.push(b);
                    } else {
                        let r = road.get_cell(wx, wy);
                        res.push(if r == 0 { EMPTY_CELL } else { r });
                    }
                }
            }
            drop(road);
            drop(cells);
        }
        res
    }

    fn flush(&self) -> Result<WorldFlushStats> {
        let mut journal = self.journal.lock();
        let do_backup = {
            let n = self
                .flush_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            n != 0 && n.is_multiple_of(BACKUP_EVERY_N_FLUSHES)
        };

        // Клетки: инкрементальная запись `.map` (как `MapModel.SaveMapV2` —
        // только заголовок/индекс/грязные блоки, без перезаписи всего файла).
        {
            let mut store = self.cells.write();
            if store.is_dirty() {
                store.save(&self.map_path)?;
            }
        }
        {
            let mut store = self.road.write();
            if store.is_dirty() {
                store.save(&self.road_path)?;
            }
        }
        if do_backup && self.map_path.exists() {
            let bak = self.map_path.with_extension("map.bak");
            let tmp = self.map_path.with_extension("map.tmp");
            let _ = fs::copy(&self.map_path, &tmp);
            let _ = fs::rename(&tmp, &bak);
        }
        if do_backup && self.road_path.exists() {
            let bak = self.road_path.with_extension("map.bak");
            let tmp = self.road_path.with_extension("map.tmp");
            let _ = fs::copy(&self.road_path, &tmp);
            let _ = fs::rename(&tmp, &bak);
        }

        // Durability: msync mmap-слоя под локом, бэкап вне лока.
        // Клонируем PathBuf только при do_backup (раз в 30 мин) — нельзя держать
        // ссылку &Path из Layer вне write-guard'а.
        let (dpath_for_backup, durability_stats): (Option<PathBuf>, LayerFlushStats) = {
            let mut l = self.durability.write();
            let backup_path = do_backup.then(|| l.path().to_owned());
            let stats = l.msync_dirty_and_clear()?;
            drop(l);
            (backup_path, stats)
        };
        if let Some(dpath) = dpath_for_backup
            && dpath.exists()
        {
            let bak = dpath.with_extension("map.bak");
            let tmp = dpath.with_extension("map.tmp");
            let _ = fs::copy(&dpath, &tmp);
            let _ = fs::rename(&tmp, &bak);
        }
        journal.checkpoint()?;
        drop(journal);
        Ok(WorldFlushStats {
            durability: durability_stats,
        })
    }

    fn is_empty(&self, x: i32, y: i32) -> bool {
        self.cell_defs.get(self.get_cell(x, y)).cell_is_empty()
    }
}

impl World {
    pub fn new(
        name: &str,
        chunks_w: u32,
        chunks_h: u32,
        cell_defs: CellDefs,
        state_dir: &Path,
    ) -> Result<Self> {
        let width = i32::try_from(chunks_w * CHUNK_SIZE)
            .map_err(|_| anyhow::anyhow!("world width overflows i32"))?;
        let height = i32::try_from(chunks_h * CHUNK_SIZE)
            .map_err(|_| anyhow::anyhow!("world height overflows i32"))?;

        let map_path = state_dir.join(format!("{name}_v2.map"));
        let road_path = state_dir.join(format!("{name}_road_v2.map"));
        let journal_path = state_dir.join(format!("{name}_world.journal"));
        let is_new = !map_path.exists();
        let needs_legacy_layer_split = map_path.exists() && !road_path.exists();
        let journal_records = WorldJournal::read_records(&journal_path)?;

        let cells = MapStore::open(&map_path, width, height)?;
        let road = MapStore::open(&road_path, width, height)?;
        tracing::info!(
            "Map store {}: {}x{}, {} blocks allocated",
            map_path.display(),
            cells.width(),
            cells.height(),
            cells.allocated_blocks()
        );
        let durability = Layer::open(
            state_dir.join(format!("{name}_durability.map")),
            chunks_w,
            chunks_h,
            LayerType::F32,
        )?;

        let world = Self {
            name: name.to_string(),
            chunks_w,
            chunks_h,
            cells: RwLock::new(cells),
            road: RwLock::new(road),
            durability: RwLock::new(durability),
            map_path,
            road_path,
            journal: Mutex::new(WorldJournal::open(journal_path)?),
            cell_defs: Arc::new(cell_defs),
            flush_count: std::sync::atomic::AtomicU64::new(0),
        };

        if is_new {
            tracing::info!("Initializing new world...");
            generator::generate(&world, 42);
            world.flush()?;
        } else if needs_legacy_layer_split {
            tracing::info!("Splitting legacy single-layer map into foreground/background layers");
            world.split_legacy_visible_layer();
            world.flush()?;
        }
        if !is_new && !journal_records.is_empty() {
            tracing::warn!(
                records = journal_records.len(),
                "Replaying world journal after uncheckpointed shutdown"
            );
            world.apply_journal_records(&journal_records);
            world.flush()?;
        }

        Ok(world)
    }

    fn set_durability_direct(&self, x: i32, y: i32, d: f32) {
        let mut layer = self.durability.write();
        let off = layer.cell_offset(x.cast_unsigned(), y.cast_unsigned());
        layer.mmap[off..off + 4].copy_from_slice(&d.to_le_bytes());
        layer.mark_dirty(x.cast_unsigned(), y.cast_unsigned());
    }

    fn apply_journal_records(&self, records: &[JournalRecord]) {
        for record in records {
            if !self.valid_coord(record.x, record.y) {
                tracing::warn!(
                    x = record.x,
                    y = record.y,
                    "Skipping out-of-bounds world journal record"
                );
                continue;
            }
            self.cells
                .write()
                .set_cell(record.x, record.y, record.foreground);
            self.road.write().set_cell(record.x, record.y, record.road);
            self.set_durability_direct(record.x, record.y, record.durability);
        }
    }

    /// Дать генератору mmap durability-слоя (u8-вид f32) под write-локом.
    pub(crate) fn with_durability_mmap<R>(&self, f: impl FnOnce(&mut [u8]) -> R) -> R {
        let mut l = self.durability.write();
        let result = f(&mut l.mmap[..]);
        l.mark_all_dirty();
        result
    }

    /// Залить сгенерированные клетки в `.map` за один write-лок. Плоский
    /// буфер индексируется как прежняя chunk-раскладка
    /// (`chunk = cy + chunks_h*cx`, `cell = ly + 32*lx`); `0` → `EMPTY`.
    pub(crate) fn ingest_generated_cells(&self, flat: &[u8]) {
        let cs = CHUNK_SIZE;
        let w = self.chunks_w * cs;
        let h = self.chunks_h * cs;
        {
            let mut store = self.cells.write();
            let mut road = self.road.write();
            for y in 0..h {
                for x in 0..w {
                    let chunk_idx = ((y / cs) + self.chunks_h * (x / cs)) as usize;
                    let cell_in_chunk = ((y % cs) + cs * (x % cs)) as usize;
                    let idx = chunk_idx * (cs * cs) as usize + cell_in_chunk;
                    let cell = flat[idx];
                    let (x, y) = (x.cast_signed(), y.cast_signed());
                    if cell == 0 || self.cell_defs.get(cell).cell_is_empty() {
                        store.set_cell(x, y, 0);
                        road.set_cell(x, y, if cell == 0 { EMPTY_CELL } else { cell });
                    } else {
                        store.set_cell(x, y, cell);
                    }
                }
            }
            drop(road);
            drop(store);
        }
    }

    fn split_legacy_visible_layer(&self) {
        let w = self.chunks_w * CHUNK_SIZE;
        let h = self.chunks_h * CHUNK_SIZE;
        {
            let mut cells = self.cells.write();
            let mut road = self.road.write();
            for y in 0..h {
                for x in 0..w {
                    let (x, y) = (x.cast_signed(), y.cast_signed());
                    let cell = cells.get_cell(x, y);
                    if cell != 0 && self.cell_defs.get(cell).cell_is_empty() {
                        cells.set_cell(x, y, 0);
                        road.set_cell(x, y, cell);
                    }
                }
            }
            drop(road);
            drop(cells);
        }
    }

    pub(crate) const fn chunks_layout(&self) -> (u32, u32, u32) {
        (self.chunks_w, self.chunks_h, CHUNK_SIZE)
    }

    pub fn chunk_pos(x: i32, y: i32) -> (u32, u32) {
        (
            x.max(0).cast_unsigned() / CHUNK_SIZE,
            y.max(0).cast_unsigned() / CHUNK_SIZE,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durability_flush_coalesces_only_dirty_chunk_ranges() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "durability_ranges_{}_{}.map",
            std::process::id(),
            nonce
        ));
        let mut layer = Layer::open(path.clone(), 4, 4, LayerType::F32).unwrap();

        layer.mark_dirty(0, 0);
        layer.mark_dirty(0, 32);
        layer.mark_dirty(64, 0);
        layer.mark_dirty(64, 0);
        let stats = layer.msync_dirty_and_clear().unwrap();

        assert_eq!(stats.dirty_chunks, 3);
        assert_eq!(stats.ranges, 2);
        assert_eq!(stats.bytes, 3 * 32 * 32 * 4);
        assert_eq!(
            layer.msync_dirty_and_clear().unwrap(),
            LayerFlushStats::default()
        );

        drop(layer);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    #[ignore = "release-only storage performance evidence"]
    fn durability_flush_large_world_profile() {
        const SAMPLES: usize = 100;
        const DIRTY_CHUNKS: u32 = 256;

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "durability_profile_{}_{}.map",
            std::process::id(),
            nonce
        ));
        let mut layer = Layer::open(path.clone(), 32, 563, LayerType::F32).unwrap();

        let mut no_dirty = Vec::with_capacity(SAMPLES);
        for _ in 0..SAMPLES {
            let started = std::time::Instant::now();
            assert_eq!(
                layer.msync_dirty_and_clear().unwrap(),
                LayerFlushStats::default()
            );
            no_dirty.push(started.elapsed());
        }

        let mut dirty = Vec::with_capacity(SAMPLES);
        for sample in 0..SAMPLES {
            for chunk_y in 0..DIRTY_CHUNKS {
                let y = chunk_y * CHUNK_SIZE;
                let offset = layer.cell_offset(0, y);
                layer.mmap[offset] = u8::try_from(sample % 2).unwrap();
                layer.mark_dirty(0, y);
            }
            let started = std::time::Instant::now();
            let stats = layer.msync_dirty_and_clear().unwrap();
            dirty.push(started.elapsed());
            assert_eq!(stats.dirty_chunks, DIRTY_CHUNKS as usize);
            assert_eq!(stats.ranges, 1);
        }

        no_dirty.sort_unstable();
        dirty.sort_unstable();
        eprintln!(
            "32x563 durability flush: no-dirty p50={:?} p95={:?} p99={:?}; 256-dirty p50={:?} p95={:?} p99={:?}",
            no_dirty[49], no_dirty[94], no_dirty[98], dirty[49], dirty[94], dirty[98]
        );

        drop(layer);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_world_cell_facade() {
        let temp_dir = std::env::temp_dir();
        let cell_defs = CellDefs::load(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("shared crate must live inside crates/")
                .parent()
                .expect("crates/ must live inside workspace root")
                .join("configs/cells.json"),
        )
        .unwrap();
        let world = World::new("test_world_facade", 1, 1, cell_defs, &temp_dir).unwrap();

        world.set_cell_typed(10, 10, CellType(cell_type::ROAD));

        let read = world.read_world_cell(10, 10).unwrap();
        assert_eq!(read.cell_type, CellType(cell_type::ROAD));

        world.set_cell_typed(11, 10, CellType(cell_type::GREEN));
        assert_eq!(world.get_cell_typed(11, 10), CellType(cell_type::GREEN));

        // cleanup temp files if created
        let _ = std::fs::remove_file(temp_dir.join("test_world_facade_v2.map"));
        let _ = std::fs::remove_file(temp_dir.join("test_world_facade_road_v2.map"));
        let _ = std::fs::remove_file(temp_dir.join("test_world_facade_durability.map"));
        let _ = std::fs::remove_file(temp_dir.join("test_world_facade_world.journal"));
    }

    #[test]
    fn solid_cell_preserves_background_road_layer() {
        let temp_dir = std::env::temp_dir();
        let name = format!("test_world_layers_{}", std::process::id());
        let cell_defs = CellDefs::load(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("shared crate must live inside crates/")
                .parent()
                .expect("crates/ must live inside workspace root")
                .join("configs/cells.json"),
        )
        .unwrap();
        let world = World::new(&name, 1, 1, cell_defs, &temp_dir).unwrap();

        world.set_cell_typed(10, 10, CellType(cell_type::ROAD));
        assert_eq!(world.get_cell(10, 10), cell_type::ROAD);
        assert_eq!(world.get_road_cell(10, 10), cell_type::ROAD);
        assert_eq!(world.get_solid_cell(10, 10), 0);

        world.set_cell_typed(10, 10, CellType(cell_type::ALIVE_CYAN));
        assert_eq!(world.get_cell(10, 10), cell_type::ALIVE_CYAN);
        assert_eq!(world.get_solid_cell(10, 10), cell_type::ALIVE_CYAN);
        assert_eq!(world.get_road_cell(10, 10), 0);

        world.destroy(10, 10);
        assert_eq!(world.get_cell(10, 10), cell_type::ROAD);
        assert_eq!(world.get_road_cell(10, 10), cell_type::ROAD);
        assert_eq!(world.get_solid_cell(10, 10), 0);
        assert_eq!(
            world.read_chunk_cells(0, 0)[10 + 10 * CHUNK_SIZE as usize],
            cell_type::ROAD
        );

        let _ = std::fs::remove_file(temp_dir.join(format!("{name}_v2.map")));
        let _ = std::fs::remove_file(temp_dir.join(format!("{name}_road_v2.map")));
        let _ = std::fs::remove_file(temp_dir.join(format!("{name}_durability.map")));
        let _ = std::fs::remove_file(temp_dir.join(format!("{name}_world.journal")));
    }

    #[test]
    fn snapshot_cells_rect_matches_visible_layer_semantics() {
        let temp_dir = std::env::temp_dir();
        let name = format!("test_world_snapshot_{}", std::process::id());
        let cell_defs = CellDefs::load(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("shared crate must live inside crates/")
                .parent()
                .expect("crates/ must live inside workspace root")
                .join("configs/cells.json"),
        )
        .unwrap();
        let world = World::new(&name, 1, 1, cell_defs, &temp_dir).unwrap();

        world.set_cell_typed(0, 0, CellType(cell_type::EMPTY));
        world.set_cell_typed(1, 1, CellType(cell_type::ROAD));
        world.set_cell_typed(2, 1, CellType(cell_type::GREEN));
        world.set_cell_typed(3, 0, CellType(cell_type::EMPTY));

        let snapshot = world.snapshot_cells_rect(0, 0, 4, 3);
        assert_eq!(snapshot.len(), 12);
        assert_eq!(snapshot[1 + 4], Some(CellType(cell_type::ROAD)));
        assert_eq!(snapshot[2 + 4], Some(CellType(cell_type::GREEN)));
        assert_eq!(snapshot[3], Some(CellType(cell_type::EMPTY)));

        let oob_snapshot = world.snapshot_cells_rect(-1, -1, 2, 2);
        assert_eq!(
            oob_snapshot,
            vec![None, None, None, Some(CellType(cell_type::EMPTY))]
        );

        let _ = std::fs::remove_file(temp_dir.join(format!("{name}_v2.map")));
        let _ = std::fs::remove_file(temp_dir.join(format!("{name}_road_v2.map")));
        let _ = std::fs::remove_file(temp_dir.join(format!("{name}_durability.map")));
        let _ = std::fs::remove_file(temp_dir.join(format!("{name}_world.journal")));
    }

    #[test]
    fn world_journal_replays_uncheckpointed_cell_write() {
        let temp_dir = std::env::temp_dir();
        let name = format!("test_world_journal_replay_{}", std::process::id());
        let cell_defs_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("shared crate must live inside crates/")
            .parent()
            .expect("crates/ must live inside workspace root")
            .join("configs/cells.json");

        {
            let world = World::new(
                &name,
                1,
                1,
                CellDefs::load(&cell_defs_path).unwrap(),
                &temp_dir,
            )
            .unwrap();
            world.set_cell_typed(10, 10, CellType(cell_type::ROAD));
            world.set_cell_typed(10, 10, CellType(cell_type::ALIVE_CYAN));
            world.set_durability(10, 10, 12.0);
        }

        let reopened = World::new(
            &name,
            1,
            1,
            CellDefs::load(&cell_defs_path).unwrap(),
            &temp_dir,
        )
        .unwrap();
        assert_eq!(reopened.get_cell(10, 10), cell_type::ALIVE_CYAN);
        assert_eq!(reopened.get_solid_cell(10, 10), cell_type::ALIVE_CYAN);
        assert_eq!(reopened.get_road_cell(10, 10), 0);
        assert!((reopened.get_durability(10, 10) - 12.0).abs() < f32::EPSILON);
        reopened.destroy(10, 10);
        assert_eq!(reopened.get_cell(10, 10), cell_type::ROAD);

        let _ = std::fs::remove_file(temp_dir.join(format!("{name}_v2.map")));
        let _ = std::fs::remove_file(temp_dir.join(format!("{name}_road_v2.map")));
        let _ = std::fs::remove_file(temp_dir.join(format!("{name}_durability.map")));
        let _ = std::fs::remove_file(temp_dir.join(format!("{name}_world.journal")));
    }

    #[test]
    fn world_flush_checkpoints_journal() {
        let temp_dir = std::env::temp_dir();
        let name = format!("test_world_journal_checkpoint_{}", std::process::id());
        let journal_path = temp_dir.join(format!("{name}_world.journal"));
        let cell_defs = CellDefs::load(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("shared crate must live inside crates/")
                .parent()
                .expect("crates/ must live inside workspace root")
                .join("configs/cells.json"),
        )
        .unwrap();
        let world = World::new(&name, 1, 1, cell_defs, &temp_dir).unwrap();

        world.set_cell_typed(12, 10, CellType(cell_type::GREEN));
        assert!(
            std::fs::metadata(&journal_path).unwrap().len() > 0,
            "world mutation must append to journal before checkpoint"
        );

        world.flush().unwrap();
        assert_eq!(
            std::fs::metadata(&journal_path).unwrap().len(),
            0,
            "checkpoint must truncate replay journal"
        );

        let _ = std::fs::remove_file(temp_dir.join(format!("{name}_v2.map")));
        let _ = std::fs::remove_file(temp_dir.join(format!("{name}_road_v2.map")));
        let _ = std::fs::remove_file(temp_dir.join(format!("{name}_durability.map")));
        let _ = std::fs::remove_file(journal_path);
    }
}
