use std::sync::Arc;

use super::super::cells::{CellDefs, CellType};
use super::super::world_cell::WorldCell;
use super::layer::WorldFlushStats;

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
    fn destroy_cell_and_road(&self, x: i32, y: i32);
    fn damage_cell(&self, x: i32, y: i32, dmg: f32) -> bool;
    fn read_chunk_cells(&self, chunk_x: u32, chunk_y: u32) -> Vec<u8>;
    fn flush(&self) -> anyhow::Result<WorldFlushStats>;
    fn is_empty(&self, x: i32, y: i32) -> bool;
}
