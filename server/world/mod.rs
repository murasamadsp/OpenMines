pub mod cells;
pub mod generator;
mod sector_palette;

use anyhow::Result;
use cells::{CellDefs, cell_type};
use memmap2::MmapMut;
use parking_lot::RwLock;
use std::fs::OpenOptions;
use std::path::Path;
use std::sync::Arc;

const CHUNK_SIZE: u32 = 32;

#[allow(dead_code)]
/// Memory-mapped world layer. Each cell is stored as T (u8 for cells/road, f32 for durability).
/// File layout: linear array of chunks, each chunk = 32*32 * sizeof(T) bytes.
/// Chunk index = `chunk_y` + `chunks_h` * `chunk_x` (matching C# convention).
struct MmapLayer {
    mmap: MmapMut,
    chunks_w: u32,
    chunks_h: u32,
    elem_size: usize,
}

#[allow(dead_code)]
impl MmapLayer {
    fn open(
        path: impl AsRef<Path>,
        chunks_w: u32,
        chunks_h: u32,
        elem_size: usize,
    ) -> Result<Self> {
        let cells_per_chunk = u64::from(CHUNK_SIZE) * u64::from(CHUNK_SIZE);
        let total_bytes =
            u64::from(chunks_w) * u64::from(chunks_h) * cells_per_chunk * elem_size as u64;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.as_ref())?;
        if file.metadata()?.len() < total_bytes {
            file.set_len(total_bytes)?;
        }
        let mmap = unsafe { MmapMut::map_mut(&file)? };
        Ok(Self {
            mmap,
            chunks_w,
            chunks_h,
            elem_size,
        })
    }

    #[inline]
    const fn chunk_index(&self, chunk_x: u32, chunk_y: u32) -> usize {
        (chunk_y + self.chunks_h * chunk_x) as usize
    }

    #[inline]
    const fn cell_offset(&self, x: u32, y: u32) -> usize {
        let cx = x / CHUNK_SIZE;
        let cy = y / CHUNK_SIZE;
        debug_assert!(cx < self.chunks_w && cy < self.chunks_h);
        let lx = x % CHUNK_SIZE;
        let ly = y % CHUNK_SIZE;
        let chunk_idx = self.chunk_index(cx, cy);
        let cell_in_chunk = (ly + CHUNK_SIZE * lx) as usize; // matches C# indexing
        let chunk_start = chunk_idx * (CHUNK_SIZE * CHUNK_SIZE) as usize;
        (chunk_start + cell_in_chunk) * self.elem_size
    }

    fn flush(&self) -> Result<()> {
        self.mmap.flush()?;
        Ok(())
    }
}

#[allow(dead_code)]
/// Typed accessor for u8 layers
struct U8Layer(MmapLayer);

#[allow(dead_code)]
impl U8Layer {
    fn open(path: impl AsRef<Path>, cw: u32, ch: u32) -> Result<Self> {
        Ok(Self(MmapLayer::open(path, cw, ch, 1)?))
    }

    #[inline]
    fn get(&self, x: u32, y: u32) -> u8 {
        let off = self.0.cell_offset(x, y);
        self.0.mmap[off]
    }

    #[inline]
    fn set(&mut self, x: u32, y: u32, val: u8) {
        let off = self.0.cell_offset(x, y);
        self.0.mmap[off] = val;
    }

    #[inline]
    fn fill(&mut self, value: u8) {
        self.0.mmap.fill(value);
    }

    fn flush(&self) -> Result<()> {
        self.0.flush()
    }
}

#[allow(dead_code)]
/// Typed accessor for f32 layers
struct F32Layer(MmapLayer);

#[allow(dead_code)]
impl F32Layer {
    fn open(path: impl AsRef<Path>, cw: u32, ch: u32) -> Result<Self> {
        Ok(Self(MmapLayer::open(path, cw, ch, 4)?))
    }

    #[inline]
    fn get(&self, x: u32, y: u32) -> f32 {
        let off = self.0.cell_offset(x, y);
        let bytes = &self.0.mmap[off..off + 4];
        f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }

    #[inline]
    fn set(&mut self, x: u32, y: u32, val: f32) {
        let off = self.0.cell_offset(x, y);
        let bytes = val.to_le_bytes();
        self.0.mmap[off..off + 4].copy_from_slice(&bytes);
    }

    fn flush(&self) -> Result<()> {
        self.0.flush()
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
    fn get_solid_cell(&self, x: i32, y: i32) -> u8;
    fn get_road_cell(&self, x: i32, y: i32) -> u8;
    fn set_cell(&self, x: i32, y: i32, cell: u8);
    fn get_durability(&self, x: i32, y: i32) -> f32;
    fn set_durability(&self, x: i32, y: i32, d: f32);
    #[allow(dead_code)]
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
    cells_layer: RwLock<U8Layer>,
    road_layer: RwLock<U8Layer>,
    durability_layer: RwLock<F32Layer>,
    pub cell_defs: Arc<CellDefs>,
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
        self.cells_width()
    }

    #[inline]
    fn cells_height(&self) -> u32 {
        self.cells_height()
    }

    #[inline]
    fn valid_coord(&self, x: i32, y: i32) -> bool {
        self.valid_coord(x, y)
    }

    #[inline]
    fn get_cell(&self, x: i32, y: i32) -> u8 {
        self.get_cell(x, y)
    }

    #[inline]
    fn get_solid_cell(&self, x: i32, y: i32) -> u8 {
        self.get_solid_cell(x, y)
    }

    #[inline]
    fn get_road_cell(&self, x: i32, y: i32) -> u8 {
        self.get_road_cell(x, y)
    }

    #[inline]
    fn set_cell(&self, x: i32, y: i32, cell: u8) {
        self.set_cell(x, y, cell);
    }

    #[inline]
    fn get_durability(&self, x: i32, y: i32) -> f32 {
        self.get_durability(x, y)
    }

    #[inline]
    fn set_durability(&self, x: i32, y: i32, d: f32) {
        self.set_durability(x, y, d);
    }

    #[inline]
    #[allow(dead_code)]
    fn destroy(&self, x: i32, y: i32) {
        self.destroy(x, y);
    }

    #[inline]
    fn damage_cell(&self, x: i32, y: i32, dmg: f32) -> bool {
        self.damage_cell(x, y, dmg)
    }

    #[inline]
    fn read_chunk_cells(&self, chunk_x: u32, chunk_y: u32) -> Vec<u8> {
        self.read_chunk_cells(chunk_x, chunk_y)
    }

    #[inline]
    fn flush(&self) -> anyhow::Result<()> {
        self.flush()
    }

    #[inline]
    fn is_empty(&self, x: i32, y: i32) -> bool {
        self.is_empty(x, y)
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
        let cells_path = state_dir.join(format!("{name}.mapb"));
        let is_new = !cells_path.exists();

        let cells_layer = U8Layer::open(&cells_path, chunks_w, chunks_h)?;
        let road_layer = U8Layer::open(
            state_dir.join(format!("{name}_road.mapb")),
            chunks_w,
            chunks_h,
        )?;
        let durability_layer = F32Layer::open(
            state_dir.join(format!("{name}_durability.mapb")),
            chunks_w,
            chunks_h,
        )?;

        let world = Self {
            name: name.to_string(),
            chunks_w,
            chunks_h,
            cells_layer: RwLock::new(cells_layer),
            road_layer: RwLock::new(road_layer),
            durability_layer: RwLock::new(durability_layer),
            cell_defs: Arc::new(cell_defs),
        };

        if is_new {
            tracing::info!("New world — running procedural generator");
            // Как в референсе MinesServer `Gen.StartGeneration`: для «воздуха» вызывается
            // `SetCell(..., 32)` (Empty) → в road, cells=0 (`World.SetCell` для isEmpty).
            // Пакетно заполняем road слоем 32: наш генератор пишет только cells/durability mmap,
            // иначе при cells==0 читался бы старый road (или при отдельной задумке — полимер 39).
            world.road_layer.write().fill(cell_type::EMPTY);
            generator::generate(&world, 42);
            world.flush()?;
        }

        Ok(world)
    }

    #[inline]
    pub const fn cells_width(&self) -> u32 {
        self.chunks_w * CHUNK_SIZE
    }

    #[inline]
    pub const fn cells_height(&self) -> u32 {
        self.chunks_h * CHUNK_SIZE
    }

    #[inline]
    pub const fn valid_coord(&self, x: i32, y: i32) -> bool {
        x >= 0
            && y >= 0
            && x.cast_unsigned() < self.cells_width()
            && y.cast_unsigned() < self.cells_height()
    }

    /// Get the effective cell at (x,y) — solid cell if present, otherwise road layer
    pub fn get_cell(&self, x: i32, y: i32) -> u8 {
        if !self.valid_coord(x, y) {
            return 0;
        }
        let (ux, uy) = (x.cast_unsigned(), y.cast_unsigned());
        let c = self.cells_layer.read().get(ux, uy);
        if c == 0 {
            let r = self.road_layer.read().get(ux, uy);
            if r == 0 { 32 } else { r }
        } else {
            c
        }
    }

    /// Get the solid cells layer value at (x,y) (0 means empty/road).
    pub fn get_solid_cell(&self, x: i32, y: i32) -> u8 {
        if !self.valid_coord(x, y) {
            return 0;
        }
        self.cells_layer
            .read()
            .get(x.cast_unsigned(), y.cast_unsigned())
    }

    /// Get the road layer value at (x,y).
    pub fn get_road_cell(&self, x: i32, y: i32) -> u8 {
        if !self.valid_coord(x, y) {
            return 0;
        }
        self.road_layer
            .read()
            .get(x.cast_unsigned(), y.cast_unsigned())
    }

    /// Set a cell — empty types go to road layer, solid to cells layer
    pub fn set_cell(&self, x: i32, y: i32, cell: u8) {
        if !self.valid_coord(x, y) {
            return;
        }
        let (ux, uy) = (x.cast_unsigned(), y.cast_unsigned());
        let prop = self.cell_defs.get(cell);
        if prop.cell_is_empty() {
            self.cells_layer.write().set(ux, uy, 0);
            self.road_layer.write().set(ux, uy, cell);
        } else {
            self.cells_layer.write().set(ux, uy, cell);
            self.durability_layer.write().set(ux, uy, prop.durability);
        }
    }

    pub fn get_durability(&self, x: i32, y: i32) -> f32 {
        if !self.valid_coord(x, y) {
            return 0.0;
        }
        self.durability_layer
            .read()
            .get(x.cast_unsigned(), y.cast_unsigned())
    }

    pub fn set_durability(&self, x: i32, y: i32, d: f32) {
        if !self.valid_coord(x, y) {
            return;
        }
        self.durability_layer
            .write()
            .set(x.cast_unsigned(), y.cast_unsigned(), d);
    }

    /// Destroy a cell — remove solid, expose road
    pub fn destroy(&self, x: i32, y: i32) {
        if !self.valid_coord(x, y) {
            return;
        }
        let (ux, uy) = (x.cast_unsigned(), y.cast_unsigned());
        let cleared = {
            let mut cells = self.cells_layer.write();
            if cells.get(ux, uy) != 0 {
                cells.set(ux, uy, 0);
                true
            } else {
                false
            }
        };
        if cleared {
            let mut road = self.road_layer.write();
            if road.get(ux, uy) == 0 {
                road.set(ux, uy, 32);
            }
        }
    }

    /// Damage a cell, returns true if destroyed
    pub fn damage_cell(&self, x: i32, y: i32, dmg: f32) -> bool {
        let cell = self.get_cell(x, y);
        let prop = self.cell_defs.get(cell);
        let mut d = self.get_durability(x, y);
        if !d.is_finite() || (d <= 0.0 && prop.durability > 0.0 && !prop.cell_is_empty()) {
            d = prop.durability;
        }
        // Если в слое прочности осталось значение больше максимума для текущего типа клетки
        // (рассинхрон mmap / старый мир / битые байты), урон почти не снижает `d` —
        // кристаллы при этом ломаются (фиксированный dmg), обычные блоки — «не копаются».
        if prop.durability > 0.0 && d > prop.durability {
            d = prop.durability;
        }
        if d - dmg <= 0.0 {
            self.set_durability(x, y, 0.0);
            self.destroy(x, y);
            true
        } else {
            self.set_durability(x, y, d - dmg);
            false
        }
    }

    /// Read chunk cells for sending to client (row-major: y outer, x inner).
    ///
    /// Два `read()` на весь чанк вместо 1024×`get_cell` (каждый = до 2 замков) — горячий путь при синке карты.
    #[allow(clippy::significant_drop_tightening)] // держим два read-guard осознанно, без 2048 `read()` на чанк
    pub fn read_chunk_cells(&self, chunk_x: u32, chunk_y: u32) -> Vec<u8> {
        let n = (CHUNK_SIZE * CHUNK_SIZE) as usize;
        if chunk_x >= self.chunks_w || chunk_y >= self.chunks_h {
            return vec![0u8; n];
        }
        let base_x = chunk_x * CHUNK_SIZE;
        let base_y = chunk_y * CHUNK_SIZE;
        let cells_guard = self.cells_layer.read();
        let road_guard = self.road_layer.read();
        let mut result = Vec::with_capacity(n);
        for y in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                let ux = base_x + x;
                let uy = base_y + y;
                let c = cells_guard.get(ux, uy);
                let v = if c == 0 {
                    let r = road_guard.get(ux, uy);
                    if r == 0 { cell_type::EMPTY } else { r }
                } else {
                    c
                };
                result.push(v);
            }
        }
        result
    }

    pub fn flush(&self) -> Result<()> {
        self.cells_layer.read().flush()?;
        self.road_layer.read().flush()?;
        self.durability_layer.read().flush()?;
        Ok(())
    }

    /// Эксклюзивная запись в mmap-слои (только при первичной генерации мира).
    /// Без промежуточных `Vec` — lossless тот же layout, что и у `par_chunks_exact_mut` по буферу.
    pub(crate) fn with_generation_layers<R>(&self, f: impl FnOnce(&mut [u8], &mut [u8]) -> R) -> R {
        let mut cells = self.cells_layer.write();
        let mut dur = self.durability_layer.write();
        f(&mut cells.0.mmap[..], &mut dur.0.mmap[..])
    }

    pub(crate) const fn chunks_layout(&self) -> (u32, u32, u32) {
        (self.chunks_w, self.chunks_h, CHUNK_SIZE)
    }

    pub fn is_empty(&self, x: i32, y: i32) -> bool {
        self.cell_defs.get(self.get_cell(x, y)).cell_is_empty()
    }

    pub fn chunk_pos(x: i32, y: i32) -> (u32, u32) {
        (
            x.max(0).cast_unsigned() / CHUNK_SIZE,
            y.max(0).cast_unsigned() / CHUNK_SIZE,
        )
    }
}
