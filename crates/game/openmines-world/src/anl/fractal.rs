use super::basis::ImplicitBasisFunction;
use super::noise::clamp;
use super::types::{BasisType, FractalType, InterpolationType};

const MAX_SOURCES: usize = 20;

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

    /// Тестовый аксессор внутреннего exp-массива весов (octave `i`).
    #[cfg(test)]
    pub(crate) fn exp_array(&self, i: usize) -> f64 {
        self.exp_array[i]
    }

    /// Тестовый аксессор внутренней матрицы correct-весов (octave `i`, [scale,bias]).
    #[cfg(test)]
    pub(crate) fn correct(&self, i: usize) -> [f64; 2] {
        self.correct[i]
    }
}
