use anyhow::Result;
use parking_lot::{Mutex, RwLock};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub mod journal;
pub mod layer;
pub mod provider;

use self::journal::{JournalRecord, WorldJournal};
use self::layer::{Layer, LayerFlushStats, LayerType, WorldFlushStats};
use self::provider::WorldProvider;

use super::cells::{CellDefs, CellType};
use super::generator;
use super::world_cell::WorldCell;
use super::{BACKUP_EVERY_N_FLUSHES, CHUNK_SIZE, EMPTY_CELL};

pub struct World {
    pub name: String,
    pub chunks_w: u32,
    pub chunks_h: u32,
    /// Клетки в клиент-совместимом формате `.map` (см. [`map_format`] /
    /// `client/Assets/Scripts/MapModel.cs`). Foreground/solid слой.
    pub(crate) cells: RwLock<MapStore>,
    /// Background/road слой. Соответствует C# `World.road`: если foreground
    /// пустой, клиент видит этот байт.
    pub(crate) road: RwLock<MapStore>,
    /// Серверная прочность клеток (`damage_cell`). У клиента понятия
    /// durability нет — это серверное состояние, отдельный mmap f32-слой.
    pub(crate) durability: RwLock<Layer>,
    /// Путь `{name}_v2.map` для инкрементального сохранения (как `MapModel`).
    pub(crate) map_path: PathBuf,
    /// Путь `{name}_road_v2.map` для background/road слоя.
    pub(crate) road_path: PathBuf,
    /// Append-only crash recovery log for world mutations after the last checkpoint.
    pub(crate) journal: Mutex<WorldJournal>,
    pub cell_defs: Arc<CellDefs>,
    /// Счётчик вызовов flush: дорогой `.bak` делаем не каждый flush,
    /// а раз в `BACKUP_EVERY_N_FLUSHES` (msync/save — каждый flush).
    pub(crate) flush_count: std::sync::atomic::AtomicU64,
}

use super::map_format::{MapFlushBatch, MapStore};

/// Detached foreground/background generations. Пока value живо и не
/// зафиксировано, любой early return автоматически возвращает dirty-маркеры.
pub(crate) struct DetachedMapFlush<'a> {
    world: &'a World,
    cells: Option<MapFlushBatch>,
    road: Option<MapFlushBatch>,
    committed: bool,
}

impl<'a> DetachedMapFlush<'a> {
    pub(crate) fn take(world: &'a World) -> Self {
        let cells = world.cells.write().take_flush_batch();
        let road = world.road.write().take_flush_batch();
        Self {
            world,
            cells,
            road,
            committed: false,
        }
    }

    pub(crate) fn persist(&self) -> Result<()> {
        if let Some(batch) = &self.cells {
            batch.persist(&self.world.map_path)?;
        }
        if let Some(batch) = &self.road {
            batch.persist(&self.world.road_path)?;
        }
        Ok(())
    }

    pub(crate) fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for DetachedMapFlush<'_> {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        if let Some(batch) = &self.cells {
            self.world.cells.write().restore_flush_batch(batch);
        }
        if let Some(batch) = &self.road {
            self.world.road.write().restore_flush_batch(batch);
        }
    }
}

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

    fn destroy_cell_and_road(&self, x: i32, y: i32) {
        self.write_world_cell(
            x,
            y,
            WorldCell {
                cell_type: CellType(EMPTY_CELL),
                durability: 0.0,
            },
        );
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

        // Под lock живой карты только отделяем immutable dirty generations.
        // open/seek/write/flush ниже не удерживают cells/road locks. Journal
        // остаётся барьером мутаций до успешного checkpoint, как и до refactor.
        let detached_maps = DetachedMapFlush::take(self);
        detached_maps.persist()?;
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
        detached_maps.commit();
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
    pub fn with_durability_mmap<R>(&self, f: impl FnOnce(&mut [u8]) -> R) -> R {
        let mut l = self.durability.write();
        let result = f(&mut l.mmap[..]);
        l.mark_all_dirty();
        result
    }

    /// Залить сгенерированные клетки в `.map` за один write-лок. Плоский
    /// буфер индексируется как прежняя chunk-раскладка
    /// (`chunk = cy + chunks_h*cx`, `cell = ly + 32*lx`); `0` → `EMPTY`.
    pub fn ingest_generated_cells(&self, flat: &[u8]) {
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
