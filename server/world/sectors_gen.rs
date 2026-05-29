//! 1:1 порт C# `Sectors.cs` + `Gen.cs` (этап «скелета» мира): RidgedMulti-шум
//! через настоящий AccidentalNoise (`super::anl`), затем `AddW` × 3, `CleanCs`
//! × 6, чёрная скала, `Clean`. Случайность — порт .NET `System.Random`
//! (`super::dotnet_random`), сид берётся из конфига (детерминированно).
//!
//! Источник дословно: `server_reference/GameShit/Generator/{Sectors.cs,Gen.cs}`.

#![allow(
    clippy::many_single_char_names,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::float_cmp,
    clippy::too_many_lines,
    clippy::module_name_repetitions,
    clippy::doc_markdown,
    clippy::needless_range_loop,
    clippy::similar_names,
    clippy::redundant_guards,
    // `mul_add`/FMA меняет округление — `chs` обязан считать как C# `30 - y*k`.
    clippy::suboptimal_flops,
    clippy::missing_const_for_fn,
    // `(x as f32 / w as f32) as f64` — дословный f32-расчёт C# до double Get.
    clippy::cast_lossless,
    // `j % 2 == 0` — дословное условие направления прохода CleanCs.
    clippy::manual_is_multiple_of,
    // Ветки Clean идентичны телом, но различны условием/розыгрышем rng.
    clippy::if_same_then_else
)]

use super::anl::{BasisType, FractalType, ImplicitFractal, InterpolationType};
use super::dotnet_random::DotNetRandom;

const BLACK_ROCK: u8 = 114;
const RED_ROCK: u8 = 117;
const EMPTY: u8 = 0;

/// Результат генерации скелета. `values[x * height + y]`: 0=пусто,
/// RED_ROCK=красная скала, BLACK_ROCK=чёрная скала.
pub struct SectorMap {
    pub values: Vec<u8>,
    /// Габариты возвращаются для самодокументации; `generator` читает только
    /// `values`, поэтому поля помечены `dead_code`.
    #[allow(dead_code)]
    pub width: u32,
    #[allow(dead_code)]
    pub height: u32,
}

/// Порт C# `Sectors`. `value`: f32 на клетку (как `SectorCell.value`),
/// индексация `x * height + y`. Значения скелета 0/1/2.
struct Sectors {
    width: usize,
    height: usize,
    seed: i32,
    value: Vec<f32>,
    mid: f32,
    /// Общий RNG (как `Sectors.r`) — используется только пост-обработкой.
    r: DotNetRandom,
}

#[inline]
fn chs(y: usize) -> f32 {
    30.0 - (y as f32) * 0.0028
}

impl Sectors {
    fn new(seed: i32, width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            seed,
            value: vec![0.0; width * height],
            mid: 0.0,
            r: DotNetRandom::new(seed),
        }
    }

    /// `GenerateENoise(freq, lac, interp, res=.45f)`.
    fn generate_enoise(&mut self, freq: f64, lac: f64, interp: InterpolationType, res: f32) {
        let w = self.width;
        let h = self.height;

        let mut fr =
            ImplicitFractal::new(FractalType::RidgedMulti, BasisType::GradientValue, interp);
        fr.set_octaves(1);
        fr.set_frequency(freq);
        fr.set_lacunarity(lac);
        fr.set_seed(self.seed);

        let mut max = fr.get(0.0, 0.0) as f32;
        let mut min = fr.get(0.0, 0.0) as f32;

        for x in 0..w {
            for y in 0..h {
                let nx = (x as f32 / w as f32) as f64;
                let ny = (y as f32 / h as f32) as f64;
                let v = fr.get(nx, ny) as f32;
                max = if max < v { v } else { max };
                min = if min < v { min } else { v };
                self.value[x * h + y] = v;
            }
        }

        let mut mid = 0f32;
        for x in 0..w {
            for y in 0..h {
                let idx = x * h + y;
                self.value[idx] = (self.value[idx] - min) / (max - min);
                mid += self.value[idx];
            }
        }
        mid /= (w * h) as f32;
        self.mid = mid;

        self.resample(res);
    }

    /// `resample(res=.45f)`: `value < mid+res → 0`, иначе `→ 1`.
    fn resample(&mut self, res: f32) {
        let threshold = self.mid + res;
        for v in &mut self.value {
            *v = if *v < threshold { 0.0 } else { 1.0 };
        }
    }

    /// `AddW(freq, lac, interp, res=.45f)`: наложить новый слой на пустые клетки.
    fn add_w(&mut self, freq: f64, lac: f64, interp: InterpolationType, res: f32) {
        let temp = self.value.clone();
        self.generate_enoise(freq, lac, interp, res);
        for i in 0..self.value.len() {
            // temp == 0 ? map(new) : temp
            self.value[i] = if temp[i] == 0.0 {
                self.value[i]
            } else {
                temp[i]
            };
        }
    }

    /// `CleanCs(j, b)`: проход по строкам (чётные снизу-вверх, нечётные сверху-вниз);
    /// клетки-скалы (1) с достаточным числом чёрных/пустых соседей → чёрная (2).
    fn clean_cs(&mut self, j: usize, b: bool) {
        let w = self.width as i32;
        let h = self.height as i32;
        let hu = self.height;

        let valid = |x: i32, y: i32| x < w && x >= 0 && y < h && y >= 0;

        // for (y = j%2==0 ? 0 : h-1; cond; y±±)
        let even = j % 2 == 0;
        let mut y: i32 = if even { 0 } else { h - 1 };
        loop {
            if even {
                if y >= h {
                    break;
                }
            } else if y < 0 {
                break;
            }

            for x in 0..w {
                if self.value[(x as usize) * hu + y as usize] != 1.0 {
                    continue;
                }
                let mut c = 0i32;
                let mut ch = 0i32;
                let mut e = 0i32;
                for xx in -2..=2i32 {
                    for yy in -2..=2i32 {
                        let nx = x + xx;
                        let ny = y + yy;
                        if !valid(nx, ny) {
                            continue;
                        }
                        match self.value[(nx as usize) * hu + ny as usize] {
                            v if v == 1.0 => c += 1,
                            v if v == 2.0 => ch += 1,
                            v if v == 0.0 => e += 1,
                            _ => {}
                        }
                    }
                }
                let _ = c; // считается в референсе, но не используется в условии
                if (3 < ch && self.r.next_range(1, 101) > 60) || e > 1 {
                    self.value[(x as usize) * hu + y as usize] = 2.0;
                    if self.r.next_range(1, 101) > 95 && b {
                        self.boom(x, y);
                    }
                }
            }

            if even {
                y += 1;
            } else {
                y -= 1;
            }
        }
    }

    /// `Boom(x, y)`: радиус 3..6, зашумлённое расширение чёрной скалы.
    fn boom(&mut self, x: i32, y: i32) {
        let w = self.width as i32;
        let h = self.height as i32;
        let hu = self.height;
        let valid = |x: i32, y: i32| x < w && x >= 0 && y < h && y >= 0;

        let b = self.r.next_range(3, 7);
        for xx in -b..=b {
            for yy in -b..=b {
                let nx = x + xx;
                let ny = y + yy;
                if !valid(nx, ny) {
                    continue;
                }
                let idx = (nx as usize) * hu + ny as usize;
                let cell = self.value[idx];
                let hit = (cell == 0.0 && self.r.next_range(1, 101) > 60)
                    || (cell == 1.0 && (self.r.next_range(1, 101) as f32) < chs(y as usize));
                if hit {
                    self.value[idx] = 2.0;
                }
            }
        }
    }

    /// `Clean()`: 10% чёрных и 5% красных скал → пусто.
    fn clean(&mut self) {
        let w = self.width;
        let h = self.height;
        for y in 0..h {
            for x in 0..w {
                let idx = x * h + y;
                if self.value[idx] == 2.0 && self.r.next_range(1, 101) > 90 {
                    self.value[idx] = 0.0;
                } else if self.value[idx] == 1.0 && self.r.next_range(1, 101) > 95 {
                    self.value[idx] = 0.0;
                }
            }
        }
    }

    /// `Add()`: CleanCs(0,true) + CleanCs(1..6) + слой чёрной скалы по глубине.
    fn add(&mut self) {
        self.clean_cs(0, true);
        for i in 1..6 {
            self.clean_cs(i, false);
        }
        // чёрная скала по chs(y)
        let w = self.width;
        let h = self.height;
        for x in 0..w {
            for y in 0..h {
                let idx = x * h + y;
                if self.value[idx] == 1.0 && (self.r.next_range(1, 101) as f32) < chs(y) {
                    self.value[idx] = 2.0;
                }
            }
        }
    }

    /// `End()`: Add + Clean (тип-конверсия читает `.value`).
    fn end(&mut self) {
        self.add();
        self.clean();
    }
}

/// Полная генерация скелета мира, 1:1 с C# `Gen.StartGeneration()`:
/// GenerateENoise(15,1,Cubic) + AddW(15,1,Linear) + AddW(25,5,Linear)
/// + AddW(35,20,Quintic) + End. Значения 0/1/2 → EMPTY/RED_ROCK/BLACK_ROCK.
pub fn generate_skeleton(w: u32, h: u32, seed: u32) -> SectorMap {
    let mut s = Sectors::new(seed as i32, w as usize, h as usize);

    s.generate_enoise(15.0, 1.0, InterpolationType::Cubic, 0.45);
    s.add_w(15.0, 1.0, InterpolationType::Linear, 0.45);
    s.add_w(25.0, 5.0, InterpolationType::Linear, 0.45);
    s.add_w(35.0, 20.0, InterpolationType::Quintic, 0.45);
    s.end();

    let values: Vec<u8> = s
        .value
        .iter()
        .map(|&v| {
            if v == 2.0 {
                BLACK_ROCK
            } else if v == 1.0 {
                RED_ROCK
            } else {
                EMPTY
            }
        })
        .collect();

    SectorMap {
        values,
        width: w,
        height: h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chs_matches_reference() {
        assert!((chs(0) - 30.0).abs() < 1e-6);
        assert!((chs(10000) - (30.0 - 10000.0 * 0.0028)).abs() < 1e-3);
    }

    #[test]
    fn skeleton_runs_and_sizes() {
        let map = generate_skeleton(64, 128, 4242);
        assert_eq!(map.values.len(), 64 * 128);
        assert_eq!(map.width, 64);
        assert_eq!(map.height, 128);
    }

    #[test]
    fn skeleton_deterministic() {
        let a = generate_skeleton(48, 96, 4242);
        let b = generate_skeleton(48, 96, 4242);
        assert_eq!(a.values, b.values);
    }

    #[test]
    fn skeleton_has_rock_and_empty() {
        // Скелет ridged должен дать смесь пустоты и скалы, не быть однородным.
        let map = generate_skeleton(96, 192, 4242);
        let has_empty = map.values.contains(&EMPTY);
        let has_rock = map.values.iter().any(|&v| v != EMPTY);
        assert!(has_empty, "нет пустых клеток");
        assert!(has_rock, "нет скалы");
    }
}
