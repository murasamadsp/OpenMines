//! 1:1 порт .NET Framework `System.Random` (subtractive Knuth PRNG).
//!
//! ANL `ImplicitBasisFunction` сидит угол поворота через `new Random(seed)`,
//! а `Sectors`/`SectorFiller`/`Sector` гоняют `Random.Next`/`NextDouble`.
//! Для верного 1:1 поведения шума нужен именно этот алгоритм, а не Java-LCG.
//!
//! Источник: реализация `System.Random` из .NET Reference Source
//! (`Random.cs`, алгоритм Кнута из Numerical Recipes). Используется legacy
//! C#-сервером через `RcherNZ.AccidentalNoise` (netstandard2.0 → .NET Framework
//! Random semantics).

#![allow(
    // Усечения/потери точности — дословные `(int)`/`(double)` касты .NET Random.
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    // `% 2 == 0` — дословная проба знака в GetSampleForLargeRange.
    clippy::manual_is_multiple_of,
    clippy::missing_const_for_fn
)]

const MBIG: i32 = i32::MAX; // 2_147_483_647
const MSEED: i32 = 161_803_398;

/// 1:1 с `System.Random`.
pub struct DotNetRandom {
    seed_array: [i32; 56],
    inext: usize,
    inextp: usize,
}

impl DotNetRandom {
    /// `new Random(Int32 seed)`.
    // `mj = seed_array[ii]` на последней итерации не читается, но это дословная
    // строка .NET Reference Source (в итерациях 1..54 значение используется).
    #[allow(unused_assignments)]
    pub fn new(seed: i32) -> Self {
        let mut seed_array = [0i32; 56];

        // subtraction = (seed == Int32.MinValue) ? Int32.MaxValue : Abs(seed)
        let subtraction = if seed == i32::MIN {
            i32::MAX
        } else {
            seed.abs()
        };

        let mut mj = MSEED.wrapping_sub(subtraction);
        seed_array[55] = mj;
        let mut mk = 1i32;

        // for (int i = 1; i < 55; i++) — заполняет в порядке (21*i)%55
        let mut ii: usize = 0;
        for _i in 1..55 {
            ii += 21;
            if ii >= 55 {
                ii -= 55;
            }
            seed_array[ii] = mk;
            mk = mj.wrapping_sub(mk);
            if mk < 0 {
                mk = mk.wrapping_add(MBIG);
            }
            mj = seed_array[ii];
        }

        for _ in 1..5 {
            for i in 1..56 {
                let j = if i + 30 >= 55 { i + 30 - 55 } else { i + 30 };
                seed_array[i] = seed_array[i].wrapping_sub(seed_array[1 + j]);
                if seed_array[i] < 0 {
                    seed_array[i] = seed_array[i].wrapping_add(MBIG);
                }
            }
        }

        Self {
            seed_array,
            inext: 0,
            inextp: 21,
        }
    }

    /// `protected virtual Double Sample()`.
    fn sample(&mut self) -> f64 {
        f64::from(self.internal_sample()) * (1.0 / f64::from(MBIG))
    }

    /// `private Int32 InternalSample()`.
    fn internal_sample(&mut self) -> i32 {
        let mut loc_inext = self.inext;
        let mut loc_inextp = self.inextp;

        loc_inext += 1;
        if loc_inext >= 56 {
            loc_inext = 1;
        }
        loc_inextp += 1;
        if loc_inextp >= 56 {
            loc_inextp = 1;
        }

        let mut ret_val = self.seed_array[loc_inext].wrapping_sub(self.seed_array[loc_inextp]);

        if ret_val == MBIG {
            ret_val -= 1;
        }
        if ret_val < 0 {
            ret_val = ret_val.wrapping_add(MBIG);
        }

        self.seed_array[loc_inext] = ret_val;
        self.inext = loc_inext;
        self.inextp = loc_inextp;

        ret_val
    }

    /// `public virtual Double NextDouble()`.
    pub fn next_double(&mut self) -> f64 {
        self.sample()
    }

    /// `private Double GetSampleForLargeRange()`.
    fn sample_for_large_range(&mut self) -> f64 {
        let mut result = i64::from(self.internal_sample());
        // вероятность отрицательного с шансом 50% (negative half of distribution)
        let neg = self.internal_sample() % 2 == 0;
        if neg {
            result = -result;
        }
        let mut d = result as f64;
        d += f64::from(i32::MAX - 1); // (Int32.MaxValue - 1)
        d /= f64::from(2u32 * (i32::MAX as u32) - 1); // 2 * (uint)Int32.MaxValue - 1
        d
    }

    /// `public virtual Int32 Next(Int32 minValue, Int32 maxValue)` — [min, max).
    pub fn next_range(&mut self, min_value: i32, max_value: i32) -> i32 {
        assert!(min_value <= max_value, "minValue must be <= maxValue");

        let range = i64::from(max_value) - i64::from(min_value);
        if range <= i64::from(i32::MAX) {
            (self.sample() * range as f64) as i32 + min_value
        } else {
            (self.sample_for_large_range() * range as f64) as i64 as i32 + min_value
        }
    }

    /// `public virtual Int32 Next()` — [0, Int32.MaxValue).
    /// Часть полного `System.Random` API; в проде не вызывается (только тесты).
    #[allow(dead_code)]
    pub fn next(&mut self) -> i32 {
        self.internal_sample()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Golden-последовательности алгоритма .NET `System.Random`. Сверены с
    // независимой эталонной реализацией того же алгоритма (.NET Reference
    // Source). ВНИМАНИЕ: это самосогласованность с алгоритмом, не прогон
    // настоящего C# — при доступном `dotnet` стоит подтвердить ещё раз.
    #[test]
    fn next_matches_reference_seed42() {
        let mut r = DotNetRandom::new(42);
        assert_eq!(r.next(), 1_434_747_710);
        assert_eq!(r.next(), 302_596_119);
        assert_eq!(r.next(), 269_548_474);
    }

    #[test]
    fn next_range_reference_seed0() {
        let mut r = DotNetRandom::new(0);
        assert_eq!(r.next_range(1, 101), 73);
        assert_eq!(r.next_range(1, 101), 82);
    }

    #[test]
    fn deterministic_same_seed() {
        let mut a = DotNetRandom::new(4242);
        let mut b = DotNetRandom::new(4242);
        for _ in 0..1000 {
            assert_eq!(a.next(), b.next());
        }
    }

    #[test]
    fn next_double_in_unit_range() {
        let mut r = DotNetRandom::new(123);
        for _ in 0..1000 {
            let d = r.next_double();
            assert!((0.0..1.0).contains(&d), "next_double out of [0,1): {d}");
        }
    }
}
