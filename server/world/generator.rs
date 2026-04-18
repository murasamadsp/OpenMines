//! Генератор мира: ridged «скелет» как в референсе, затем заливка **секторов**
//! (BFS по пустотам ≥50 клеток) палитрой из `Sector.GenerateInsides` без тяжёлого `SectorFiller`.

#![allow(
    clippy::many_single_char_names,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]

use super::sector_palette;
use rayon::prelude::*;
use std::collections::VecDeque;

const CHUNK_SZ: u32 = 32;

/// Верхние ряды — полностью чистые (зона спауна).
const SPAWN_CLEAR_ROWS: u32 = 50;

const BLACK_ROCK: u8 = 114;
const RED_ROCK: u8 = 117;

/// Как `DetectAndFillSectors`: каверны меньше этого размера не наполняются.
const SECTOR_MIN_CELLS: usize = 50;

/// Доля клеток, остающихся пустыми (аналог старого `> 0.28` → ~72% с контентом).
const CAVITY_SKIP: f32 = 0.28;

// ── Хэш (как раньше) ──────────────────────────────────────────────────────────

#[inline]
const fn hash2(x: u32, y: u32, seed: u32) -> u32 {
    let mut h = x.wrapping_mul(0x27d4_eb2d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85eb_ca6b);
    h = h.wrapping_add(y.wrapping_mul(0xc2b2_ae35));
    h ^= h >> 13;
    h = h.wrapping_mul(0x27d4_eb2d);
    h ^= h >> 16;
    h = h.wrapping_add(seed.wrapping_mul(0x9e37_79b1));
    h ^ (h >> 16)
}

#[inline]
#[allow(clippy::cast_precision_loss)]
fn hash_f01(x: u32, y: u32, seed: u32) -> f32 {
    (hash2(x, y, seed) as f32) * (1.0 / u32::MAX as f32)
}

#[inline]
fn smoothstep(t: f32) -> f32 {
    let inner = (-2.0_f32).mul_add(t, 3.0);
    t * t * inner
}

#[inline]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn value_noise(noise_x: f32, noise_y: f32, seed: u32) -> f32 {
    let xf = noise_x.floor();
    let yf = noise_y.floor();
    let tx = smoothstep(noise_x - xf);
    let ty = smoothstep(noise_y - yf);
    let ix = xf as i32 as u32;
    let iy = yf as i32 as u32;
    let h00 = hash_f01(ix, iy, seed);
    let h10 = hash_f01(ix.wrapping_add(1), iy, seed);
    let h01 = hash_f01(ix, iy.wrapping_add(1), seed);
    let h11 = hash_f01(ix.wrapping_add(1), iy.wrapping_add(1), seed);
    let ab = (h10 - h00).mul_add(tx, h00);
    let cd = (h11 - h01).mul_add(tx, h01);
    (cd - ab).mul_add(ty, ab)
}

fn ridged(pos_x: f32, pos_y: f32, seed: u32, octaves: u32, freq: f32) -> f32 {
    let mut amp = 1.0f32;
    let mut frequency = freq;
    let mut sum = 0.0f32;
    let mut norm = 0.0f32;
    for octave in 0..octaves {
        let noise_val = value_noise(
            pos_x * frequency,
            pos_y * frequency,
            seed.wrapping_add(octave * 7),
        );
        let ridge_val = 1.0 - (noise_val * 2.0 - 1.0).abs();
        sum += ridge_val * ridge_val * amp;
        norm += amp;
        amp *= 0.5;
        frequency *= 2.0;
    }
    sum / norm
}

/// Маска «скала / пустота»: как в `Sectors.GenerateENoise` — ridged в **нормированных**
/// координатах `(x/width, y/height)` и один порог после нормализации (у нас без второго
/// прохода min/max по всему миру — фиксированный порог ≈ `mid+res` из референса).
///
/// Раньше здесь было OR трёх ridged по **пиксельным** координатам с жёсткими порогами —
/// почти вся карта становилась 114/117, пустоты дробились на куски <50 клеток и
/// `fill_sectors` не наполнял сектора песком/кристаллами.
#[inline]
fn is_rock(x: u32, y: u32, w: u32, h: u32, seed: u32) -> bool {
    #[allow(clippy::cast_precision_loss)]
    let nx = x as f32 / w.max(1) as f32;
    #[allow(clippy::cast_precision_loss)]
    let ny = y as f32 / h.max(1) as f32;
    // Частота 25 и 1 октава — как первый `ImplicitFractal` в `Sectors.GenerateENoise`.
    let v = ridged(nx, ny, seed, 1, 25.0);
    v >= 0.55
}

/// Как `chs(y)` в `Sectors.cs` (без искусственного пола — при большой глубине шанс 114 → 0).
#[inline]
#[allow(
    clippy::cast_precision_loss,
    clippy::suboptimal_flops,
    clippy::missing_const_for_fn
)]
fn chs_y(y: u32) -> f32 {
    (y as f32).mul_add(-0.0028, 30.0)
}

#[inline]
const fn cell_index_in_map(x: u32, y: u32, chunks_w: u32, chunks_h: u32) -> usize {
    let cx = x / CHUNK_SZ;
    let cy = y / CHUNK_SZ;
    debug_assert!(cx < chunks_w && cy < chunks_h);
    let lx = x % CHUNK_SZ;
    let ly = y % CHUNK_SZ;
    let chunk_idx = (cy + chunks_h * cx) as usize;
    let cell_in_chunk = (ly + CHUNK_SZ * lx) as usize;
    let chunk_start = chunk_idx * (CHUNK_SZ * CHUNK_SZ) as usize;
    chunk_start + cell_in_chunk
}

fn base_cell(x: u32, y: u32, w: u32, h: u32, seed: u32) -> u8 {
    if y < SPAWN_CLEAR_ROWS {
        return 0;
    }
    if is_rock(x, y, w, h, seed) {
        let ch = chs_y(y);
        let r = hash_f01(x, y, seed.wrapping_add(0x1337)) * 100.0;
        return if r < ch { BLACK_ROCK } else { RED_ROCK };
    }
    0
}

#[allow(clippy::too_many_arguments)]
fn fill_sectors(
    cells: &mut [u8],
    dur: &mut [u8],
    w: u32,
    h: u32,
    chunks_w: u32,
    chunks_h: u32,
    seed: u32,
    cell_dur: &[f32; 256],
) {
    let flat_n = (w * h) as usize;
    let mut visited = vec![false; flat_n];
    let mut sector_seq: u32 = 0;

    for y in 0..h {
        for x in 0..w {
            let idx = cell_index_in_map(x, y, chunks_w, chunks_h);
            if visited[idx] {
                continue;
            }
            if cells[idx] != 0 {
                continue;
            }

            let mut queue = VecDeque::new();
            let mut comp = Vec::new();
            let mut min_y = y;
            queue.push_back((x, y));
            visited[idx] = true;

            while let Some((cx, cy)) = queue.pop_front() {
                min_y = min_y.min(cy);
                comp.push((cx, cy));
                for (dx, dy) in [(0i32, 1), (0, -1), (-1, 0), (1, 0)] {
                    let nx = cx as i32 + dx;
                    let ny = cy as i32 + dy;
                    if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                        continue;
                    }
                    let nx = nx as u32;
                    let ny = ny as u32;
                    let nidx = cell_index_in_map(nx, ny, chunks_w, chunks_h);
                    if visited[nidx] {
                        continue;
                    }
                    if cells[nidx] != 0 {
                        continue;
                    }
                    visited[nidx] = true;
                    queue.push_back((nx, ny));
                }
            }

            if comp.len() < SECTOR_MIN_CELLS {
                continue;
            }

            sector_seq = sector_seq.wrapping_add(1);
            let depth = min_y;
            let tier = sector_palette::depth_tier(depth);
            let gig_inner = hash2(seed.wrapping_add(sector_seq), comp[0].0, comp[0].1) % 100 >= 80;

            let palette: Vec<u8> = if gig_inner {
                sector_palette::crys_palette(tier, gig_inner).to_vec()
            } else {
                let (buf, n) = sector_palette::merged_palette_buf(tier, gig_inner);
                buf[..n].to_vec()
            };

            if palette.is_empty() {
                continue;
            }
            let plen = palette.len();

            for (cx, cy) in comp {
                let skip = hash_f01(cx, cy, seed.wrapping_add(0xdead_beef));
                if skip > CAVITY_SKIP {
                    continue;
                }
                let pick = (hash2(cx, cy, seed.wrapping_add(0xbeef_dead)) as usize) % plen;
                let cell = palette[pick];
                let cidx = cell_index_in_map(cx, cy, chunks_w, chunks_h);
                cells[cidx] = cell;
                let d = cell_dur[cell as usize];
                let db = d.to_le_bytes();
                let doff = cidx * 4;
                dur[doff..doff + 4].copy_from_slice(&db);
            }
        }
    }
}

pub fn generate(world: &super::World, seed: u64) {
    let (chunks_w, chunks_h, cs) = world.chunks_layout();
    debug_assert_eq!(cs, CHUNK_SZ);
    let w = chunks_w * CHUNK_SZ;
    let h = chunks_h * CHUNK_SZ;
    let cells_per_chunk = (cs * cs) as usize;
    let total_chunks = (chunks_w as usize) * (chunks_h as usize);
    let total_cells = total_chunks * cells_per_chunk;

    #[allow(clippy::cast_possible_truncation)]
    let seed32 = seed as u32;

    let mut cell_dur_table = [0.0f32; 256];
    for (i, def) in world.cell_defs.cells.iter().enumerate() {
        if i < cell_dur_table.len() {
            cell_dur_table[i] = def.durability;
        }
    }

    tracing::info!(
        "World generator: ridged base + sector fill — {}x{} cells ({} chunks, seed={seed})",
        w,
        h,
        total_chunks
    );

    let t0 = std::time::Instant::now();

    world.with_generation_layers(|cells_mmap, dur_mmap| {
        cells_mmap
            .par_chunks_exact_mut(cells_per_chunk)
            .zip(dur_mmap.par_chunks_exact_mut(cells_per_chunk * 4))
            .enumerate()
            .for_each(|(ci, (cells_slice, dur_slice))| {
                #[allow(clippy::cast_possible_truncation)]
                let ci32 = ci as u32;
                let cx = ci32 / chunks_h;
                let cy = ci32 % chunks_h;
                let base_x = cx * cs;
                let base_y = cy * cs;
                for lx in 0..cs {
                    for ly in 0..cs {
                        let x = base_x + lx;
                        let y = base_y + ly;
                        let cell = base_cell(x, y, w, h, seed32);
                        let idx = (ly + cs * lx) as usize;
                        cells_slice[idx] = cell;
                        let d = cell_dur_table[cell as usize];
                        let bytes = d.to_le_bytes();
                        let off = idx * 4;
                        dur_slice[off..off + 4].copy_from_slice(&bytes);
                    }
                }
            });

        fill_sectors(
            cells_mmap,
            dur_mmap,
            w,
            h,
            chunks_w,
            chunks_h,
            seed32,
            &cell_dur_table,
        );
    });

    tracing::info!(
        "World generator: finished ({:?}, ~{} MiB layers)",
        t0.elapsed(),
        (total_cells + total_cells * 4) / (1024 * 1024)
    );
}
