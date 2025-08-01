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
    const ONE: Self;
    const ZERO: Self;
}

pub trait Signed: Neg<Output = Self> {}

impl Number for f32 {
    const MIN: Self = f32::MIN;
    const MAX: Self = f32::MAX;
    const ONE: Self = 1.0;
    const ZERO: Self = 0.0;
}

impl Signed for f32 {}

impl Number for i32 {
    const MIN: Self = i32::MIN;
    const MAX: Self = i32::MAX;
    const ONE: Self = 1;
    const ZERO: Self = 0;
}

impl Signed for i32 {}

impl Number for u32 {
    const MIN: Self = u32::MIN;
    const MAX: Self = u32::MAX;
    const ONE: Self = 1;
    const ZERO: Self = 0;
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
