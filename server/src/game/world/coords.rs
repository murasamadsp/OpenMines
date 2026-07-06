#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorldPos(pub i32, pub i32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChunkPos(pub u32, pub u32);

impl From<(i32, i32)> for WorldPos {
    fn from(value: (i32, i32)) -> Self {
        Self(value.0, value.1)
    }
}

impl From<WorldPos> for (i32, i32) {
    fn from(value: WorldPos) -> Self {
        (value.0, value.1)
    }
}

impl From<(u32, u32)> for ChunkPos {
    fn from(value: (u32, u32)) -> Self {
        Self(value.0, value.1)
    }
}

impl From<ChunkPos> for (u32, u32) {
    fn from(value: ChunkPos) -> Self {
        (value.0, value.1)
    }
}
