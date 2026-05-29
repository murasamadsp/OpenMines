//! 1:1 порт 2D-пути библиотеки шума AccidentalNoise (TinkerWorX C#-порт
//! JTippetts ANL — он же NuGet `RcherNZ.AccidentalNoise`, что использует
//! legacy C#-сервер).
//!
//! Перенесён только 2D-путь — единственный, что вызывает генератор мира
//! (`fr.Get(x, y)`). 3D/4D/6D, `BasisType.White` (индекс 4 никогда не выпадает
//! в `rand.Next(0,4)`) опущены намеренно.
//!
//! Источник дословно: `server/world/anl_reference/{Noise.cs,
//! ImplicitBasisFunction.cs, ImplicitFractal.cs, NoiseLookupTable.cs}`.

#![allow(
    clippy::many_single_char_names,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::float_cmp,
    clippy::similar_names,
    clippy::module_name_repetitions,
    clippy::doc_markdown,
    clippy::unreadable_literal,
    clippy::must_use_candidate,
    clippy::missing_panics_doc,
    clippy::too_many_arguments,
    // `mul_add`/FMA даёт ИНОЕ округление — для 1:1 float-семантики ANL запрещён.
    clippy::suboptimal_flops,
    // `-1.0 * x`, `2.0 * abs - 1.0` — дословные выражения ANL CalculateWeights.
    clippy::neg_multiply,
    clippy::excessive_precision,
    clippy::missing_const_for_fn,
    clippy::if_same_then_else,
    // Полный 1:1 порт ANL-API: не-RidgedMulti фрактал-типы, не-GradientValue
    // базисы и `from_i32` задействует `SectorFiller` (следующий этап порта).
    dead_code
)]

use super::dotnet_random::DotNetRandom;
use std::f64::consts::PI;

const MAX_SOURCES: usize = 20;

// --- Enums (порядок значений = индексы для (Type)rand.Next(...)) ---

/// `FractalType` — порядок объявления критичен: `(FractalType)rand.Next(0,5)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FractalType {
    FractionalBrownianMotion = 0,
    RidgedMulti = 1,
    Billow = 2,
    Multi = 3,
    HybridMulti = 4,
}

/// `BasisType` — `(BasisType)rand.Next(0,4)` даёт 0..3 (White=4 не выпадает).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BasisType {
    Value = 0,
    Gradient = 1,
    GradientValue = 2,
    Simplex = 3,
}

/// `InterpolationType` — `(InterpolationType)rand.Next(0,4)` даёт 0..3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterpolationType {
    None = 0,
    Linear = 1,
    Cubic = 2,
    Quintic = 3,
}

impl FractalType {
    pub fn from_i32(v: i32) -> Self {
        match v {
            0 => Self::FractionalBrownianMotion,
            1 => Self::RidgedMulti,
            2 => Self::Billow,
            3 => Self::Multi,
            _ => Self::HybridMulti,
        }
    }
}

impl BasisType {
    pub fn from_i32(v: i32) -> Self {
        match v {
            0 => Self::Value,
            1 => Self::Gradient,
            3 => Self::Simplex,
            _ => Self::GradientValue,
        }
    }
}

impl InterpolationType {
    pub fn from_i32(v: i32) -> Self {
        match v {
            0 => Self::None,
            1 => Self::Linear,
            3 => Self::Quintic,
            _ => Self::Cubic,
        }
    }
}

// --- Noise статика (Noise.cs) ---

#[inline]
fn fast_floor(t: f64) -> i32 {
    // C#: t > 0 ? (Int32)t : (Int32)t - 1  (приведение к int усекает к нулю)
    if t > 0.0 {
        t as i32
    } else {
        // `as i32` насыщает гигантские |t| к границам i32 (в C# overflow
        // unchecked); `wrapping_sub` вместо паники при экстремальных координатах.
        (t as i32).wrapping_sub(1)
    }
}

#[inline]
fn lerp(s: f64, v1: f64, v2: f64) -> f64 {
    v1 + s * (v2 - v1)
}

#[inline]
fn interp(kind: InterpolationType, t: f64) -> f64 {
    match kind {
        InterpolationType::None => 0.0,
        InterpolationType::Linear => t,
        InterpolationType::Cubic => t * t * (3.0 - 2.0 * t), // HermiteInterpolation
        InterpolationType::Quintic => t * t * t * (t * (t * 6.0 - 15.0) + 10.0),
    }
}

const FNV_32_PRIME: u32 = 0x01000193;
const FNV_32_INIT: u32 = 2166136261;

/// `HashCoordinates(Int32 x, Int32 y, Int32 seed)` — FNV-1a по 12 LE-байтам
/// `Int32[]{x,y,seed}` + XOR-fold до байта.
#[inline]
fn hash_coords(x: i32, y: i32, seed: i32) -> u32 {
    let mut buf = [0u8; 12];
    buf[0..4].copy_from_slice(&x.to_le_bytes());
    buf[4..8].copy_from_slice(&y.to_le_bytes());
    buf[8..12].copy_from_slice(&seed.to_le_bytes());

    let mut hval = FNV_32_INIT;
    for b in buf {
        hval ^= u32::from(b);
        hval = hval.wrapping_mul(FNV_32_PRIME);
    }
    // XORFoldHash → (byte)((hash>>8) ^ (hash & 0xFF))
    u32::from((((hval >> 8) ^ (hval & 0xFF)) & 0xFF) as u8)
}

/// `Gradient2D[index]` — таблица из 256 записей = 64 повтора
/// `{0,1},{0,-1},{1,0},{-1,0}`, т.е. `CARDINAL[index % 4]`.
const CARDINAL: [(f64, f64); 4] = [(0.0, 1.0), (0.0, -1.0), (1.0, 0.0), (-1.0, 0.0)];

#[inline]
fn gradient2d(index: u32) -> (f64, f64) {
    CARDINAL[(index % 4) as usize]
}

#[inline]
fn internal_value_noise(_x: f64, _y: f64, ix: i32, iy: i32, seed: i32) -> f64 {
    let noise = f64::from(hash_coords(ix, iy, seed)) / 255.0;
    noise * 2.0 - 1.0
}

#[inline]
fn internal_gradient_noise(x: f64, y: f64, ix: i32, iy: i32, seed: i32) -> f64 {
    let hash = hash_coords(ix, iy, seed);
    let dx = x - f64::from(ix);
    let dy = y - f64::from(iy);
    let g = gradient2d(hash);
    dx * g.0 + dy * g.1
}

type WorkerNoise2 = fn(f64, f64, i32, i32, i32) -> f64;

#[inline]
fn interpolate_x_2(
    x: f64,
    y: f64,
    xs: f64,
    x0: i32,
    x1: i32,
    iy: i32,
    seed: i32,
    f: WorkerNoise2,
) -> f64 {
    let v1 = f(x, y, x0, iy, seed);
    let v2 = f(x, y, x1, iy, seed);
    lerp(xs, v1, v2)
}

#[inline]
fn interpolate_xy_2(
    x: f64,
    y: f64,
    xs: f64,
    ys: f64,
    x0: i32,
    x1: i32,
    y0: i32,
    y1: i32,
    seed: i32,
    f: WorkerNoise2,
) -> f64 {
    let v1 = interpolate_x_2(x, y, xs, x0, x1, y0, seed, f);
    let v2 = interpolate_x_2(x, y, xs, x0, x1, y1, seed, f);
    lerp(ys, v1, v2)
}

fn value_noise(x: f64, y: f64, seed: i32, ip: InterpolationType) -> f64 {
    let x0 = fast_floor(x);
    let y0 = fast_floor(y);
    // wrapping: при огромных координатах (Lacunarity^octaves) C# считает в
    // unchecked-int; в Rust debug `+1` иначе паникует. На нормальных входах
    // wrap не наступает, результат идентичен.
    let x1 = x0.wrapping_add(1);
    let y1 = y0.wrapping_add(1);
    let xs = interp(ip, x - f64::from(x0));
    let ys = interp(ip, y - f64::from(y0));
    interpolate_xy_2(x, y, xs, ys, x0, x1, y0, y1, seed, internal_value_noise)
}

fn gradient_noise(x: f64, y: f64, seed: i32, ip: InterpolationType) -> f64 {
    let x0 = fast_floor(x);
    let y0 = fast_floor(y);
    let x1 = x0.wrapping_add(1);
    let y1 = y0.wrapping_add(1);
    let xs = interp(ip, x - f64::from(x0));
    let ys = interp(ip, y - f64::from(y0));
    interpolate_xy_2(x, y, xs, ys, x0, x1, y0, y1, seed, internal_gradient_noise)
}

fn gradient_value_noise(x: f64, y: f64, seed: i32, ip: InterpolationType) -> f64 {
    value_noise(x, y, seed, ip) + gradient_noise(x, y, seed, ip)
}

/// `SimplexNoise(x, y, seed, _)` — дословно из `Noise.cs` (gradient table = 2D).
fn simplex_noise(x: f64, y: f64, seed: i32, _ip: InterpolationType) -> f64 {
    const F2: f64 = 0.366025403784438647; // 0.5*(sqrt(3)-1)
    const G2: f64 = 0.211324865405187118; // (3-sqrt(3))/6

    let s = (x + y) * F2;
    let i = fast_floor(x + s);
    let j = fast_floor(y + s);

    // Целочисленные операции — `wrapping`, как unchecked-int в C# (при
    // вырожденных координатах сектора i/j могут улетать к границам i32).
    let t = (f64::from(i) + f64::from(j)) * G2;
    let xx0 = f64::from(i) - t;
    let yy0 = f64::from(j) - t;
    let x0 = x - xx0;
    let y0 = y - yy0;

    let (i1, j1) = if x0 > y0 { (1, 0) } else { (0, 1) };

    let x1 = x0 - f64::from(i1) + G2;
    let y1 = y0 - f64::from(j1) + G2;
    let x2 = x0 - 1.0 + 2.0 * G2;
    let y2 = y0 - 1.0 + 2.0 * G2;

    let h0 = hash_coords(i, j, seed);
    let h1 = hash_coords(i.wrapping_add(i1), j.wrapping_add(j1), seed);
    let h2 = hash_coords(i.wrapping_add(1), j.wrapping_add(1), seed);

    let g0 = gradient2d(h0);
    let g1 = gradient2d(h1);
    let g2 = gradient2d(h2);

    let mut n0 = 0.0;
    let mut t0 = 0.5 - x0 * x0 - y0 * y0;
    if t0 >= 0.0 {
        t0 *= t0;
        n0 = t0 * t0 * (g0.0 * x0 + g0.1 * y0);
    }

    let mut n1 = 0.0;
    let mut t1 = 0.5 - x1 * x1 - y1 * y1;
    if t1 >= 0.0 {
        t1 *= t1;
        n1 = t1 * t1 * (g1.0 * x1 + g1.1 * y1);
    }

    let mut n2 = 0.0;
    let mut t2 = 0.5 - x2 * x2 - y2 * y2;
    if t2 >= 0.0 {
        t2 *= t2;
        n2 = t2 * t2 * (g2.0 * x2 + g2.1 * y2);
    }

    (70.0 * (n0 + n1 + n2)) * 1.42188695 + 0.001054489
}

#[inline]
fn clamp(value: f64, low: f64, high: f64) -> f64 {
    if value < low {
        low
    } else if value > high {
        high
    } else {
        value
    }
}

// --- ImplicitBasisFunction (ImplicitBasisFunction.cs) ---

type WorkerNoise2Basis = fn(f64, f64, i32, InterpolationType) -> f64;

struct ImplicitBasisFunction {
    noise2d: WorkerNoise2Basis,
    interp: InterpolationType,
    seed: i32,
    cos2d: f64,
    sin2d: f64,
}

impl ImplicitBasisFunction {
    fn new(basis: BasisType, interp: InterpolationType) -> Self {
        let noise2d: WorkerNoise2Basis = match basis {
            BasisType::Value => value_noise,
            BasisType::Gradient => gradient_noise,
            BasisType::GradientValue => gradient_value_noise,
            BasisType::Simplex => simplex_noise,
        };
        // В C# Seed изначально = DateTime.Now.Ticks, но ImplicitFractal всегда
        // перезаписывает его через set_seed; стартуем с 0.
        let mut f = Self {
            noise2d,
            interp,
            seed: 0,
            cos2d: 1.0,
            sin2d: 0.0,
        };
        f.set_seed(0);
        f
    }

    fn set_seed(&mut self, value: i32) {
        self.seed = value;
        let mut random = DotNetRandom::new(value);

        let ax = random.next_double();
        let ay = random.next_double();
        let az = random.next_double();
        let _len = (ax * ax + ay * ay + az * az).sqrt();
        // SetRotationAngle использует 4-й NextDouble (3D-матрица) — в 2D не нужен,
        // но обязан продвинуть состояние RNG, чтобы 5-й вызов совпал с C#.
        let _angle3d = random.next_double() * PI * 2.0;
        // 5-й NextDouble — угол поворота координат для 2D.
        let angle = random.next_double() * PI * 2.0;
        self.cos2d = angle.cos();
        self.sin2d = angle.sin();
    }

    #[inline]
    fn get(&self, x: f64, y: f64) -> f64 {
        let nx = x * self.cos2d - y * self.sin2d;
        let ny = y * self.cos2d + x * self.sin2d;
        (self.noise2d)(nx, ny, self.seed, self.interp)
    }
}

// --- ImplicitFractal (ImplicitFractal.cs) ---

pub struct ImplicitFractal {
    sources: Vec<ImplicitBasisFunction>,
    exp_array: [f64; MAX_SOURCES],
    correct: [[f64; 2]; MAX_SOURCES],
    seed: i32,
    fractal_type: FractalType,
    octaves: i32,
    frequency: f64,
    lacunarity: f64,
    gain: f64,
    offset: f64,
    h: f64,
}

impl ImplicitFractal {
    pub fn new(
        fractal_type: FractalType,
        basis_type: BasisType,
        interp: InterpolationType,
    ) -> Self {
        let mut sources = Vec::with_capacity(MAX_SOURCES);
        for _ in 0..MAX_SOURCES {
            sources.push(ImplicitBasisFunction::new(basis_type, interp));
        }
        let mut f = Self {
            sources,
            exp_array: [0.0; MAX_SOURCES],
            correct: [[0.0; 2]; MAX_SOURCES],
            seed: 0,
            fractal_type,
            octaves: 8,
            frequency: 1.0,
            lacunarity: 2.0,
            gain: 0.0,
            offset: 0.0,
            h: 1.0,
        };
        f.set_type(fractal_type);
        f
    }

    fn set_type(&mut self, t: FractalType) {
        self.fractal_type = t;
        match t {
            FractalType::FractionalBrownianMotion => {
                self.h = 1.0;
                self.gain = 0.0;
                self.offset = 0.0;
                self.fbm_calculate_weights();
            }
            FractalType::RidgedMulti => {
                self.h = 0.90;
                self.gain = 2.0;
                self.offset = 1.0;
                self.ridged_multi_calculate_weights();
            }
            FractalType::Billow => {
                self.h = 1.0;
                self.gain = 0.0;
                self.offset = 0.0;
                self.billow_calculate_weights();
            }
            FractalType::Multi => {
                self.h = 1.0;
                self.gain = 0.0;
                self.offset = 0.0;
                self.multi_calculate_weights();
            }
            FractalType::HybridMulti => {
                self.h = 0.25;
                self.gain = 1.0;
                self.offset = 0.70;
                self.hybrid_multi_calculate_weights();
            }
        }
    }

    /// `Octaves` setter (clamp `>= MAX_SOURCES` → `MAX_SOURCES - 1`).
    pub fn set_octaves(&mut self, value: i32) {
        self.octaves = if value >= MAX_SOURCES as i32 {
            MAX_SOURCES as i32 - 1
        } else {
            value
        };
    }

    pub fn set_frequency(&mut self, v: f64) {
        self.frequency = v;
    }

    pub fn set_lacunarity(&mut self, v: f64) {
        self.lacunarity = v;
    }

    /// `Seed` setter — каждый источник получает `seed + s*300`.
    pub fn set_seed(&mut self, seed: i32) {
        self.seed = seed;
        for (s, src) in self.sources.iter_mut().enumerate() {
            src.set_seed(seed.wrapping_add((s as i32).wrapping_mul(300)));
        }
    }

    fn fbm_calculate_weights(&mut self) {
        for i in 0..MAX_SOURCES {
            self.exp_array[i] = self.lacunarity.powf(-(i as f64) * self.h);
        }
        let mut minvalue = 0.0;
        let mut maxvalue = 0.0;
        for i in 0..MAX_SOURCES {
            minvalue += -1.0 * self.exp_array[i];
            maxvalue += 1.0 * self.exp_array[i];
            let scale = 2.0 / (maxvalue - minvalue);
            let bias = -1.0 - minvalue * scale;
            self.correct[i][0] = scale;
            self.correct[i][1] = bias;
        }
    }

    fn ridged_multi_calculate_weights(&mut self) {
        for i in 0..MAX_SOURCES {
            self.exp_array[i] = self.lacunarity.powf(-(i as f64) * self.h);
        }
        let mut minvalue = 0.0;
        let mut maxvalue = 0.0;
        for i in 0..MAX_SOURCES {
            minvalue += (self.offset - 1.0) * (self.offset - 1.0) * self.exp_array[i];
            maxvalue += self.offset * self.offset * self.exp_array[i];
            let scale = 2.0 / (maxvalue - minvalue);
            let bias = -1.0 - minvalue * scale;
            self.correct[i][0] = scale;
            self.correct[i][1] = bias;
        }
    }

    fn billow_calculate_weights(&mut self) {
        for i in 0..MAX_SOURCES {
            self.exp_array[i] = self.lacunarity.powf(-(i as f64) * self.h);
        }
        let mut minvalue = 0.0;
        let mut maxvalue = 0.0;
        for i in 0..MAX_SOURCES {
            minvalue += -1.0 * self.exp_array[i];
            maxvalue += 1.0 * self.exp_array[i];
            let scale = 2.0 / (maxvalue - minvalue);
            let bias = -1.0 - minvalue * scale;
            self.correct[i][0] = scale;
            self.correct[i][1] = bias;
        }
    }

    fn multi_calculate_weights(&mut self) {
        for i in 0..MAX_SOURCES {
            self.exp_array[i] = self.lacunarity.powf(-(i as f64) * self.h);
        }
        let mut minvalue = 1.0;
        let mut maxvalue = 1.0;
        for i in 0..MAX_SOURCES {
            minvalue *= -1.0 * self.exp_array[i] + 1.0;
            maxvalue *= 1.0 * self.exp_array[i] + 1.0;
            let scale = 2.0 / (maxvalue - minvalue);
            let bias = -1.0 - minvalue * scale;
            self.correct[i][0] = scale;
            self.correct[i][1] = bias;
        }
    }

    fn hybrid_multi_calculate_weights(&mut self) {
        for i in 0..MAX_SOURCES {
            self.exp_array[i] = self.lacunarity.powf(-(i as f64) * self.h);
        }
        let mut minvalue = self.offset - 1.0;
        let mut maxvalue = self.offset + 1.0;
        let mut weightmin = self.gain * minvalue;
        let mut weightmax = self.gain * maxvalue;

        let mut scale = 2.0 / (maxvalue - minvalue);
        let mut bias = -1.0 - minvalue * scale;
        self.correct[0][0] = scale;
        self.correct[0][1] = bias;

        for i in 1..MAX_SOURCES {
            if weightmin > 1.0 {
                weightmin = 1.0;
            }
            if weightmax > 1.0 {
                weightmax = 1.0;
            }
            let mut signal = (self.offset - 1.0) * self.exp_array[i];
            minvalue += signal * weightmin;
            weightmin *= self.gain * signal;

            signal = (self.offset + 1.0) * self.exp_array[i];
            maxvalue += signal * weightmax;
            weightmax *= self.gain * signal;

            scale = 2.0 / (maxvalue - minvalue);
            bias = -1.0 - minvalue * scale;
            self.correct[i][0] = scale;
            self.correct[i][1] = bias;
        }
    }

    pub fn get(&self, x: f64, y: f64) -> f64 {
        let v = match self.fractal_type {
            FractalType::FractionalBrownianMotion => self.fbm_get(x, y),
            FractalType::RidgedMulti => self.ridged_multi_get(x, y),
            FractalType::Billow => self.billow_get(x, y),
            FractalType::Multi => self.multi_get(x, y),
            FractalType::HybridMulti => self.hybrid_multi_get(x, y),
        };
        clamp(v, -1.0, 1.0)
    }

    fn fbm_get(&self, mut x: f64, mut y: f64) -> f64 {
        let mut value = 0.0;
        x *= self.frequency;
        y *= self.frequency;
        for i in 0..self.octaves as usize {
            let signal = self.sources[i].get(x, y) * self.exp_array[i];
            value += signal;
            x *= self.lacunarity;
            y *= self.lacunarity;
        }
        // NB: 2D-перегрузка Fbm в C# не применяет correct[] (см. исходник).
        value
    }

    fn ridged_multi_get(&self, mut x: f64, mut y: f64) -> f64 {
        let mut result = 0.0;
        x *= self.frequency;
        y *= self.frequency;
        for i in 0..self.octaves as usize {
            let mut signal = self.sources[i].get(x, y);
            signal = self.offset - signal.abs();
            signal *= signal;
            result += signal * self.exp_array[i];
            x *= self.lacunarity;
            y *= self.lacunarity;
        }
        let oc = self.octaves as usize - 1;
        result * self.correct[oc][0] + self.correct[oc][1]
    }

    fn billow_get(&self, mut x: f64, mut y: f64) -> f64 {
        let mut value = 0.0;
        x *= self.frequency;
        y *= self.frequency;
        for i in 0..self.octaves as usize {
            let mut signal = self.sources[i].get(x, y);
            signal = 2.0 * signal.abs() - 1.0;
            value += signal * self.exp_array[i];
            x *= self.lacunarity;
            y *= self.lacunarity;
        }
        value += 0.5;
        let oc = self.octaves as usize - 1;
        value * self.correct[oc][0] + self.correct[oc][1]
    }

    fn multi_get(&self, mut x: f64, mut y: f64) -> f64 {
        let mut value = 1.0;
        x *= self.frequency;
        y *= self.frequency;
        for i in 0..self.octaves as usize {
            value *= self.sources[i].get(x, y) * self.exp_array[i] + 1.0;
            x *= self.lacunarity;
            y *= self.lacunarity;
        }
        let oc = self.octaves as usize - 1;
        value * self.correct[oc][0] + self.correct[oc][1]
    }

    fn hybrid_multi_get(&self, mut x: f64, mut y: f64) -> f64 {
        x *= self.frequency;
        y *= self.frequency;

        let mut value = self.sources[0].get(x, y) + self.offset;
        let mut weight = self.gain * value;
        x *= self.lacunarity;
        y *= self.lacunarity;

        for i in 1..self.octaves as usize {
            if weight > 1.0 {
                weight = 1.0;
            }
            let signal = (self.sources[i].get(x, y) + self.offset) * self.exp_array[i];
            value += weight * signal;
            weight *= self.gain * signal;
            x *= self.lacunarity;
            y *= self.lacunarity;
        }
        let oc = self.octaves as usize - 1;
        value * self.correct[oc][0] + self.correct[oc][1]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ridged_multi_get_in_clamp_range() {
        let mut fr = ImplicitFractal::new(
            FractalType::RidgedMulti,
            BasisType::GradientValue,
            InterpolationType::Cubic,
        );
        fr.set_octaves(1);
        fr.set_frequency(15.0);
        fr.set_lacunarity(1.0);
        fr.set_seed(4242);
        for x in 0..50 {
            for y in 0..50 {
                let v = fr.get(f64::from(x) / 50.0, f64::from(y) / 50.0);
                assert!((-1.0..=1.0).contains(&v), "out of clamp: {v}");
            }
        }
    }

    #[test]
    fn deterministic_same_seed() {
        let make = || {
            let mut fr = ImplicitFractal::new(
                FractalType::RidgedMulti,
                BasisType::GradientValue,
                InterpolationType::Cubic,
            );
            fr.set_octaves(1);
            fr.set_frequency(15.0);
            fr.set_lacunarity(1.0);
            fr.set_seed(4242);
            fr
        };
        let a = make();
        let b = make();
        for x in 0..20 {
            for y in 0..20 {
                let xx = f64::from(x) / 20.0;
                let yy = f64::from(y) / 20.0;
                assert_eq!(a.get(xx, yy), b.get(xx, yy));
            }
        }
    }

    #[test]
    fn ridged_octave1_weights() {
        // octaves=1, lac=2, h=0.9, offset=1 (значения с момента конструктора):
        // correct[0] = (2, -1), выход = clamp(2*(1-|s|)^2 - 1).
        let fr = ImplicitFractal::new(
            FractalType::RidgedMulti,
            BasisType::GradientValue,
            InterpolationType::Cubic,
        );
        assert_eq!(fr.exp_array[0], 1.0);
        assert_eq!(fr.correct[0][0], 2.0);
        assert_eq!(fr.correct[0][1], -1.0);
    }
}
