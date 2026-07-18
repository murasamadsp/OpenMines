//! 1:1 порт 2D-пути библиотеки шума AccidentalNoise (TinkerWorX C#-порт
//! JTippetts ANL — он же NuGet `RcherNZ.AccidentalNoise`, что использует
//! legacy C#-сервер).
//!
//! Перенесён только 2D-путь — единственный, что вызывает генератор мира
//! (`fr.Get(x, y)`). 3D/4D/6D, `BasisType.White` (индекс 4 никогда не выпадает
//! в `rand.Next(0,4)`) опущены намеренно.
//!
//! Источник дословно: `crates/openmines-world/anl_reference/{Noise.cs,
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

pub mod basis;
pub mod fractal;
pub mod noise;
pub mod types;

pub use fractal::ImplicitFractal;
pub use types::{BasisType, FractalType, InterpolationType};

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
        assert_eq!(fr.exp_array(0), 1.0);
        assert_eq!(fr.correct(0)[0], 2.0);
        assert_eq!(fr.correct(0)[1], -1.0);
    }
}
