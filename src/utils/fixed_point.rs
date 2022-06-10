use std::hash::Hash;

pub static FIXED_POINT_FACTOR: i64 = 1_i64 << 31;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FixedPoint64(pub(crate) i64);

impl FixedPoint64 {
    #[inline]
    pub fn from_f64(val: f64) -> Self {
        Self((FIXED_POINT_FACTOR as f64 * val).round() as i64)
    }

    #[inline]
    pub fn as_f64(&self) -> f64 {
        self.0 as f64 / FIXED_POINT_FACTOR as f64
    }
}

impl From<f64> for FixedPoint64 {
    fn from(v: f64) -> Self {
        Self::from_f64(v)
    }
}

impl From<FixedPoint64> for f64 {
    fn from(v: FixedPoint64) -> Self {
        v.as_f64()
    }
}
