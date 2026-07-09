#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct WorldPos(pub i32, pub i32);

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct ChunkPos(pub u32, pub u32);

/// Смещение по сетке для направления игрока (0-3), как в legacy Unity client.
#[must_use]
pub const fn dir_offset(dir: i32) -> (i32, i32) {
    match dir {
        0 => (0, 1),  // down
        1 => (-1, 0), // left
        2 => (0, -1), // up
        3 => (1, 0),  // right
        _ => (0, 0),
    }
}

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
