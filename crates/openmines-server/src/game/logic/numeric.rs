use num_traits::ToPrimitive as _;

/// Converts a continuous formula result into discrete points.
/// Truncates toward zero, saturates out-of-range values, and maps `NaN` to zero.
#[must_use]
pub fn saturating_trunc_f32_to_i32(value: f32) -> i32 {
    value.to_i32().unwrap_or_else(|| {
        if value.is_nan() {
            0
        } else if value.is_sign_negative() {
            i32::MIN
        } else {
            i32::MAX
        }
    })
}

#[cfg(test)]
mod tests {
    use super::saturating_trunc_f32_to_i32;

    #[test]
    fn float_to_int_truncates_and_saturates() {
        assert_eq!(saturating_trunc_f32_to_i32(1.9), 1);
        assert_eq!(saturating_trunc_f32_to_i32(-1.9), -1);
        assert_eq!(saturating_trunc_f32_to_i32(f32::MAX), i32::MAX);
        assert_eq!(saturating_trunc_f32_to_i32(f32::MIN), i32::MIN);
        assert_eq!(saturating_trunc_f32_to_i32(f32::INFINITY), i32::MAX);
        assert_eq!(saturating_trunc_f32_to_i32(f32::NEG_INFINITY), i32::MIN);
        assert_eq!(saturating_trunc_f32_to_i32(f32::NAN), 0);
    }
}
