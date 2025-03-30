use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

use super::Vec2;

pub trait Number:
    Sized
    + Add<Self, Output = Self>
    + Sub<Self, Output = Self>
    + Div<Self, Output = Self>
    + Mul<Self, Output = Self>
    + AddAssign<Self>
    + SubAssign<Self>
    + DivAssign<Self>
    + MulAssign<Self>
    + Neg<Output = Self>
    + PartialOrd
    + Copy
{
    fn min(self, other: Self) -> Self {
        if self < other {
            self
        } else {
            other
        }
    }

    fn max(self, other: Self) -> Self {
        if self > other {
            self
        } else {
            other
        }
    }

    const MIN: Self;
    const MAX: Self;
    const ZERO: Self;
}

impl Number for f32 {
    const MIN: Self = f32::MIN;
    const MAX: Self = f32::MAX;
    const ZERO: Self = 0.0;
}

pub trait Sqrt: Number {
    fn sqrt(self) -> Self;
    fn fast_normalize(vector: Vec2<Self>) -> Vec2<Self> {
        vector / vector.length()
    }
}

impl Sqrt for f32 {
    fn sqrt(self) -> Self {
        self.sqrt()
    }

    fn fast_normalize(vector: Vec2<Self>) -> Vec2<Self> {
        super::fast_divide_by_sqrt(vector, vector.length_sq())
    }
}
