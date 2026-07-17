use anyhow::{Context, Result};
use memmap2::MmapMut;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

use super::super::CHUNK_SIZE;

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

    pub(crate) fn mark_all_dirty(&mut self) {
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
