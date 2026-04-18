//! Пороги глубины и наборы `types` / `crys` из `server_reference/.../Sector.cs` (`GenerateInsides`).
//! `CellType.GRock` в enum референса битый (=112); в `cells.json` тип **122** — здесь везде **122**.

use crate::world::cells::cell_type;

/// Индекс тира по `depth` (= минимальный Y сектора), как `switch (depth)` в референсе.
#[inline]
pub const fn depth_tier(depth_y: u32) -> usize {
    match depth_y {
        0..500 => 0,
        500..1000 => 1,
        1000..2000 => 2,
        2000..3000 => 3,
        3000..4000 => 4,
        4000..6000 => 5,
        6000..7000 => 6,
        7000..8000 => 7,
        8000..10000 => 8,
        10000..11000 => 9,
        11000..13000 => 10,
        13000..15000 => 11,
        15000..17000 => 12,
        17000..18000 => 13,
        _ => 14,
    }
}

/// Ветка `types` в референсе (без `gig`).
#[allow(clippy::too_many_lines)]
pub const fn types_palette(tier: usize) -> &'static [u8] {
    match tier {
        0 => &[
            cell_type::YELLOW_SAND,
            cell_type::ROCK,
            cell_type::DARK_YELLOW_SAND,
            cell_type::HEAVY_ROCK,
            cell_type::ROCK,
            cell_type::LAVA,
        ],
        1 => &[
            cell_type::ROCK,
            cell_type::HEAVY_ROCK,
            cell_type::YELLOW_SAND,
            cell_type::DARK_YELLOW_SAND,
            cell_type::LAVA,
            cell_type::BOULDER1,
            cell_type::BOULDER2,
            cell_type::BOULDER3,
        ],
        2 | 3 => &[
            cell_type::ROCK,
            cell_type::HEAVY_ROCK,
            cell_type::BOULDER1,
            cell_type::BOULDER2,
            cell_type::BOULDER3,
            cell_type::BLUE_SAND,
            cell_type::DARK_BLUE_SAND,
            cell_type::LAVA,
        ],
        4 => &[
            cell_type::ROCK,
            cell_type::HEAVY_ROCK,
            cell_type::BOULDER1,
            cell_type::BOULDER2,
            cell_type::BOULDER3,
            cell_type::BLUE_SAND,
            cell_type::DARK_BLUE_SAND,
            cell_type::WHITE_SAND,
            cell_type::DARK_WHITE_SAND,
            cell_type::LAVA,
        ],
        5 => &[
            cell_type::ROCK,
            cell_type::HEAVY_ROCK,
            cell_type::GOLDEN_ROCK,
            cell_type::BOULDER1,
            cell_type::BOULDER2,
            cell_type::BOULDER3,
            cell_type::BLUE_SAND,
            cell_type::DARK_BLUE_SAND,
            cell_type::WHITE_SAND,
            cell_type::DARK_WHITE_SAND,
            cell_type::LAVA,
        ],
        6 => &[
            cell_type::ROCK,
            cell_type::HEAVY_ROCK,
            cell_type::GOLDEN_ROCK,
            cell_type::BOULDER1,
            cell_type::BOULDER2,
            cell_type::BOULDER3,
            cell_type::BLUE_SAND,
            cell_type::DARK_BLUE_SAND,
            cell_type::WHITE_SAND,
            cell_type::DARK_WHITE_SAND,
            cell_type::LAVA,
            cell_type::PASSIVE_ACID,
        ],
        7 => &[
            cell_type::HEAVY_ROCK,
            cell_type::GOLDEN_ROCK,
            cell_type::BOULDER1,
            cell_type::BOULDER2,
            cell_type::BOULDER3,
            cell_type::BLUE_SAND,
            cell_type::DARK_BLUE_SAND,
            cell_type::WHITE_SAND,
            cell_type::DARK_WHITE_SAND,
            cell_type::LAVA,
            cell_type::PASSIVE_ACID,
        ],
        8 => &[
            cell_type::HEAVY_ROCK,
            cell_type::GOLDEN_ROCK,
            cell_type::BOULDER1,
            cell_type::BOULDER2,
            cell_type::BOULDER3,
            cell_type::WHITE_SAND,
            cell_type::DARK_WHITE_SAND,
            cell_type::RUSTY_SAND,
            cell_type::DARK_RUSTY_SAND,
            cell_type::PASSIVE_ACID,
        ],
        9 | 10 => &[
            cell_type::DEEP_ROCK,
            cell_type::GOLDEN_ROCK,
            cell_type::BLACK_BOULDER1,
            cell_type::BLACK_BOULDER2,
            cell_type::BLACK_BOULDER3,
            cell_type::WHITE_SAND,
            cell_type::DARK_WHITE_SAND,
            cell_type::RUSTY_SAND,
            cell_type::DARK_RUSTY_SAND,
            cell_type::PASSIVE_ACID,
        ],
        11 => &[
            cell_type::DEEP_ROCK,
            cell_type::GOLDEN_ROCK,
            cell_type::BLACK_BOULDER1,
            cell_type::BLACK_BOULDER2,
            cell_type::BLACK_BOULDER3,
            cell_type::WHITE_SAND,
            cell_type::DARK_WHITE_SAND,
            cell_type::RUSTY_SAND,
            cell_type::DARK_RUSTY_SAND,
            cell_type::GRAY_ACID,
        ],
        12 | 13 => &[
            cell_type::DEEP_ROCK,
            cell_type::G_ROCK,
            cell_type::BLACK_BOULDER1,
            cell_type::BLACK_BOULDER2,
            cell_type::BLACK_BOULDER3,
            cell_type::WHITE_SAND,
            cell_type::DARK_WHITE_SAND,
            cell_type::RUSTY_SAND,
            cell_type::DARK_RUSTY_SAND,
            cell_type::GRAY_ACID,
        ],
        _ => &[
            cell_type::G_ROCK,
            cell_type::METAL_BOULDER1,
            cell_type::METAL_BOULDER2,
            cell_type::METAL_BOULDER3,
            cell_type::WHITE_SAND,
            cell_type::DARK_WHITE_SAND,
            cell_type::RUSTY_SAND,
            cell_type::DARK_RUSTY_SAND,
            cell_type::GRAY_ACID,
            cell_type::PEARL,
        ],
    }
}

/// Ветка `crys` — второй аргумент `gig` совпадает с `gig ? … : …` в референсе.
#[allow(clippy::too_many_lines)]
pub const fn crys_palette(tier: usize, gig: bool) -> &'static [u8] {
    match tier {
        0 => &[cell_type::GREEN, cell_type::BLUE],
        1 => {
            if gig {
                &[
                    cell_type::GREEN,
                    cell_type::BLUE,
                    cell_type::X_BLUE,
                    cell_type::X_GREEN,
                ]
            } else {
                &[cell_type::GREEN, cell_type::BLUE, cell_type::X_BLUE]
            }
        }
        2 => {
            if gig {
                &[
                    cell_type::BLUE,
                    cell_type::X_BLUE,
                    cell_type::GREEN,
                    cell_type::X_GREEN,
                ]
            } else {
                &[cell_type::GREEN, cell_type::BLUE, cell_type::X_BLUE]
            }
        }
        3 => &[
            cell_type::BLUE,
            cell_type::X_BLUE,
            cell_type::RED,
            cell_type::X_RED,
        ],
        4 => {
            if gig {
                &[
                    cell_type::BLUE,
                    cell_type::X_BLUE,
                    cell_type::RED,
                    cell_type::X_RED,
                ]
            } else {
                &[cell_type::RED, cell_type::BLUE]
            }
        }
        5 => {
            if gig {
                &[
                    cell_type::RED,
                    cell_type::VIOLET,
                    cell_type::X_RED,
                    cell_type::X_VIOLET,
                ]
            } else {
                &[cell_type::RED, cell_type::VIOLET, cell_type::X_RED]
            }
        }
        6 => {
            if gig {
                &[cell_type::X_VIOLET]
            } else {
                &[cell_type::X_VIOLET, cell_type::VIOLET]
            }
        }
        7 => {
            if gig {
                &[cell_type::X_VIOLET]
            } else {
                &[cell_type::VIOLET, cell_type::VIOLET, cell_type::WHITE]
            }
        }
        8 => {
            if gig {
                &[cell_type::X_CYAN, cell_type::CYAN, cell_type::WHITE]
            } else {
                &[cell_type::CYAN, cell_type::WHITE, cell_type::X_CYAN]
            }
        }
        9 => {
            if gig {
                &[cell_type::X_BLUE, cell_type::X_CYAN]
            } else {
                &[
                    cell_type::CYAN,
                    cell_type::X_BLUE,
                    cell_type::X_GREEN,
                    cell_type::X_CYAN,
                ]
            }
        }
        10 => &[cell_type::X_GREEN, cell_type::X_BLUE, cell_type::X_VIOLET],
        11 => &[cell_type::X_RED, cell_type::WHITE],
        12 => &[cell_type::X_GREEN, cell_type::X_CYAN],
        13 => &[cell_type::X_RED, cell_type::X_VIOLET, cell_type::X_CYAN],
        _ => &[
            cell_type::X_GREEN,
            cell_type::X_BLUE,
            cell_type::X_RED,
            cell_type::X_VIOLET,
            cell_type::X_CYAN,
        ],
    }
}

/// Слияние `types` + `crys` без дублей (до 64 байт) — замена случайной подвыборки `GenerateInsides`.
pub fn merged_palette_buf(tier: usize, gig: bool) -> ([u8; 64], usize) {
    let mut buf = [0u8; 64];
    let mut n = 0usize;
    let push = |b: &mut [u8; 64], nn: &mut usize, v: u8| {
        if *nn < b.len() && !b[..*nn].contains(&v) {
            b[*nn] = v;
            *nn += 1;
        }
    };
    for &t in types_palette(tier) {
        push(&mut buf, &mut n, t);
    }
    for &c in crys_palette(tier, gig) {
        push(&mut buf, &mut n, c);
    }
    (buf, n)
}
