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
pub use cells::{CellDefs, CellType};
pub mod world_cell;
pub use world_cell::WorldCell;
pub mod generator;
pub mod map_format;
mod sector_palette;
mod sectors_gen;

pub mod world;

pub use world::World;
pub use world::journal::{JournalRecord, WorldJournal};
pub use world::layer::{Layer, LayerFlushStats, LayerType, WorldFlushStats};
pub use world::provider::WorldProvider;

pub const CHUNK_SIZE: u32 = 32;
pub const JOURNAL_MAGIC: [u8; 4] = *b"OWJ1";
pub const JOURNAL_RECORD_LEN: usize = 18;

/// Значение пустой/выкопанной клетки (как видит клиент по проводу).
pub const EMPTY_CELL: u8 = cells::cell_type::EMPTY;

/// При 60s-цикле flush: бэкап ≈ раз в 30 мин (msync остаётся каждые 60s).
pub const BACKUP_EVERY_N_FLUSHES: u64 = 30;

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

        world.set_cell_typed(10, 10, CellType(cells::cell_type::ROAD));

        let read = world.read_world_cell(10, 10).unwrap();
        assert_eq!(read.cell_type, CellType(cells::cell_type::ROAD));

        world.set_cell_typed(11, 10, CellType(cells::cell_type::GREEN));
        assert_eq!(
            world.get_cell_typed(11, 10),
            CellType(cells::cell_type::GREEN)
        );

        world.set_cell_typed(12, 10, CellType(cells::cell_type::ROAD));
        world.set_cell_typed(12, 10, CellType(cells::cell_type::ROCK));
        world.destroy_cell_and_road(12, 10);
        assert_eq!(world.get_solid_cell(12, 10), 0);
        assert_eq!(world.get_road_cell(12, 10), cells::cell_type::EMPTY);

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

        world.set_cell_typed(10, 10, CellType(cells::cell_type::ROAD));
        assert_eq!(world.get_cell(10, 10), cells::cell_type::ROAD);
        assert_eq!(world.get_road_cell(10, 10), cells::cell_type::ROAD);
        assert_eq!(world.get_solid_cell(10, 10), 0);

        world.set_cell_typed(10, 10, CellType(cells::cell_type::ALIVE_CYAN));
        assert_eq!(world.get_cell(10, 10), cells::cell_type::ALIVE_CYAN);
        assert_eq!(world.get_solid_cell(10, 10), cells::cell_type::ALIVE_CYAN);
        assert_eq!(world.get_road_cell(10, 10), 0);

        world.destroy(10, 10);
        assert_eq!(world.get_cell(10, 10), cells::cell_type::ROAD);
        assert_eq!(world.get_road_cell(10, 10), cells::cell_type::ROAD);
        assert_eq!(world.get_solid_cell(10, 10), 0);
        assert_eq!(
            world.read_chunk_cells(0, 0)[10 + 10 * CHUNK_SIZE as usize],
            cells::cell_type::ROAD
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

        world.set_cell_typed(0, 0, CellType(cells::cell_type::EMPTY));
        world.set_cell_typed(1, 1, CellType(cells::cell_type::ROAD));
        world.set_cell_typed(2, 1, CellType(cells::cell_type::GREEN));
        world.set_cell_typed(3, 0, CellType(cells::cell_type::EMPTY));

        let snapshot = world.snapshot_cells_rect(0, 0, 4, 3);
        assert_eq!(snapshot.len(), 12);
        assert_eq!(snapshot[1 + 4], Some(CellType(cells::cell_type::ROAD)));
        assert_eq!(snapshot[2 + 4], Some(CellType(cells::cell_type::GREEN)));
        assert_eq!(snapshot[3], Some(CellType(cells::cell_type::EMPTY)));

        let oob_snapshot = world.snapshot_cells_rect(-1, -1, 2, 2);
        assert_eq!(
            oob_snapshot,
            vec![None, None, None, Some(CellType(cells::cell_type::EMPTY))]
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
            world.set_cell_typed(10, 10, CellType(cells::cell_type::ROAD));
            world.set_cell_typed(10, 10, CellType(cells::cell_type::ALIVE_CYAN));
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
        assert_eq!(reopened.get_cell(10, 10), cells::cell_type::ALIVE_CYAN);
        assert_eq!(
            reopened.get_solid_cell(10, 10),
            cells::cell_type::ALIVE_CYAN
        );
        assert_eq!(reopened.get_road_cell(10, 10), 0);
        assert!((reopened.get_durability(10, 10) - 12.0).abs() < f32::EPSILON);
        reopened.destroy(10, 10);
        assert_eq!(reopened.get_cell(10, 10), cells::cell_type::ROAD);

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

        world.set_cell_typed(12, 10, CellType(cells::cell_type::GREEN));
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

    #[test]
    fn detached_map_flush_releases_read_locks_and_restores_on_drop() {
        let temp_dir = std::env::temp_dir();
        let name = format!(
            "test_world_detached_flush_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
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
        world.cells.write().set_cell(5, 5, cells::cell_type::GREEN);

        let detached = world::DetachedMapFlush::take(&world);
        assert!(world.cells.try_read().is_some());
        assert!(world.road.try_read().is_some());
        assert!(!world.cells.read().is_dirty());

        drop(detached);
        assert!(world.cells.read().is_dirty());

        drop(world);
        let _ = std::fs::remove_file(temp_dir.join(format!("{name}_v2.map")));
        let _ = std::fs::remove_file(temp_dir.join(format!("{name}_road_v2.map")));
        let _ = std::fs::remove_file(temp_dir.join(format!("{name}_durability.map")));
        let _ = std::fs::remove_file(temp_dir.join(format!("{name}_world.journal")));
    }

    #[test]
    fn failed_world_flush_restores_map_dirty_and_preserves_journal() {
        let temp_dir = std::env::temp_dir();
        let name = format!(
            "test_world_failed_flush_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
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
        let mut world = World::new(&name, 1, 1, cell_defs, &temp_dir).unwrap();
        world.set_cell_typed(5, 5, CellType(cells::cell_type::GREEN));
        let journal_len = std::fs::metadata(&journal_path).unwrap().len();
        assert!(journal_len > 0);

        let map_path = world.map_path.clone();
        world.map_path = temp_dir
            .join(format!("{name}_missing_parent"))
            .join("world.map");
        assert!(world.flush().is_err());
        assert!(world.cells.read().is_dirty());
        assert_eq!(std::fs::metadata(&journal_path).unwrap().len(), journal_len);

        world.map_path = map_path;
        world.flush().unwrap();
        assert!(!world.cells.read().is_dirty());
        assert_eq!(std::fs::metadata(&journal_path).unwrap().len(), 0);

        drop(world);
        let _ = std::fs::remove_file(temp_dir.join(format!("{name}_v2.map")));
        let _ = std::fs::remove_file(temp_dir.join(format!("{name}_road_v2.map")));
        let _ = std::fs::remove_file(temp_dir.join(format!("{name}_durability.map")));
        let _ = std::fs::remove_file(journal_path);
    }
}
