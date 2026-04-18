#[inline]
pub fn net_u8_clamped(v: i32, max: i32) -> u8 {
    u8::try_from(v.clamp(0, max)).unwrap_or(0)
}

#[inline]
pub fn net_u16_nonneg(v: i32) -> u16 {
    u16::try_from(v.max(0)).unwrap_or(0)
}
