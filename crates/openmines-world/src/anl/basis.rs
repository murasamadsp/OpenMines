use super::noise::{gradient_noise, gradient_value_noise, simplex_noise, value_noise};
use super::types::{BasisType, InterpolationType};
use dotnet_rng::DotnetRng;
use std::f64::consts::PI;

pub(crate) type WorkerNoise2Basis = fn(f64, f64, i32, InterpolationType) -> f64;

pub(crate) struct ImplicitBasisFunction {
    noise2d: WorkerNoise2Basis,
    interp: InterpolationType,
    seed: i32,
    cos2d: f64,
    sin2d: f64,
}

impl ImplicitBasisFunction {
    pub(crate) fn new(basis: BasisType, interp: InterpolationType) -> Self {
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

    pub(crate) fn set_seed(&mut self, value: i32) {
        self.seed = value;
        let mut random = DotnetRng::new(value);

        let ax = random.next_f64();
        let ay = random.next_f64();
        let az = random.next_f64();
        let _len = (ax * ax + ay * ay + az * az).sqrt();
        // SetRotationAngle использует 4-й NextDouble (3D-матрица) — в 2D не нужен,
        // но обязан продвинуть состояние RNG, чтобы 5-й вызов совпал с C#.
        let _angle3d = random.next_f64() * PI * 2.0;
        // 5-й NextDouble — угол поворота координат для 2D.
        let angle = random.next_f64() * PI * 2.0;
        self.cos2d = angle.cos();
        self.sin2d = angle.sin();
    }

    #[inline]
    pub(crate) fn get(&self, x: f64, y: f64) -> f64 {
        let nx = x * self.cos2d - y * self.sin2d;
        let ny = y * self.cos2d + x * self.sin2d;
        (self.noise2d)(nx, ny, self.seed, self.interp)
    }
}
