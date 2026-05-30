#[inline]
pub fn net_u8_clamped(v: i32, max: i32) -> u8 {
    u8::try_from(v.clamp(0, max)).unwrap_or(0)
}

#[inline]
pub fn net_u16_nonneg(v: i32) -> u16 {
    u16::try_from(v.max(0)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u8_clamped_passes_value_in_range() {
        assert_eq!(net_u8_clamped(5, 10), 5);
    }

    #[test]
    fn u8_clamped_floors_negative_to_zero() {
        assert_eq!(net_u8_clamped(-3, 10), 0);
    }

    #[test]
    fn u8_clamped_caps_at_max() {
        assert_eq!(net_u8_clamped(300, 10), 10);
    }

    #[test]
    fn u8_clamped_saturates_when_max_exceeds_u8() {
        // clamp пропускает 200, оно влезает в u8.
        assert_eq!(net_u8_clamped(200, 1000), 200);
    }

    #[test]
    fn u8_clamped_yields_zero_when_clamped_value_overflows_u8() {
        // clamp(0,1000)=300 → u8::try_from(300) падает → фолбэк 0.
        assert_eq!(net_u8_clamped(300, 1000), 0);
    }

    #[test]
    fn u8_clamped_handles_boundary_255() {
        assert_eq!(net_u8_clamped(255, 255), 255);
    }

    #[test]
    fn u16_nonneg_passes_positive() {
        assert_eq!(net_u16_nonneg(5), 5);
    }

    #[test]
    fn u16_nonneg_floors_negative_to_zero() {
        assert_eq!(net_u16_nonneg(-5), 0);
    }

    #[test]
    fn u16_nonneg_handles_boundary_65535() {
        assert_eq!(net_u16_nonneg(65_535), 65_535);
    }

    #[test]
    fn u16_nonneg_yields_zero_on_u16_overflow() {
        assert_eq!(net_u16_nonneg(70_000), 0);
    }
}
