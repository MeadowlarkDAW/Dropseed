#[inline]
pub fn pcm_u8_to_f32(s: u8) -> f32 {
    ((f32::from(s)) * (2.0 / std::u8::MAX as f32)) - 1.0
}

#[inline]
pub fn pcm_u16_to_f32(s: u16) -> f32 {
    ((f32::from(s)) * (2.0 / std::u16::MAX as f32)) - 1.0
}

#[inline]
pub fn pcm_u24_to_f32_ne(s: [u8; 3]) -> f32 {
    #[cfg(target_endian = "little")]
    return pcm_u24_to_f32_le(s);

    #[cfg(target_endian = "big")]
    return pcm_u24_to_f32_be(s);
}

#[inline]
pub fn pcm_u24_to_f32_le(s: [u8; 3]) -> f32 {
    // In little-endian the MSB is the last byte.
    let bytes = [s[0], s[1], s[2], 0];

    let val = u32::from_le_bytes(bytes);

    ((f64::from(val) * (2.0 / 16_777_215.0)) - 1.0) as f32
}

#[inline]
pub fn pcm_u24_to_f32_be(s: [u8; 3]) -> f32 {
    // In big-endian the MSB is the first byte.
    let bytes = [0, s[0], s[1], s[2]];

    let val = u32::from_be_bytes(bytes);

    ((f64::from(val) * (2.0 / 16_777_215.0)) - 1.0) as f32
}

#[inline]
pub fn pcm_u32_to_f32(s: u32) -> f32 {
    ((f64::from(s) * (2.0 / std::u32::MAX as f64)) - 1.0) as f32
}

#[inline]
pub fn pcm_i8_to_f32(s: i8) -> f32 {
    f32::from(s) / std::i8::MAX as f32
}

#[inline]
pub fn pcm_s8_to_f32(s: i8) -> f32 {
    f32::from(s) / std::i8::MAX as f32
}

#[inline]
pub fn pcm_s16_to_f32(s: i16) -> f32 {
    f32::from(s) / std::i16::MAX as f32
}

#[inline]
pub fn pcm_s24_to_f32_ne(s: [u8; 3]) -> f32 {
    #[cfg(target_endian = "little")]
    return pcm_s24_to_f32_le(s);

    #[cfg(target_endian = "big")]
    return pcm_s24_to_f32_be(s);
}

#[inline]
pub fn pcm_s24_to_f32_le(s: [u8; 3]) -> f32 {
    // In little-endian the MSB is the last byte.
    let bytes = [s[0], s[1], s[2], 0];

    let val = i32::from_le_bytes(bytes);

    (f64::from(val) / 8_388_607.0) as f32
}

#[inline]
pub fn pcm_s24_to_f32_be(s: [u8; 3]) -> f32 {
    // In big-endian the MSB is the first byte.
    let bytes = [0, s[0], s[1], s[2]];

    let val = i32::from_be_bytes(bytes);

    (f64::from(val) / 8_388_607.0) as f32
}

#[inline]
pub fn pcm_s32_to_f32(s: i32) -> f32 {
    (f64::from(s) / std::i32::MAX as f64) as f32
}
