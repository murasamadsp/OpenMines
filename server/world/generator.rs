//! Генерация мира: Sectors («скелет» через `sectors_gen`) + заливка секторов
//! (`SectorFiller`). 1:1 с C# `Gen.cs` + `SectorFiller.cs` + `Sector.cs`.
//!
//! Шум и случайность — настоящие: `super::anl` (порт AccidentalNoise) и
//! `super::dotnet_random` (порт .NET `System.Random`). Несколько мест C#
//! недетерминированы (`new Random()` = `Environment.TickCount`) или
//! багованы (формула `swidth`/`sheight` в `DetectAndFillSectors`) — они
//! воспроизведены дословно, но засеяны детерминированно от нашего seed.

#![allow(
    clippy::many_single_char_names,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::too_many_arguments,
    clippy::similar_names,
    clippy::module_name_repetitions,
    clippy::doc_markdown,
    clippy::needless_range_loop,
    clippy::cast_lossless,
    clippy::collapsible_if,
    clippy::redundant_guards,
    clippy::explicit_iter_loop,
    clippy::doc_lazy_continuation,
    // `while v == 0.0` — дословный `while (v == 0)` из C# FillNoiseToSector.
    clippy::while_float
)]

use super::anl::{BasisType, FractalType, ImplicitFractal, InterpolationType};
use super::dotnet_random::DotNetRandom;
use super::sector_palette;
use super::sectors_gen;
use rayon::prelude::*;
use std::collections::VecDeque;

const CHUNK_SZ: u32 = 32;
const SPAWN_CLEAR_ROWS: u32 = 50;
const SECTOR_MIN_CELLS: usize = 50;
const EMPTY: u8 = 0;
/// `count > 40000` → `CreateFillForCells(gig=false)`, иначе `gig=true`.
const GIG_SIZE_THRESHOLD: usize = 40000;
/// Жёсткий предел повторов `CreateFillForCells` — в C# цикл на `goto` без
/// предела (на практике сходится за 1-2 итерации); ставим страховку.
const MAX_FILL_ATTEMPTS: u32 = 200;

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

/// `Sector.GenerateInsides()` — 1:1. Внутренний `gig` (20%) определяет:
/// только кристаллы, либо случайное подмножество types + crys.
fn generate_insides(tier: usize, rng: &mut DotNetRandom) -> Vec<u8> {
    // var gig = r.Next(1, 101) >= 80
    let gig = rng.next_range(1, 101) >= 80;
    let types = sector_palette::types_palette(tier);
    let crys = sector_palette::crys_palette(tier, gig);

    if gig {
        // re = re.Concat(crys) — порядок crys, без дублей (Concat не дедуплит,
        // но crys-палитры уникальны; на всякий случай дедуплим как Append-петли).
        let mut result: Vec<u8> = Vec::new();
        for &c in crys {
            result.push(c);
        }
        return result;
    }

    // var lenm = r.Next(1, types.Length); var lencry = r.Next(1, crys.Length);
    let lenm = rng.next_range(1, types.len() as i32) as usize;
    let lencry = rng.next_range(1, crys.len() as i32) as usize;

    let mut result: Vec<u8> = Vec::new();

    // for (i=0; i<lenm; i++) { j=types[Next(0,len)]; if !contains push; else i--; }
    // i-- = повтор итерации до уникального; страховка от зацикливания при
    // палитрах с дублями (unique < lenm невозможно при lenm<len, но cap есть).
    let mut got = 0usize;
    let mut guard = 0u32;
    while got < lenm && guard < 10_000 {
        guard += 1;
        let j = types[rng.next_range(0, types.len() as i32) as usize];
        if !result.contains(&j) {
            result.push(j);
            got += 1;
        }
    }
    got = 0;
    guard = 0;
    while got < lencry && guard < 10_000 {
        guard += 1;
        let j = crys[rng.next_range(0, crys.len() as i32) as usize];
        if !result.contains(&j) {
            result.push(j);
            got += 1;
        }
    }

    result
}

/// `SectorFiller.NotTypedNoise()` — случайный фрактал. 1:1: порядок rng-вызовов
/// type/basis/interp/octaves/freq/lacunarity. C# не задаёт `Seed` (basis
/// сидится `DateTime.Now.Ticks` — недетерминированно); мы засеваем фрактал
/// одним дополнительным `next()`-вызовом ради детерминированного разнообразия
/// шума между секторами.
fn not_typed_noise(rng: &mut DotNetRandom) -> ImplicitFractal {
    let ftype = rng.next_range(0, 5);
    let basis = rng.next_range(0, 4);
    let interp = rng.next_range(0, 4);
    let octaves = rng.next_range(4, 20);
    let freq = rng.next_range(4, 20);
    let lac = rng.next_range(1, 20);

    let mut fr = ImplicitFractal::new(
        FractalType::from_i32(ftype),
        BasisType::from_i32(basis),
        InterpolationType::from_i32(interp),
    );
    fr.set_octaves(octaves);
    fr.set_frequency(freq as f64);
    fr.set_lacunarity(lac as f64);
    // Доп. сид (нет в C#) — детерминированное разнообразие между секторами.
    fr.set_seed(rng.next());
    fr
}

/// `RandomSizedParts(args)` — 1:1. Делит [0,~2] на сегменты `(start, end)`,
/// хранимые НЕсортированно; проверка пересечения `s <= end && e >= start`.
fn random_sized_parts(rng: &mut DotNetRandom, args: &[u8]) -> Vec<(u8, f32, f32)> {
    let mut parts: Vec<(u8, f32, f32)> = Vec::new();
    for &d in args {
        let mut guard = 0u32;
        loop {
            let start = rng.next_double() as f32;
            let end = start + rng.next_double() as f32;
            let overlap = parts.iter().any(|&(_, s, e)| s <= end && e >= start);
            if !overlap {
                parts.push((d, start, end));
                break;
            }
            guard += 1;
            // C# может зациклиться; cap + принудительная вставка во избежание зависания.
            if guard >= 1000 {
                parts.push((d, start, end));
                break;
            }
        }
    }
    parts
}

/// `FillNoiseToSector(s)` — 1:1. Создаёт свежий `NotTypedNoise`, считает шум по
/// клеткам, нормализует позже. `x==0?100`, делитель = габариты сектора.
/// Возвращает `(значения, min, max)`. NaN-проверка в C# мертва (`v==NaN` всегда
/// false), реально цикл срабатывает только на `v==0`.
fn fill_noise_to_sector(
    rng: &mut DotNetRandom,
    comp: &[(u32, u32)],
    s_width: i32,
    s_height: i32,
) -> (Vec<f32>, f32, f32) {
    let fr = not_typed_noise(rng);

    let widthx = if s_width == 0 { 100 } else { s_width } as f32;
    let heighty = if s_height == 0 { 100 } else { s_height } as f32;

    let g0 = fr.get(0.0, 0.0) as f32;
    let mut max = g0;
    let mut min = g0;

    let mut local_off_x = rng.next_double();
    let mut local_off_y = rng.next_double();

    let mut vals = Vec::with_capacity(comp.len());
    for &(px, py) in comp {
        let x = if px == 0 { 100 } else { px as i32 } as f32;
        let y = if py == 0 { 100 } else { py as i32 } as f32;

        let mut v = fr.get((x / widthx) as f64, (y / heighty) as f64) as f32;
        let mut guard = 0u32;
        // while (v == 0)  (NaN-ветка в C# недостижима)
        while v == 0.0 {
            local_off_x += rng.next_double();
            local_off_y += rng.next_double();
            if local_off_x > x as f64 {
                local_off_x = 0.0;
            }
            if local_off_y > y as f64 {
                local_off_y = 0.0;
            }
            v = fr.get(
                ((x + local_off_x as f32) / widthx) as f64,
                ((y + local_off_y as f32) / heighty) as f64,
            ) as f32;
            guard += 1;
            if guard >= 64 {
                break;
            }
        }

        max = if max < v { v } else { max };
        min = if min < v { min } else { v };
        vals.push(v);
    }

    (vals, min, max)
}

/// `SampleAndFindTypes(s, parts, data)` — 1:1, включая квирки:
/// - холостой `NotTypedNoise()` жжёт rng;
/// - нормализация `(v-min)/(max-min)` без epsilon;
/// - тип = последний part, чей диапазон содержит значение;
/// - счётчик инкрементится НА КАЖДУЮ итерацию по parts (а не на клетку).
/// Мутирует `values` (нормализует) и `types`; возвращает `(тип, счётчик)`.
fn sample_and_find_types(
    rng: &mut DotNetRandom,
    values: &mut [f32],
    types: &mut [u8],
    parts: &[(u8, f32, f32)],
    min: f32,
    max: f32,
) -> Vec<(u8, usize)> {
    // var fr = NotTypedNoise(); — создаётся и НЕ используется (жжёт rng).
    let _ = not_typed_noise(rng);

    let range = max - min;
    let mut counts: Vec<(u8, usize)> = Vec::new();

    for idx in 0..values.len() {
        values[idx] = (values[idx] - min) / range;
        let v = values[idx];
        for &(key, s, e) in parts {
            if v >= s && v <= e {
                types[idx] = key;
            }
            let t = types[idx];
            if let Some(p) = counts.iter_mut().find(|(tt, _)| *tt == t) {
                p.1 += 1;
            } else {
                counts.push((t, 1));
            }
        }
    }

    counts
}

/// `alive(x)` из `SectorFiller` — целочисленная формула 1:1.
#[inline]
const fn alive(x: i32) -> i32 {
    40 + (((85 - 40) * (x - 50000)) / (5000 - 50000))
}

/// `SectorFiller.CreateFillForCells(s, gig, args)` — 1:1, включая поток
/// `goto restart` / `goto refillnoise` через счётчики.
fn fill_sector(
    cells: &mut [u8],
    dur: &mut [u8],
    comp: &[(u32, u32)],
    s_width: i32,
    s_height: i32,
    chunks_w: u32,
    chunks_h: u32,
    tier: usize,
    gig: bool,
    sector_seed: u32,
    cell_dur: &[f32; 256],
) {
    let mut rng = DotNetRandom::new(sector_seed as i32);

    // args = s.GenerateInsides() — вычисляется до CreateFillForCells.
    let args = generate_insides(tier, &mut rng);
    if args.is_empty() {
        return;
    }

    let n = comp.len();
    let mut types = vec![EMPTY; n];

    let mut segmentsmall = 0i32;
    let mut notenoughparts = 0i32;
    let mut empty = 0i32;
    let mut attempts = 0u32;

    'restart: loop {
        // restart: var parts = RandomSizedParts(args); while parts.Count<len {...}
        let mut parts = random_sized_parts(&mut rng, &args);
        while parts.len() < args.len() {
            parts = random_sized_parts(&mut rng, &args);
        }

        loop {
            attempts += 1;
            if attempts > MAX_FILL_ATTEMPTS {
                return; // страховка — сектор остаётся пустым
            }

            // refillnoise:
            let (mut vals, min, max) = fill_noise_to_sector(&mut rng, comp, s_width, s_height);
            // сбрасываем типы перед каждым SampleAndFindTypes (как свежий проход)
            for t in types.iter_mut() {
                *t = EMPTY;
            }
            let result = sample_and_find_types(&mut rng, &mut vals, &mut types, &parts, min, max);

            // if (result.Count < parts.Count)
            if result.len() < parts.len() {
                notenoughparts += 1;
                if notenoughparts > 2 {
                    notenoughparts = 0;
                    continue 'restart;
                }
                continue;
            }

            // if (result.ContainsKey(Empty) && seccells*0.4 < result[Empty])
            if let Some(&(_, ec)) = result.iter().find(|(t, _)| *t == EMPTY) {
                if (n as f64) * 0.4 < ec as f64 {
                    empty += 1;
                    if empty > 4 {
                        empty = 0;
                        continue 'restart;
                    }
                    continue;
                }
            }

            // foreach (i in result) { if (seccells/parts.Count)*0.4 > i.Value ... }
            let mut goto_restart = false;
            let mut goto_refill = false;
            for &(_, count) in &result {
                let min_expected = ((n / parts.len()) as f64) * 0.4;
                if min_expected > count as f64 {
                    segmentsmall += 1;
                    if segmentsmall > 2 {
                        segmentsmall = 0;
                        goto_restart = true;
                    } else {
                        goto_refill = true;
                    }
                    break;
                }
            }
            if goto_restart {
                continue 'restart;
            }
            if goto_refill {
                continue;
            }

            // gig: заполнить пустоты случайным типом из args + холостой alive-розыгрыш.
            if gig {
                // ft = args[rand.Next(0, args.Length - 1)]  (последний элемент недостижим)
                let ft_idx = rng.next_range(0, (args.len() as i32 - 1).max(0)) as usize;
                let ft = args[ft_idx.min(args.len() - 1)];
                for idx in 0..n {
                    if types[idx] == EMPTY {
                        types[idx] = ft;
                    }
                    // if (alive(count) > rand.Next(1,101)) { /* тело закомментировано */ }
                    let _ = alive(n as i32) > rng.next_range(1, 101);
                }
            }

            // Запись типов в карту (Empty не пишем — остаётся пусто).
            for (idx, &(cx, cy)) in comp.iter().enumerate() {
                let t = types[idx];
                if t != EMPTY {
                    let cidx = cell_index_in_map(cx, cy, chunks_w, chunks_h);
                    cells[cidx] = t;
                    let d = cell_dur[t as usize];
                    let db = d.to_le_bytes();
                    let doff = cidx * 4;
                    dur[doff..doff + 4].copy_from_slice(&db);
                }
            }
            return;
        }
    }
}

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

    // C# обходит `for y { for x }` и BFS по пустотам (value==0).
    for y in 0..h {
        for x in 0..w {
            let idx = cell_index_in_map(x, y, chunks_w, chunks_h);
            if visited[idx] || cells[idx] != EMPTY {
                continue;
            }

            // BFS + воспроизведение багованных swidth/sheight + depth=min y.
            let first_x = x as i32;
            let first_y = y as i32;
            let mut s_width = 0i32;
            let mut s_height = 0i32;
            let mut depth = y;

            let mut queue = VecDeque::new();
            let mut comp = Vec::new();
            queue.push_back((x, y));
            visited[idx] = true;

            while let Some((cx, cy)) = queue.pop_front() {
                let cx_i = cx as i32;
                let cy_i = cy as i32;
                depth = depth.min(cy);
                // swidth = swidth > (cell.x-first.x) ? swidth : (first.x-first.y)
                s_width = if s_width > (cx_i - first_x) {
                    s_width
                } else {
                    first_x - first_y
                };
                // sheight = sheight > (cell.y-first.y) ? sheight : (cell.x-first.y)
                s_height = if s_height > (cy_i - first_y) {
                    s_height
                } else {
                    cx_i - first_y
                };
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
                    if visited[nidx] || cells[nidx] != EMPTY {
                        continue;
                    }
                    visited[nidx] = true;
                    queue.push_back((nx, ny));
                }
            }

            // if (s.seccells.Count < 50) continue;
            if comp.len() < SECTOR_MIN_CELLS {
                continue;
            }

            sector_seq = sector_seq.wrapping_add(1);
            let tier = sector_palette::depth_tier(depth);
            // count > 40000 → gig=false, иначе gig=true.
            let gig = comp.len() <= GIG_SIZE_THRESHOLD;

            fill_sector(
                cells,
                dur,
                &comp,
                s_width,
                s_height,
                chunks_w,
                chunks_h,
                tier,
                gig,
                seed.wrapping_add(sector_seq).wrapping_mul(0x1337),
                cell_dur,
            );
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
        "World generator: AccidentalNoise RidgedMulti + 3-layer AddW + CleanCs + SectorFiller — {}x{} cells ({} chunks, seed={seed})",
        w,
        h,
        total_chunks
    );

    let t0 = std::time::Instant::now();

    // 1. Скелет мира (1:1 C# Sectors.cs / Gen.StartGeneration).
    let skeleton = sectors_gen::generate_skeleton(w, h, seed32);

    // 2. Заполнение flat_cells из скелета, с spawn clear zone.
    let mut flat_cells = vec![0u8; total_cells];

    world.with_durability_mmap(|dur_mmap| {
        flat_cells
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
                        let cell = if y < SPAWN_CLEAR_ROWS {
                            0
                        } else {
                            let sidx = (x * h + y) as usize;
                            skeleton.values[sidx]
                        };
                        let idx = (ly + cs * lx) as usize;
                        cells_slice[idx] = cell;
                        let d = cell_dur_table[cell as usize];
                        let bytes = d.to_le_bytes();
                        let off = idx * 4;
                        dur_slice[off..off + 4].copy_from_slice(&bytes);
                    }
                }
            });

        // 3. Заливка секторов (1:1 SectorFiller + GenerateInsides).
        fill_sectors(
            &mut flat_cells,
            dur_mmap,
            w,
            h,
            chunks_w,
            chunks_h,
            seed32,
            &cell_dur_table,
        );
    });

    world.ingest_generated_cells(&flat_cells);

    tracing::info!(
        "World generator: finished ({:?}, ~{} MiB layers)",
        t0.elapsed(),
        (total_cells + total_cells * 4) / (1024 * 1024)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Прогон полного пайплайна заливки на синтетическом скелете (без mmap):
    /// доказывает, что `SectorFiller` не виснет и реально наполняет каверны.
    #[test]
    fn fill_sectors_fills_cavities_without_hanging() {
        let chunks_w = 2u32;
        let chunks_h = 2u32;
        let w = chunks_w * CHUNK_SZ; // 64
        let h = chunks_h * CHUNK_SZ; // 64
        let total = (w * h) as usize;

        // Скелет: внешняя рамка-скала, большая пустая каверна внутри (> 50 клеток).
        let mut cells = vec![EMPTY; total];
        for x in 0..w {
            for y in 0..h {
                let edge = x < 2 || y < 2 || x >= w - 2 || y >= h - 2;
                if edge {
                    let idx = cell_index_in_map(x, y, chunks_w, chunks_h);
                    cells[idx] = 117; // RED_ROCK
                }
            }
        }
        let mut dur = vec![0u8; total * 4];
        let mut cell_dur = [1.0f32; 256];
        cell_dur[0] = 0.0;

        fill_sectors(
            &mut cells, &mut dur, w, h, chunks_w, chunks_h, 4242, &cell_dur,
        );

        // Каверна должна получить хоть какой-то контент (не остаться полностью пустой).
        let filled = cells
            .iter()
            .fold(0usize, |a, &c| a + usize::from(c != EMPTY));
        let frame = ((w * 2 + h * 2) as usize).saturating_sub(8); // грубая оценка рамки
        assert!(
            filled > frame,
            "SectorFiller ничего не залил: filled={filled}, frame≈{frame}"
        );
    }

    /// Детерминизм заливки: одинаковый seed → идентичный результат.
    #[test]
    fn fill_sectors_deterministic() {
        let chunks_w = 2u32;
        let chunks_h = 2u32;
        let w = chunks_w * CHUNK_SZ;
        let h = chunks_h * CHUNK_SZ;
        let total = (w * h) as usize;
        let cell_dur = [1.0f32; 256];

        let run = || {
            let mut cells = vec![EMPTY; total];
            for x in 0..w {
                for y in 0..h {
                    if x < 2 || y < 2 || x >= w - 2 || y >= h - 2 {
                        cells[cell_index_in_map(x, y, chunks_w, chunks_h)] = 117;
                    }
                }
            }
            let mut dur = vec![0u8; total * 4];
            fill_sectors(
                &mut cells, &mut dur, w, h, chunks_w, chunks_h, 777, &cell_dur,
            );
            cells
        };

        assert_eq!(run(), run());
    }
}
