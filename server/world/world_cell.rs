use super::CellType;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldCell {
    pub cell_type: CellType,
    pub durability: f32,
}
