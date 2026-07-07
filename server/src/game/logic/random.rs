use rand::Rng as _;

#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn probabilistic_i64_with_roll(value: f32, roll: f32) -> i64 {
    assert!(value.is_finite(), "probabilistic value must be finite");
    if value <= 0.0 {
        return 0;
    }
    let whole = value.trunc() as i64;
    let fraction = value.fract();
    whole + i64::from(fraction > 0.0 && roll < fraction)
}

#[must_use]
pub fn probabilistic_i64(value: f32) -> i64 {
    probabilistic_i64_with_roll(value, rand::rng().random_range(0.0..1.0))
}

#[must_use]
pub fn probabilistic_i32(value: f32) -> i32 {
    i32::try_from(probabilistic_i64(value)).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use super::probabilistic_i64_with_roll;

    #[test]
    fn fractional_part_is_probability_for_next_integer() {
        assert_eq!(probabilistic_i64_with_roll(2.25, 0.24), 3);
        assert_eq!(probabilistic_i64_with_roll(2.25, 0.25), 2);
        assert_eq!(probabilistic_i64_with_roll(2.25, 0.99), 2);
        assert_eq!(probabilistic_i64_with_roll(2.0, 0.0), 2);
        assert_eq!(probabilistic_i64_with_roll(0.5, 0.49), 1);
        assert_eq!(probabilistic_i64_with_roll(0.5, 0.50), 0);
    }

    #[test]
    fn roll_happens_after_formula_parts_are_summed() {
        let general_mining = 5.04_f32;
        let red_mining = 2.30_f32;
        let total = general_mining + red_mining;

        assert_eq!(probabilistic_i64_with_roll(total, 0.33), 8);
        assert_eq!(probabilistic_i64_with_roll(total, 0.35), 7);
    }
}
