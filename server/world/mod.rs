pub mod anl;
pub mod cells;
pub mod dotnet_random;
pub mod generator;
pub mod map_format;
mod sector_palette;
mod sectors_gen;

use anyhow::{Context, Result};
use cells::{CellDefs, cell_type};
use map_format::MapStore;
use memmap2::MmapMut;
use parking_lot::RwLock;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const CHUNK_SIZE: u32 = 32;

/// Поддерживаемые типы данных в слоях карты.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerType {
    /// Единственный mmap-слой — durability (f32 на клетку). Клетки теперь
    /// хранятся в клиентском `.map` (см. [`map_format`]), не в `.mapb`.
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

        let mmap = unsafe { MmapMut::map_mut(&file)? };
        let dirty_count = (chunks_w * chunks_h) as usize;

        Ok(Self {
            mmap,
            path,
            chunks_h,
            data_type,
            dirty_mask: vec![false; dirty_count],
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
        if let Some(v) = self.dirty_mask.get_mut(idx) {
            *v = true;
        }
    }

    /// Под write-локом слоя: только msync (быстро, µs) + сброс dirty.
    /// Дорогой full-file `.bak` копируется ВНЕ лока (см. `World::flush`),
    /// иначе `fs::copy` ~ГБ держит write-лок секунды и фризит весь сервер.
    pub fn msync_and_clear(&mut self) -> Result<()> {
        let m0 = std::time::Instant::now();
        self.mmap.flush()?;
        let el = m0.elapsed();
        if el > std::time::Duration::from_millis(50) {
            tracing::warn!(
                target: "tickprof",
                "LAYER msync {:?} = {:?} (UNDER write lock)",
                self.path.file_name().unwrap_or_default(),
                el
            );
        }
        self.dirty_mask.fill(false);
        Ok(())
    }

    /// Путь файла слоя (для бэкапа вне лока).
    pub fn path(&self) -> PathBuf {
        self.path.clone()
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
    // TODO: get_solid_cell/get_road_cell will be used when layer-specific cell queries are needed
    #[allow(dead_code)]
    fn get_solid_cell(&self, x: i32, y: i32) -> u8;
    #[allow(dead_code)]
    fn get_road_cell(&self, x: i32, y: i32) -> u8;
    fn set_cell(&self, x: i32, y: i32, cell: u8);
    fn get_durability(&self, x: i32, y: i32) -> f32;
    fn set_durability(&self, x: i32, y: i32, d: f32);
    fn destroy(&self, x: i32, y: i32);
    fn damage_cell(&self, x: i32, y: i32, dmg: f32) -> bool;
    fn read_chunk_cells(&self, chunk_x: u32, chunk_y: u32) -> Vec<u8>;
    fn flush(&self) -> anyhow::Result<()>;
    fn is_empty(&self, x: i32, y: i32) -> bool;
}

pub struct World {
    pub name: String,
    pub chunks_w: u32,
    pub chunks_h: u32,
    /// Клетки в клиент-совместимом формате `.map` (см. [`map_format`] /
    /// `client/Assets/Scripts/MapModel.cs`). Один разреженный файл.
    cells: RwLock<MapStore>,
    /// Серверная прочность клеток (`damage_cell`). У клиента понятия
    /// durability нет — это серверное состояние, отдельный mmap f32-слой.
    durability: RwLock<Layer>,
    /// Путь `{name}_v2.map` для инкрементального сохранения (как `MapModel`).
    map_path: PathBuf,
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
        if b == 0 { EMPTY_CELL } else { b }
    }

    fn get_solid_cell(&self, x: i32, y: i32) -> u8 {
        let c = self.get_cell(x, y);
        if self.cell_defs.get(c).cell_is_empty() {
            0
        } else {
            c
        }
    }

    fn get_road_cell(&self, x: i32, y: i32) -> u8 {
        let c = self.get_cell(x, y);
        if self.cell_defs.get(c).cell_is_empty() {
            c
        } else {
            0
        }
    }

    fn set_cell(&self, x: i32, y: i32, cell: u8) {
        if !self.valid_coord(x, y) {
            return;
        }
        let (ux, uy) = (x.cast_unsigned(), y.cast_unsigned());
        let prop = self.cell_defs.get(cell);
        // Один байт на клетку — как клиентский `.map` (road/cells объединены).
        self.cells.write().set_cell(x, y, cell);
        // durability — серверное состояние (у клиента его нет): пусто → 0,
        // иначе значение по типу клетки из cell_defs.
        let d = if prop.cell_is_empty() {
            0.0f32
        } else {
            prop.durability
        };
        let mut layer = self.durability.write();
        let off = layer.cell_offset(ux, uy);
        layer.mmap[off..off + 4].copy_from_slice(&d.to_le_bytes());
        layer.mark_dirty(ux, uy);
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
        let mut layer = self.durability.write();
        let off = layer.cell_offset(x.cast_unsigned(), y.cast_unsigned());
        layer.mmap[off..off + 4].copy_from_slice(&d.to_le_bytes());
        layer.mark_dirty(x.cast_unsigned(), y.cast_unsigned());
    }

    fn destroy(&self, x: i32, y: i32) {
        if !self.valid_coord(x, y) || self.is_empty(x, y) {
            return;
        }
        // Выкопанная клетка = EMPTY (как видит клиент по проводу); dur → 0.
        self.set_cell(x, y, EMPTY_CELL);
    }

    fn damage_cell(&self, x: i32, y: i32, dmg: f32) -> bool {
        if !self.valid_coord(x, y) {
            return false;
        }
        let (ux, uy) = (x.cast_unsigned(), y.cast_unsigned());
        let destroyed = {
            let mut layer = self.durability.write();
            let off = layer.cell_offset(ux, uy);
            let d = f32::from_le_bytes([
                layer.mmap[off],
                layer.mmap[off + 1],
                layer.mmap[off + 2],
                layer.mmap[off + 3],
            ]);
            if d - dmg <= 0.0 {
                layer.mmap[off..off + 4].copy_from_slice(&0.0f32.to_le_bytes());
                layer.mark_dirty(ux, uy);
                true
            } else {
                layer.mmap[off..off + 4].copy_from_slice(&(d - dmg).to_le_bytes());
                layer.mark_dirty(ux, uy);
                false
            }
        };
        if destroyed {
            self.destroy(x, y);
        }
        destroyed
    }

    fn read_chunk_cells(&self, chunk_x: u32, chunk_y: u32) -> Vec<u8> {
        let n = (CHUNK_SIZE * CHUNK_SIZE) as usize;
        if chunk_x >= self.chunks_w || chunk_y >= self.chunks_h {
            return vec![0u8; n];
        }
        let base_x = chunk_x * CHUNK_SIZE;
        let base_y = chunk_y * CHUNK_SIZE;
        let store = self.cells.read();
        let mut res = Vec::with_capacity(n);
        // Порядок байт HB 'M' = как кэширует клиент (`MapBlock.data`,
        // индекс `x + 32*y`): for y:0..32 { for x:0..32 }.
        for y in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                let b = store.get_cell((base_x + x).cast_signed(), (base_y + y).cast_signed());
                res.push(if b == 0 { EMPTY_CELL } else { b });
            }
        }
        drop(store);
        res
    }

    fn flush(&self) -> Result<()> {
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
        if do_backup && self.map_path.exists() {
            let bak = self.map_path.with_extension("map.bak");
            let tmp = self.map_path.with_extension("map.tmp");
            let _ = fs::copy(&self.map_path, &tmp);
            let _ = fs::rename(&tmp, &bak);
        }

        // Durability: msync mmap-слоя под локом, бэкап вне лока.
        let dpath = {
            let mut l = self.durability.write();
            l.msync_and_clear()?;
            l.path()
        };
        if do_backup && dpath.exists() {
            let bak = dpath.with_extension("mapb.bak");
            let tmp = dpath.with_extension("mapb.tmp");
            let _ = fs::copy(&dpath, &tmp);
            let _ = fs::rename(&tmp, &bak);
        }
        Ok(())
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
        let is_new = !map_path.exists();

        let cells = MapStore::open(&map_path, width, height)?;
        tracing::info!(
            "Map store {}: {}x{}, {} blocks allocated",
            map_path.display(),
            cells.width(),
            cells.height(),
            cells.allocated_blocks()
        );
        let durability = Layer::open(
            state_dir.join(format!("{name}_durability.mapb")),
            chunks_w,
            chunks_h,
            LayerType::F32,
        )?;

        let world = Self {
            name: name.to_string(),
            chunks_w,
            chunks_h,
            cells: RwLock::new(cells),
            durability: RwLock::new(durability),
            map_path,
            cell_defs: Arc::new(cell_defs),
            flush_count: std::sync::atomic::AtomicU64::new(0),
        };

        if is_new {
            tracing::info!("Initializing new world...");
            generator::generate(&world, 42);
            world.flush()?;
        }

        Ok(world)
    }

    /// Дать генератору mmap durability-слоя (u8-вид f32) под write-локом.
    pub(crate) fn with_durability_mmap<R>(&self, f: impl FnOnce(&mut [u8]) -> R) -> R {
        let mut l = self.durability.write();
        f(&mut l.mmap[..])
    }

    /// Залить сгенерированные клетки в `.map` за один write-лок. Плоский
    /// буфер индексируется как прежняя `.mapb`-раскладка
    /// (`chunk = cy + chunks_h*cx`, `cell = ly + 32*lx`); `0` → `EMPTY`.
    pub(crate) fn ingest_generated_cells(&self, flat: &[u8]) {
        let cs = CHUNK_SIZE;
        let w = self.chunks_w * cs;
        let h = self.chunks_h * cs;
        let mut store = self.cells.write();
        for y in 0..h {
            for x in 0..w {
                let chunk_idx = ((y / cs) + self.chunks_h * (x / cs)) as usize;
                let cell_in_chunk = ((y % cs) + cs * (x % cs)) as usize;
                let idx = chunk_idx * (cs * cs) as usize + cell_in_chunk;
                let cell = flat[idx];
                store.set_cell(
                    x.cast_signed(),
                    y.cast_signed(),
                    if cell == 0 { EMPTY_CELL } else { cell },
                );
            }
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
