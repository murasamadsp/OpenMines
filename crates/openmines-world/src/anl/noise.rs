use super::types::InterpolationType;

#[inline]
pub(crate) fn fast_floor(t: f64) -> i32 {
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
pub(crate) fn lerp(s: f64, v1: f64, v2: f64) -> f64 {
    v1 + s * (v2 - v1)
}

#[inline]
pub(crate) fn interp(kind: InterpolationType, t: f64) -> f64 {
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
pub(crate) fn hash_coords(x: i32, y: i32, seed: i32) -> u32 {
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
pub(crate) fn gradient2d(index: u32) -> (f64, f64) {
    CARDINAL[(index % 4) as usize]
}

#[inline]
pub(crate) fn internal_value_noise(_x: f64, _y: f64, ix: i32, iy: i32, seed: i32) -> f64 {
    let noise = f64::from(hash_coords(ix, iy, seed)) / 255.0;
    noise * 2.0 - 1.0
}

#[inline]
pub(crate) fn internal_gradient_noise(x: f64, y: f64, ix: i32, iy: i32, seed: i32) -> f64 {
    let hash = hash_coords(ix, iy, seed);
    let dx = x - f64::from(ix);
    let dy = y - f64::from(iy);
    let g = gradient2d(hash);
    dx * g.0 + dy * g.1
}

pub(crate) type WorkerNoise2 = fn(f64, f64, i32, i32, i32) -> f64;

#[inline]
pub(crate) fn interpolate_x_2(
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
pub(crate) fn interpolate_xy_2(
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

pub(crate) fn value_noise(x: f64, y: f64, seed: i32, ip: InterpolationType) -> f64 {
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

pub(crate) fn gradient_noise(x: f64, y: f64, seed: i32, ip: InterpolationType) -> f64 {
    let x0 = fast_floor(x);
    let y0 = fast_floor(y);
    let x1 = x0.wrapping_add(1);
    let y1 = y0.wrapping_add(1);
    let xs = interp(ip, x - f64::from(x0));
    let ys = interp(ip, y - f64::from(y0));
    interpolate_xy_2(x, y, xs, ys, x0, x1, y0, y1, seed, internal_gradient_noise)
}

pub(crate) fn gradient_value_noise(x: f64, y: f64, seed: i32, ip: InterpolationType) -> f64 {
    value_noise(x, y, seed, ip) + gradient_noise(x, y, seed, ip)
}

/// `SimplexNoise(x, y, seed, _)` — дословно из `Noise.cs` (gradient table = 2D).
pub(crate) fn simplex_noise(x: f64, y: f64, seed: i32, _ip: InterpolationType) -> f64 {
    const F2: f64 = 0.366025403784438647; // 0.5*(sqrt(3)-1)
    const G2: f64 = 0.211324865405187118; // (3-sqrt(3))/6

    let s = (x + y) * F2;
    let i = fast_floor(x + s);
    let j = fast_floor(y + s);

    // C# `(i + j) * G2`: `i + j` — INT-сложение, которое ПЕРЕПОЛНЯЕТ i32 при
    // больших координатах (высокие octaves×freq×lac → inner-coord ~1.5e9, сумма
    // >i32::MAX → two's-complement wrap), и лишь потом `* G2` (double). Складываем
    // в i32 через `wrapping_add` ДО конверсии в f64, иначе расход с C# на глубоких
    // октавах базиса Simplex.
    let t = f64::from(i.wrapping_add(j)) * G2;
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
pub(crate) fn clamp(value: f64, low: f64, high: f64) -> f64 {
    if value < low {
        low
    } else if value > high {
        high
    } else {
        value
    }
}
