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
