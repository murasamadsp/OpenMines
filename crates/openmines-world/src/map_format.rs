//! Клиент-совместимый формат мира `.map` (источник правды:
//! `client/Assets/Scripts/MapModel.cs`, `MapBlock.cs`).
//!
//! Один разреженный файл, layout идентичен `MapModel.SaveMapV2`/`LoadMapV2`:
//!
//! ```text
//! [0]                       i32 LE  width
//! [4]                       i32 LE  height
//! [8]                       index-таблица: blocks_w*blocks_h × i32 LE
//!                           (blockId -> slot, -1 = блок отсутствует)
//! [8 + idx_bytes]           блоки по 1024 байта, slot i = блок back_index[i]
//! ```
//!
//! - `blocks_w = width / 32`, `blocks_h = height / 32` (floor, как `Mathf.FloorToInt`);
//! - `blockId = (x >> 5) + (y >> 5) * blocks_w`;
//! - индекс клетки в блоке = `x % 32 + 32 * (y % 32)` (x внутренний, y внешний);
//! - один байт на клетку (road/cells объединены, как у клиента);
//! - хранятся только материализованные блоки, слоты выделяются в порядке
//!   первого `set_cell` в блок (как `_indexSize++` в `MapModel`).
//!
//! Модуль — чистый кодек хранения клеток. Границы мира, durability и
//! сетевая отдача чанков относятся к слою `World`, не сюда.

use anyhow::{Context, Result, bail};

/// Сторона блока в клетках (`MapModel` использует 32).
pub const BLOCK_SIZE: i32 = 32;
/// Клеток в блоке (`MapBlock.data` = `new byte[1024]`).
pub const BLOCK_CELLS: usize = (BLOCK_SIZE * BLOCK_SIZE) as usize;
/// Размер заголовка: два i32 (`Buffer.BlockCopy(new int[]{w,h}, ... ,8)`).
const HEADER_BYTES: usize = 8;
/// Значение `indexes`, означающее «блок не выделен» (`MapModel` init = -1).
const NO_SLOT: i32 = -1;

/// Разреженное клеточное хранилище мира в формате клиента `.map`.
#[derive(Debug)]
pub struct MapStore {
    width: i32,
    height: i32,
    blocks_w: i32,
    blocks_h: i32,
    /// `blockId -> slot` (или `NO_SLOT`); длина = `blocks_w * blocks_h`.
    indexes: Vec<i32>,
    /// `slot -> blockId`; длина = числу выделенных слотов.
    back_index: Vec<usize>,
    /// `slot -> 1024 байта клеток`.
    blocks: Vec<[u8; BLOCK_CELLS]>,
    /// `slot -> изменён с последнего save` (`MapBlock.notSaved`).
    dirty: Vec<bool>,
    /// Только изменённые slots. Маска `dirty` дедуплицирует повторные записи в
    /// один блок, поэтому steady-state flush зависит от dirty work.
    dirty_slots: Vec<usize>,
    /// Index-таблица изменилась с последнего save (`MapModel.updateIndex`).
    index_dirty: bool,
    /// Заголовок ещё не записан (`MapModel.isNewFile`).
    needs_header: bool,
    /// Монотонный номер detached batch. Нужен для диагностики и проверки
    /// порядка поколений при повторном flush после ошибки.
    next_flush_generation: u64,
}

#[derive(Debug)]
struct MapFlushSlot {
    slot: usize,
    data: [u8; BLOCK_CELLS],
}

/// Неизменяемый снимок одного поколения map flush.
///
/// Batch не заимствует [`MapStore`], поэтому файловый I/O можно выполнять после
/// освобождения lock живой карты. При ошибке [`MapStore::restore_flush_batch`]
/// возвращает только dirty-маркеры; данные всегда берутся из актуального store.
#[derive(Debug)]
pub(crate) struct MapFlushBatch {
    generation: u64,
    header: Option<(i32, i32)>,
    indexes: Option<Vec<i32>>,
    index_len: usize,
    slots: Vec<MapFlushSlot>,
}

impl MapFlushBatch {
    pub(crate) fn persist(&self, path: &std::path::Path) -> Result<()> {
        use std::io::Write as _;

        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .with_context(|| format!("open map file {}", path.display()))?;
        self.write_to(&mut file).with_context(|| {
            format!(
                "write map flush generation {} to {}",
                self.generation,
                path.display()
            )
        })?;
        file.flush()
            .with_context(|| format!("flush map file {}", path.display()))?;
        Ok(())
    }

    fn write_to<W>(&self, writer: &mut W) -> std::io::Result<()>
    where
        W: std::io::Seek + std::io::Write,
    {
        use std::io::SeekFrom;

        if let Some((width, height)) = self.header {
            writer.seek(SeekFrom::Start(0))?;
            writer.write_all(&width.to_le_bytes())?;
            writer.write_all(&height.to_le_bytes())?;
        }
        if let Some(indexes) = &self.indexes {
            writer.seek(SeekFrom::Start(HEADER_BYTES as u64))?;
            let mut table = Vec::with_capacity(indexes.len() * 4);
            for &idx in indexes {
                table.extend_from_slice(&idx.to_le_bytes());
            }
            writer.write_all(&table)?;
        }
        let base = (HEADER_BYTES + self.index_len * 4) as u64;
        for slot in &self.slots {
            writer.seek(SeekFrom::Start(base + (slot.slot * BLOCK_CELLS) as u64))?;
            writer.write_all(&slot.data)?;
        }
        Ok(())
    }

    #[cfg(test)]
    const fn generation(&self) -> u64 {
        self.generation
    }

    #[cfg(test)]
    const fn dirty_slot_count(&self) -> usize {
        self.slots.len()
    }
}

impl MapStore {
    /// Создать пустой мир `width × height`.
    ///
    /// # Errors
    /// Если `width`/`height` не положительны или дают пустую сетку блоков.
    pub fn new(width: i32, height: i32) -> Result<Self> {
        if width <= 0 || height <= 0 {
            bail!("map dimensions must be positive, got {width}x{height}");
        }
        let blocks_w = width / BLOCK_SIZE;
        let blocks_h = height / BLOCK_SIZE;
        if blocks_w <= 0 || blocks_h <= 0 {
            bail!("map smaller than one 32x32 block: {width}x{height}");
        }
        let grid = usize::try_from(blocks_w)
            .and_then(|w| usize::try_from(blocks_h).map(|h| w * h))
            .map_err(|_| anyhow::anyhow!("block grid too large"))?;
        Ok(Self {
            width,
            height,
            blocks_w,
            blocks_h,
            indexes: vec![NO_SLOT; grid],
            back_index: Vec::new(),
            blocks: Vec::new(),
            dirty: Vec::new(),
            dirty_slots: Vec::new(),
            index_dirty: true,
            needs_header: true,
            next_flush_generation: 0,
        })
    }

    #[must_use]
    pub const fn width(&self) -> i32 {
        self.width
    }

    #[must_use]
    pub const fn height(&self) -> i32 {
        self.height
    }

    /// Число материализованных блоков (`MapModel._indexSize`).
    #[must_use]
    pub const fn allocated_blocks(&self) -> usize {
        self.blocks.len()
    }

    /// `(blockId, cellIndex)` для `(x, y)`, либо `None` вне сетки целых
    /// блоков (`blocks_w*32 × blocks_h*32`). Соответствует
    /// `blockId = (x>>5) + (y>>5)*blocks_w`, `cell = x%32 + 32*(y%32)`
    /// из `MapModel`. Клиент для `x >= width` возвращает рамку до
    /// индексации — поэтому за пределами целых блоков клетка недостижима.
    #[must_use]
    fn locate(&self, x: i32, y: i32) -> Option<(usize, usize)> {
        if x < 0 || y < 0 || x >= self.blocks_w * BLOCK_SIZE || y >= self.blocks_h * BLOCK_SIZE {
            return None;
        }
        let bx = x / BLOCK_SIZE;
        let by = y / BLOCK_SIZE;
        let cell = (x % BLOCK_SIZE) + BLOCK_SIZE * (y % BLOCK_SIZE);
        // Все слагаемые неотрицательны и ограничены сеткой блоков.
        let block_id = usize::try_from(bx + by * self.blocks_w).ok()?;
        let cell_idx = usize::try_from(cell).ok()?;
        Some((block_id, cell_idx))
    }

    /// Слот блока, либо `None`, если блок не материализован.
    #[must_use]
    fn slot_of(&self, block_id: usize) -> Option<usize> {
        match self.indexes[block_id] {
            NO_SLOT => None,
            s => usize::try_from(s).ok(),
        }
    }

    fn mark_slot_dirty(&mut self, slot: usize) {
        if !self.dirty[slot] {
            self.dirty[slot] = true;
            self.dirty_slots.push(slot);
        }
    }

    /// Значение клетки. Невыделенный блок или вне границ → `0`
    /// (ядро `MapModel.GetCell`: `_Blocks[num] == null` → `0`).
    #[must_use]
    pub fn get_cell(&self, x: i32, y: i32) -> u8 {
        let Some((block_id, cell_idx)) = self.locate(x, y) else {
            return 0;
        };
        self.slot_of(block_id)
            .map_or(0, |slot| self.blocks[slot][cell_idx])
    }

    /// Записать клетку. Первый `set_cell` в блок материализует его и
    /// выделяет слот в порядке вставки (`MapModel.SetCell`: при `null`
    /// блоке — `indexes[num] = _indexSize; back_index[_indexSize] = num; _indexSize++`).
    /// Вне границ — игнор (как ранний `return` в `MapModel.SetCell`).
    ///
    /// # Panics
    /// Если число слотов превысит `i32::MAX` (сетка блоков ограничена
    /// размерами мира, недостижимо для реальных карт).
    pub fn set_cell(&mut self, x: i32, y: i32, cell: u8) {
        let Some((block_id, cell_idx)) = self.locate(x, y) else {
            return;
        };
        let slot = if let Some(s) = self.slot_of(block_id) {
            s
        } else {
            let new_slot = self.blocks.len();
            self.blocks.push([0u8; BLOCK_CELLS]);
            self.back_index.push(block_id);
            self.dirty.push(false);
            self.indexes[block_id] =
                i32::try_from(new_slot).expect("slot count fits i32 (grid bounded by world size)");
            self.index_dirty = true;
            new_slot
        };
        self.blocks[slot][cell_idx] = cell;
        self.mark_slot_dirty(slot);
    }

    /// Размер сериализованного файла в байтах (для тестов байт-паритета;
    /// в проде используется инкрементальный [`Self::save`]).
    #[cfg(test)]
    #[must_use]
    pub const fn serialized_len(&self) -> usize {
        HEADER_BYTES + self.indexes.len() * 4 + self.blocks.len() * BLOCK_CELLS
    }

    /// Полная сериализация в формат `_v2.map` (`MapModel.SaveMapV2`).
    /// Только для тестов байт-паритета; прод пишет через [`Self::save`].
    #[cfg(test)]
    #[must_use]
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.serialized_len());
        out.extend_from_slice(&self.width.to_le_bytes());
        out.extend_from_slice(&self.height.to_le_bytes());
        for &idx in &self.indexes {
            out.extend_from_slice(&idx.to_le_bytes());
        }
        // Слот i на диске = блок back_index[i] (1024*i, как LoadBlockFromFile).
        for block in &self.blocks {
            out.extend_from_slice(&block[..]);
        }
        out
    }

    /// Разобрать файл `_v2.map` (`MapModel.LoadMapV2`).
    ///
    /// # Errors
    /// При несоответствии размеров заголовку или усечённом файле.
    pub fn deserialize(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < HEADER_BYTES {
            bail!("map file shorter than header ({} bytes)", bytes.len());
        }
        let width = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let height = i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let mut store = Self::new(width, height)?;

        let idx_bytes = store.indexes.len() * 4;
        let idx_end = HEADER_BYTES + idx_bytes;
        if bytes.len() < idx_end {
            bail!(
                "map file truncated in index table: have {}, need {}",
                bytes.len(),
                idx_end
            );
        }
        // _indexSize = max(indexes[i]) + 1 (MapModel.LoadMapV2).
        let mut index_size: i32 = 0;
        for (i, chunk) in bytes[HEADER_BYTES..idx_end].chunks_exact(4).enumerate() {
            let v = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            store.indexes[i] = v;
            if v + 1 > index_size {
                index_size = v + 1;
            }
        }

        let slots = usize::try_from(index_size).unwrap_or(0);
        let need = idx_end + slots * BLOCK_CELLS;
        if bytes.len() < need {
            bail!(
                "map file truncated in blocks: have {}, need {} ({slots} slots)",
                bytes.len(),
                need
            );
        }
        store.back_index = vec![0usize; slots];
        for (block_id, &slot) in store.indexes.iter().enumerate() {
            if slot != NO_SLOT {
                let s = usize::try_from(slot)
                    .map_err(|_| anyhow::anyhow!("negative slot in index table"))?;
                if s >= slots {
                    bail!("index table references slot {s} >= {slots}");
                }
                store.back_index[s] = block_id;
            }
        }
        store.blocks = Vec::with_capacity(slots);
        for s in 0..slots {
            let start = idx_end + s * BLOCK_CELLS;
            let mut data = [0u8; BLOCK_CELLS];
            data.copy_from_slice(&bytes[start..start + BLOCK_CELLS]);
            store.blocks.push(data);
        }
        // Загружено с диска — всё уже сохранено.
        store.dirty = vec![false; slots];
        store.dirty_slots.clear();
        store.index_dirty = false;
        store.needs_header = false;
        Ok(store)
    }

    /// Открыть `.map` по пути. Если файл есть и заголовок совпадает с
    /// `width`/`height` — загрузить; иначе свежий пустой мир (как
    /// `MapModel`: при несовпадении размеров — `ReopenEmptyFile`).
    ///
    /// # Errors
    /// Ошибка чтения файла или повреждённый существующий файл нужного размера.
    pub fn open(path: &std::path::Path, width: i32, height: i32) -> Result<Self> {
        if path.exists() {
            let bytes =
                std::fs::read(path).with_context(|| format!("read map file {}", path.display()))?;
            if bytes.len() >= HEADER_BYTES {
                let w = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                let h = i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
                if w == width && h == height {
                    return Self::deserialize(&bytes);
                }
            }
        }
        Self::new(width, height)
    }

    pub(crate) fn take_flush_batch(&mut self) -> Option<MapFlushBatch> {
        if !self.is_dirty() {
            return None;
        }
        let slots = self
            .dirty_slots
            .iter()
            .map(|&slot| MapFlushSlot {
                slot,
                data: self.blocks[slot],
            })
            .collect();
        let batch = MapFlushBatch {
            generation: self.next_flush_generation,
            header: self.needs_header.then_some((self.width, self.height)),
            indexes: self.index_dirty.then(|| self.indexes.clone()),
            index_len: self.indexes.len(),
            slots,
        };
        self.next_flush_generation = self
            .next_flush_generation
            .checked_add(1)
            .expect("map flush generation overflow");
        self.needs_header = false;
        self.index_dirty = false;
        for &slot in &self.dirty_slots {
            self.dirty[slot] = false;
        }
        self.dirty_slots.clear();
        Some(batch)
    }

    pub(crate) fn restore_flush_batch(&mut self, batch: &MapFlushBatch) {
        self.needs_header |= batch.header.is_some();
        self.index_dirty |= batch.indexes.is_some();
        for flushed in &batch.slots {
            self.mark_slot_dirty(flushed.slot);
        }
    }

    /// Инкрементально записать в `.map` с восстановлением dirty state при
    /// частичной или полной ошибке файлового I/O.
    pub fn save(&mut self, path: &std::path::Path) -> Result<()> {
        let Some(batch) = self.take_flush_batch() else {
            return Ok(());
        };
        if let Err(error) = batch.persist(path) {
            self.restore_flush_batch(&batch);
            return Err(error);
        }
        Ok(())
    }

    /// Есть ли несохранённые изменения (для пропуска лишних `save`).
    #[must_use]
    pub const fn is_dirty(&self) -> bool {
        self.needs_header || self.index_dirty || !self.dirty_slots.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FailAfter {
        inner: std::io::Cursor<Vec<u8>>,
        remaining: usize,
    }

    impl std::io::Write for FailAfter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if self.remaining == 0 {
                return Err(std::io::Error::other("injected map write failure"));
            }
            let len = buf.len().min(self.remaining);
            let written = self.inner.write(&buf[..len])?;
            self.remaining -= written;
            Ok(written)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.inner.flush()
        }
    }

    impl std::io::Seek for FailAfter {
        fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(pos)
        }
    }

    #[test]
    fn new_computes_block_grid_with_floor() {
        // 70 / 32 = 2 (floor), не 3.
        let m = MapStore::new(70, 96).unwrap();
        assert_eq!(m.blocks_w, 2);
        assert_eq!(m.blocks_h, 3);
        assert_eq!(m.indexes.len(), 6);
        assert!(m.indexes.iter().all(|&v| v == NO_SLOT));
        assert_eq!(m.allocated_blocks(), 0);
    }

    #[test]
    fn rejects_degenerate_dimensions() {
        assert!(MapStore::new(0, 64).is_err());
        assert!(MapStore::new(64, -1).is_err());
        assert!(MapStore::new(31, 31).is_err()); // меньше одного блока
    }

    #[test]
    fn locate_matches_mapmodel_formulas() {
        let m = MapStore::new(64, 64).unwrap(); // 2x2 блока
        // (33,1): bx=1,by=0 -> id 1 ; cell = 33%32 + 32*(1%32) = 1 + 32 = 33
        assert_eq!(m.locate(33, 1), Some((1, 33)));
        // (0,32): bx=0,by=1 -> id = 0 + 1*2 = 2 ; cell = 0
        assert_eq!(m.locate(0, 32), Some((2, 0)));
        // вне сетки целых блоков
        assert_eq!(m.locate(-1, 0), None);
        assert_eq!(m.locate(64, 0), None);
    }

    #[test]
    fn set_get_roundtrip_and_sparsity() {
        let mut m = MapStore::new(64, 64).unwrap();
        assert_eq!(m.get_cell(10, 10), 0); // невыделенный блок
        m.set_cell(10, 10, 117);
        m.set_cell(40, 5, 32); // другой блок (bx=1)
        assert_eq!(m.get_cell(10, 10), 117);
        assert_eq!(m.get_cell(40, 5), 32);
        // Только 2 блока материализованы из 4.
        assert_eq!(m.allocated_blocks(), 2);
        assert_eq!(m.get_cell(0, 0), 0);
        assert_eq!(m.get_cell(63, 63), 0);
    }

    #[test]
    fn out_of_bounds_is_ignored() {
        let mut m = MapStore::new(64, 64).unwrap();
        m.set_cell(-1, 0, 9);
        m.set_cell(64, 0, 9);
        m.set_cell(0, 64, 9);
        assert_eq!(m.allocated_blocks(), 0);
        assert_eq!(m.get_cell(-1, 0), 0);
        assert_eq!(m.get_cell(999, 999), 0);
    }

    #[test]
    fn slot_allocation_is_insertion_order() {
        let mut m = MapStore::new(96, 32).unwrap(); // 3x1 блока
        m.set_cell(64, 0, 1); // block 2 -> slot 0
        m.set_cell(0, 0, 2); // block 0 -> slot 1
        assert_eq!(m.indexes[2], 0);
        assert_eq!(m.indexes[0], 1);
        assert_eq!(m.indexes[1], NO_SLOT);
        assert_eq!(m.back_index, vec![2, 0]);
    }

    #[test]
    fn serialized_layout_matches_spec_exactly() {
        let mut m = MapStore::new(64, 32).unwrap(); // blocks_w=2, blocks_h=1
        m.set_cell(33, 0, 0xAB); // block 1 -> slot 0, cell idx 1
        let bytes = m.serialize();

        // header: width=64, height=32 LE
        assert_eq!(&bytes[0..4], &64i32.to_le_bytes());
        assert_eq!(&bytes[4..8], &32i32.to_le_bytes());
        // index table: 2 i32 -> indexes[0]=-1, indexes[1]=0
        assert_eq!(&bytes[8..12], &(-1i32).to_le_bytes());
        assert_eq!(&bytes[12..16], &0i32.to_le_bytes());
        // один блок: 1024 байта, cell idx 1 = 0xAB, остальное 0
        let block_start = HEADER_BYTES + 2 * 4;
        assert_eq!(bytes.len(), block_start + BLOCK_CELLS);
        assert_eq!(bytes[block_start + 1], 0xAB);
        assert_eq!(bytes[block_start], 0);
        assert_eq!(bytes[block_start + 2], 0);
    }

    #[test]
    fn deserialize_inverts_serialize() {
        let mut m = MapStore::new(128, 96).unwrap();
        m.set_cell(5, 5, 117);
        m.set_cell(100, 70, 49);
        m.set_cell(127, 95, 90);
        m.set_cell(5, 5, 64); // перезапись той же клетки

        let bytes = m.serialize();
        let back = MapStore::deserialize(&bytes).unwrap();

        assert_eq!(back.width(), 128);
        assert_eq!(back.height(), 96);
        assert_eq!(back.allocated_blocks(), m.allocated_blocks());
        assert_eq!(back.get_cell(5, 5), 64);
        assert_eq!(back.get_cell(100, 70), 49);
        assert_eq!(back.get_cell(127, 95), 90);
        assert_eq!(back.get_cell(0, 0), 0);
        assert_eq!(back.serialize(), bytes);
    }

    #[test]
    fn deserialize_rejects_truncated_input() {
        let m = MapStore::new(64, 64).unwrap();
        let bytes = m.serialize();
        assert!(MapStore::deserialize(&bytes[..4]).is_err()); // короче заголовка
        assert!(MapStore::deserialize(&bytes[..bytes.len() - 1]).is_err()); // обрезана таблица
    }

    #[test]
    fn file_save_open_roundtrip_incremental() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("om_mapfmt_test_{}.map", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let mut m = MapStore::new(128, 96).unwrap();
        m.set_cell(5, 5, 117);
        m.set_cell(100, 70, 49);
        assert!(m.is_dirty());
        m.save(&path).unwrap();
        assert!(!m.is_dirty());

        // Файл на диске идентичен полной сериализации.
        assert_eq!(std::fs::read(&path).unwrap(), m.serialize());

        // Инкрементальная дозапись одного слота.
        m.set_cell(5, 5, 64);
        m.set_cell(127, 95, 90); // новый блок -> index_dirty
        m.save(&path).unwrap();

        let reopened = MapStore::open(&path, 128, 96).unwrap();
        assert_eq!(reopened.get_cell(5, 5), 64);
        assert_eq!(reopened.get_cell(100, 70), 49);
        assert_eq!(reopened.get_cell(127, 95), 90);
        assert!(!reopened.is_dirty());
        assert_eq!(reopened.serialize(), m.serialize());

        // Несовпадение размеров -> свежий мир (как ReopenEmptyFile).
        let fresh = MapStore::open(&path, 64, 64).unwrap();
        assert_eq!(fresh.allocated_blocks(), 0);

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn detached_flush_collects_only_unique_dirty_slots() {
        const BLOCKS: i32 = 128;
        let mut map = MapStore::new(BLOCK_SIZE * BLOCKS, BLOCK_SIZE).unwrap();
        for block in 0..BLOCKS {
            map.set_cell(block * BLOCK_SIZE, 0, 1);
        }
        let initial = map.take_flush_batch().unwrap();
        assert_eq!(initial.dirty_slot_count(), BLOCKS as usize);
        assert!(!map.is_dirty());

        for value in 2..=10 {
            map.set_cell(0, 0, value);
        }
        map.set_cell((BLOCKS - 1) * BLOCK_SIZE, 0, 11);
        let incremental = map.take_flush_batch().unwrap();

        assert_eq!(incremental.dirty_slot_count(), 2);
        assert!(incremental.header.is_none());
        assert!(incremental.indexes.is_none());
    }

    #[test]
    fn failed_older_generation_requeues_latest_slot_value_once() {
        let mut map = MapStore::new(64, 32).unwrap();
        map.set_cell(5, 5, 10);
        let first = map.take_flush_batch().unwrap();

        map.set_cell(5, 5, 20);
        map.restore_flush_batch(&first);
        assert_eq!(map.dirty_slots.len(), 1);

        let retry = map.take_flush_batch().unwrap();
        let (block_id, cell_idx) = map.locate(5, 5).unwrap();
        let slot = map.slot_of(block_id).unwrap();
        let persisted = retry
            .slots
            .iter()
            .find(|candidate| candidate.slot == slot)
            .unwrap();

        assert!(retry.generation() > first.generation());
        assert_eq!(retry.dirty_slot_count(), 1);
        assert_eq!(persisted.data[cell_idx], 20);
    }

    #[test]
    fn partial_batch_write_can_be_restored_and_retried() {
        let mut map = MapStore::new(64, 32).unwrap();
        map.set_cell(5, 5, 117);
        let batch = map.take_flush_batch().unwrap();
        let mut writer = FailAfter {
            inner: std::io::Cursor::new(Vec::new()),
            remaining: HEADER_BYTES + 4,
        };

        assert!(batch.write_to(&mut writer).is_err());
        map.restore_flush_batch(&batch);
        assert!(map.is_dirty());

        let retry = map.take_flush_batch().unwrap();
        let mut bytes = std::io::Cursor::new(Vec::new());
        retry.write_to(&mut bytes).unwrap();
        assert_eq!(bytes.into_inner(), map.serialize());
        assert!(!map.is_dirty());
    }

    #[test]
    fn save_open_failure_restores_detached_dirty_state() {
        let mut map = MapStore::new(64, 32).unwrap();
        map.set_cell(5, 5, 117);
        let missing_parent = std::env::temp_dir().join(format!(
            "om_mapfmt_missing_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = missing_parent.join("map.map");

        assert!(map.save(&path).is_err());
        assert!(map.is_dirty());

        let retry = map.take_flush_batch().unwrap();
        assert_eq!(retry.dirty_slot_count(), 1);
        assert!(retry.header.is_some());
        assert!(retry.indexes.is_some());
    }

    #[test]
    fn open_missing_file_creates_fresh() {
        let path = std::env::temp_dir().join("om_mapfmt_does_not_exist_xyz.map");
        let _ = std::fs::remove_file(&path);
        let m = MapStore::open(&path, 64, 64).unwrap();
        assert_eq!(m.width(), 64);
        assert_eq!(m.allocated_blocks(), 0);
        assert!(m.is_dirty());
    }

    #[test]
    fn empty_world_serializes_to_header_plus_index_only() {
        let m = MapStore::new(64, 64).unwrap(); // 2x2 -> 4 индекса, 0 блоков
        let bytes = m.serialize();
        assert_eq!(bytes.len(), HEADER_BYTES + 4 * 4);
        let back = MapStore::deserialize(&bytes).unwrap();
        assert_eq!(back.allocated_blocks(), 0);
    }
}
