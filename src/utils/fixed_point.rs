use derive_more::{Add, AddAssign, Mul, MulAssign, Sum};
use std::hash::Hash;

/// This will never change.
pub static FIXED_POINT_FACTOR: i64 = 1_i64 << 31;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Add, Mul, Sum, AddAssign, MulAssign)]
pub struct FixedPoint64(i64);

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
