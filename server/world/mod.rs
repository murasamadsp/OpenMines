pub mod cells;
pub mod generator;
mod sector_palette;

use anyhow::{Context, Result};
use cells::{CellDefs, cell_type};
use memmap2::MmapMut;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const CHUNK_SIZE: u32 = 32;

/// Поддерживаемые типы данных в слоях карты.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerType {
    U8,
    F32,
    #[allow(dead_code)]
    U16,
}

impl LayerType {
    const fn size(self) -> usize {
        match self {
            Self::U8 => 1,
            Self::F32 => 4,
            Self::U16 => 2,
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
            .with_context(|| format!("Failed to open layer file: {path:?}"))?;

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
    pub fn cell_offset(&self, x: u32, y: u32) -> usize {
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

    pub fn flush(&mut self) -> Result<()> {
        // Пункт 5: Надежное сохранение.
        self.mmap.flush()?;

        let bak_path = self.path.with_extension("mapb.bak");
        let tmp_path = self.path.with_extension("mapb.tmp");

        if self.path.exists() {
            let _ = fs::copy(&self.path, &tmp_path);
            let _ = fs::rename(tmp_path, bak_path);
        }

        self.dirty_mask.fill(false);
        Ok(())
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
    #[allow(dead_code)]
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
    /// Пункт 4: Все слои теперь хранятся в одной мапе.
    layers: HashMap<String, RwLock<Layer>>,
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
        let (ux, uy) = (x.cast_unsigned(), y.cast_unsigned());
        let c = {
            let layer = self.layers.get("cells").unwrap().read();
            layer.mmap[layer.cell_offset(ux, uy)]
        };
        if c == 0 {
            let layer = self.layers.get("road").unwrap().read();
            let r = layer.mmap[layer.cell_offset(ux, uy)];
            if r == 0 { 32 } else { r }
        } else {
            c
        }
    }

    fn get_solid_cell(&self, x: i32, y: i32) -> u8 {
        if !self.valid_coord(x, y) {
            return 0;
        }
        let layer = self.layers.get("cells").unwrap().read();
        layer.mmap[layer.cell_offset(x.cast_unsigned(), y.cast_unsigned())]
    }

    fn get_road_cell(&self, x: i32, y: i32) -> u8 {
        if !self.valid_coord(x, y) {
            return 0;
        }
        let layer = self.layers.get("road").unwrap().read();
        layer.mmap[layer.cell_offset(x.cast_unsigned(), y.cast_unsigned())]
    }

    fn set_cell(&self, x: i32, y: i32, cell: u8) {
        if !self.valid_coord(x, y) {
            return;
        }
        let (ux, uy) = (x.cast_unsigned(), y.cast_unsigned());
        let prop = self.cell_defs.get(cell);
        if prop.cell_is_empty() {
            {
                let mut layer = self.layers.get("cells").unwrap().write();
                let off = layer.cell_offset(ux, uy);
                layer.mmap[off] = 0;
                layer.mark_dirty(ux, uy);
            }
            {
                let mut layer = self.layers.get("road").unwrap().write();
                let off = layer.cell_offset(ux, uy);
                layer.mmap[off] = cell;
                layer.mark_dirty(ux, uy);
            }
        } else {
            {
                let mut layer = self.layers.get("cells").unwrap().write();
                let off = layer.cell_offset(ux, uy);
                layer.mmap[off] = cell;
                layer.mark_dirty(ux, uy);
            }
            {
                let mut layer = self.layers.get("durability").unwrap().write();
                let off = layer.cell_offset(ux, uy);
                let bytes = prop.durability.to_le_bytes();
                layer.mmap[off..off + 4].copy_from_slice(&bytes);
                layer.mark_dirty(ux, uy);
            }
        }
    }

    fn get_durability(&self, x: i32, y: i32) -> f32 {
        if !self.valid_coord(x, y) {
            return 0.0;
        }
        let layer = self.layers.get("durability").unwrap().read();
        let off = layer.cell_offset(x.cast_unsigned(), y.cast_unsigned());
        let b = &layer.mmap[off..off + 4];
        let val = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        drop(layer);
        val
    }

    fn set_durability(&self, x: i32, y: i32, d: f32) {
        if !self.valid_coord(x, y) {
            return;
        }
        let mut layer = self.layers.get("durability").unwrap().write();
        let off = layer.cell_offset(x.cast_unsigned(), y.cast_unsigned());
        layer.mmap[off..off + 4].copy_from_slice(&d.to_le_bytes());
        layer.mark_dirty(x.cast_unsigned(), y.cast_unsigned());
    }

    fn destroy(&self, x: i32, y: i32) {
        if !self.valid_coord(x, y) {
            return;
        }
        let (ux, uy) = (x.cast_unsigned(), y.cast_unsigned());
        let was_solid = {
            let mut layer = self.layers.get("cells").unwrap().write();
            let off = layer.cell_offset(ux, uy);
            if layer.mmap[off] != 0 {
                layer.mmap[off] = 0;
                layer.mark_dirty(ux, uy);
                true
            } else {
                false
            }
        };
        if was_solid {
            let mut layer = self.layers.get("road").unwrap().write();
            let off = layer.cell_offset(ux, uy);
            if layer.mmap[off] == 0 {
                layer.mmap[off] = 32;
                layer.mark_dirty(ux, uy);
            }
        }
    }

    fn damage_cell(&self, x: i32, y: i32, dmg: f32) -> bool {
        if !self.valid_coord(x, y) {
            return false;
        }
        let (ux, uy) = (x.cast_unsigned(), y.cast_unsigned());
        let destroyed = {
            let mut layer = self.layers.get("durability").unwrap().write();
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
        let l_cells = self.layers.get("cells").unwrap().read();
        let l_road = self.layers.get("road").unwrap().read();
        let mut res = Vec::with_capacity(n);
        // 1:1 ref wire order for HB 'M':
        // `Chunk.cells => for y:0..32, for x:0..32, cell[x,y]`
        // (см. `server_reference/GameShit/WorldSystem/Chunk.cs`, свойство `cells`).
        for y in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                let ux = base_x + x;
                let uy = base_y + y;
                let c = l_cells.mmap[l_cells.cell_offset(ux, uy)];
                let v = if c == 0 {
                    let r = l_road.mmap[l_road.cell_offset(ux, uy)];
                    if r == 0 { 32 } else { r }
                } else {
                    c
                };
                res.push(v);
            }
        }
        drop(l_cells);
        drop(l_road);
        res
    }

    fn flush(&self) -> Result<()> {
        for layer in self.layers.values() {
            layer.write().flush()?;
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
        let mut layers = HashMap::new();
        let cells_path = state_dir.join(format!("{name}.mapb"));
        let is_new = !cells_path.exists();

        layers.insert(
            "cells".to_string(),
            RwLock::new(Layer::open(cells_path, chunks_w, chunks_h, LayerType::U8)?),
        );
        layers.insert(
            "road".to_string(),
            RwLock::new(Layer::open(
                state_dir.join(format!("{name}_road.mapb")),
                chunks_w,
                chunks_h,
                LayerType::U8,
            )?),
        );
        layers.insert(
            "durability".to_string(),
            RwLock::new(Layer::open(
                state_dir.join(format!("{name}_durability.mapb")),
                chunks_w,
                chunks_h,
                LayerType::F32,
            )?),
        );

        let world = Self {
            name: name.to_string(),
            chunks_w,
            chunks_h,
            layers,
            cell_defs: Arc::new(cell_defs),
        };

        if is_new {
            tracing::info!("Initializing new world...");
            world
                .layers
                .get("road")
                .unwrap()
                .write()
                .mmap
                .fill(cell_type::EMPTY);
            generator::generate(&world, 42);
            world.flush()?;
        }

        Ok(world)
    }

    pub(crate) fn with_generation_layers<R>(&self, f: impl FnOnce(&mut [u8], &mut [u8]) -> R) -> R {
        let mut l_cells = self.layers.get("cells").unwrap().write();
        let mut l_dur = self.layers.get("durability").unwrap().write();
        f(&mut l_cells.mmap[..], &mut l_dur.mmap[..])
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
