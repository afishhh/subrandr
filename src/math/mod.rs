use std::{
    arch::asm,
    fmt::Debug,
    iter::Sum,
    ops::{Add, AddAssign, Div, Mul, MulAssign, Sub, SubAssign},
};

use num::complex::Complex64;

mod curve;
pub use curve::*;
mod fixed;
pub use fixed::*;

#[derive(Clone, Copy, Default, PartialEq)]
#[repr(C)]
pub struct Point2 {
    pub x: f32,
    pub y: f32,
}

impl Point2 {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub const fn from_array(xy: [f32; 2]) -> Self {
        Self { x: xy[0], y: xy[1] }
    }

    pub const fn to_vec(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }

    pub fn distance(self, other: Self) -> f32 {
        (self - other).length()
    }

    pub const ZERO: Self = Self::new(0., 0.);
}

impl Debug for Point2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({:.1}, {:.1})", self.x, self.y)
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
#[repr(C)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub const fn from_array(xy: [f32; 2]) -> Self {
        Self { x: xy[0], y: xy[1] }
    }

    pub const fn to_point(self) -> Point2 {
        Point2::new(self.x, self.y)
    }

    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    pub fn length_sq(self) -> f32 {
        self.x * self.x + self.y * self.y
    }

    pub fn normal(self) -> Self {
        Vec2::new(self.y, -self.x)
    }

    /// Calculates the dot product of two vectors.
    ///
    /// The dot product of two (2d) vectors is defined for vector u and v as:
    /// u⋅v = u.x * v.x + u.y * v.y
    ///
    /// However there is also a useful geometric definition:
    /// u⋅v = ||u|| * ||v|| * cos(θ)
    /// where θ is the anglge between u and v.
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y
    }

    /// Calculates the cross product of two vectors.
    ///
    /// # Note
    ///
    /// The cross product of two (2d) vectors is defined for vector u and v as:
    /// u⨯v = u.x * v.y - u.y * v.x
    ///
    /// However there is also a useful geometric definition:
    /// u⨯v = ||u|| * ||v|| * sin(θ)
    ///
    /// If this value is negative that means that the second vector is
    /// in the "clockwise direction" while if it positive then
    /// it is in the "counter-clockwise direction".
    ///
    /// another NOTE: This terminology is made up and probably not very formal.
    pub fn cross(self, other: Self) -> f32 {
        self.x * other.y - self.y * other.x
    }

    pub fn normalize(self) -> Self {
        #[cfg(target_feature = "sse")]
        unsafe {
            let length_sq = self.length_sq();
            let mut invlength: f32;
            asm!("rsqrtss {}, {}", out(xmm_reg) invlength, in(xmm_reg) length_sq);
            // rsqrtss + one newton-raphson step = 22-bits of accuracy
            // still faster than sqrt
            invlength *= 1.5 - (length_sq * 0.5 * invlength * invlength);
            self * invlength
        }
        #[cfg(not(target_feature = "sse"))]
        {
            self / self.length()
        }
    }

    pub const ZERO: Self = Self::new(0., 0.);
}

impl Debug for Vec2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{:.1}, {:.1}]", self.x, self.y)
    }
}

impl Mul<f32> for Vec2 {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Self {
            x: self.x * rhs,
            y: self.y * rhs,
        }
    }
}

impl Div<f32> for Vec2 {
    type Output = Self;

    fn div(self, rhs: f32) -> Self::Output {
        Self {
            x: self.x / rhs,
            y: self.y / rhs,
        }
    }
}

macro_rules! impl_binop {
    (@arg_or_self $arg: ident) => { $arg };
    (@arg_or_self) => { Self };
    ($trait: ident, $fn: ident$(, $trait_assign: ident, $fn_assign: ident)?; $dst: ident, $operator: tt, $operator_assign: tt, $src: ident$(, $output: ident)?) => {
        impl $trait<$src> for $dst {
            type Output = impl_binop!(@arg_or_self $($output)?);

            fn $fn(self, rhs: $src)-> Self::Output {
                <impl_binop!(@arg_or_self $($output)?)>::new(
                    self.x $operator rhs.x,
                    self.y $operator rhs.y,
                )
            }
        }

        $(
            impl $trait_assign<$src> for $dst {
                fn $fn_assign(&mut self, rhs: $src) {
                    self.x $operator_assign rhs.x;
                    self.y $operator_assign rhs.y;
                }
            }
        )?
    };
}

impl_binop!(
    Add, add, AddAssign, add_assign;
    Vec2, +, +=, Vec2
);

impl_binop!(
    Sub, sub, SubAssign, sub_assign;
    Vec2, -, -=, Vec2
);

impl_binop!(
    Add, add, AddAssign, add_assign;
    Point2, +, +=, Vec2
);

impl_binop!(
    Sub, sub, SubAssign, sub_assign;
    Point2, -, -=, Vec2
);

impl_binop!(
    Sub, sub;
    Point2, -, _, Point2, Vec2
);

impl Sum for Vec2 {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.reduce(Self::add).unwrap_or_default()
    }
}

impl Sum<Vec2> for Point2 {
    fn sum<I: Iterator<Item = Vec2>>(iter: I) -> Self {
        let mut result = Self::ZERO;
        for value in iter {
            result += value;
        }
        result
    }
}

#[derive(Debug, Clone, Default)]
pub struct Rect2 {
    pub min: Point2,
    pub max: Point2,
}

impl Rect2 {
    pub const NOTHING: Self = Self {
        min: Point2::new(f32::MAX, f32::MAX),
        max: Point2::new(f32::MIN, f32::MIN),
    };

    pub const ZERO: Self = Self {
        min: Point2::ZERO,
        max: Point2::ZERO,
    };

    pub fn is_negative(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y
    }

    pub fn clamp_to_positive(&self) -> Self {
        if self.is_negative() {
            Self::ZERO
        } else {
            self.clone()
        }
    }

    pub fn intersects(&self, other: &Self) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
    }

    pub fn size(&self) -> Vec2 {
        self.max - self.min
    }

    pub fn area(&self) -> f32 {
        let size = self.size();
        // Rect2::NOTHING
        if self.min.x > self.max.x {
            0.0
        } else {
            size.x * size.y
        }
    }

    pub fn expand_to_point(&mut self, point: &Point2) {
        self.min.x = self.min.x.min(point.x);
        self.min.y = self.min.y.min(point.y);
        self.max.x = self.max.x.max(point.x);
        self.max.y = self.max.y.max(point.y);
    }

    pub fn bounding_from_points(points: &[Point2]) -> Self {
        let mut bb = Self::NOTHING;
        for point in points {
            bb.expand_to_point(point);
        }
        bb
    }
}

pub fn solve_cubic(a: f64, b: f64, c: f64, d: f64) -> impl Iterator<Item = f64> {
    let det0 = b * b - 3.0 * a * c;
    let det1 = 2.0 * b * b * b - 9.0 * a * b * c + 27.0 * a * a * d;
    let c_sqrt_sq = det1 * det1 - 4.0 * det0.powi(3);
    let c_sqrt = Complex64::new(c_sqrt_sq, 0.0).sqrt();
    let c_cubed_1 = (det1 + c_sqrt) / 2.0;
    let c_cubed = if c_cubed_1 == Complex64::ZERO {
        (det1 - c_sqrt) / 2.0
    } else {
        c_cubed_1
    };
    let mut c = c_cubed.cbrt();

    let a3_neg_recip = (-3.0 * a).recip();

    let cube_root_of_unity = -0.5 + Complex64::new(-3.0, 0.0).sqrt() / 2.0;
    (0..3).filter_map(move |_| {
        let root = if c.re == 0.0 {
            Complex64::new(a3_neg_recip * b, 0.0)
        } else {
            a3_neg_recip * (b + c + det0 / c)
        };

        c *= cube_root_of_unity;
        if root.im > 10.0 * -f64::EPSILON && root.im < 10.0 * f64::EPSILON {
            Some(root.re)
        } else {
            None
        }
    })
}

#[derive(Debug, Clone, Copy)]
pub struct Line {
    pub a: f32,
    pub b: f32,
    pub c: f32,
}

impl Line {
    pub const fn new(a: f32, b: f32, c: f32) -> Self {
        Self { a, b, c }
    }

    pub const fn from_points(start: Point2, end: Point2) -> Self {
        let a = end.y - start.y;
        let b = start.x - end.x;
        let c = -(start.y * b + start.x * a);
        Self::new(a, b, c)
    }

    pub fn signed_distance_to_point(self, Point2 { x, y }: Point2) -> f32 {
        let Self { a, b, c } = self;
        (a * x + b * y + c) / (a * a + b * b).sqrt()
    }

    pub fn distance_to_point(self, p: Point2) -> f32 {
        self.signed_distance_to_point(p).abs()
    }

    pub fn sample_y(&self, x: f32) -> f32 {
        (-x * self.a - self.c) / self.b
    }
}

impl Mul<f32> for Line {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Self::new(self.a * rhs, self.b * rhs, self.c * rhs)
    }
}

impl MulAssign<f32> for Line {
    fn mul_assign(&mut self, rhs: f32) {
        self.a *= rhs;
        self.b *= rhs;
        self.c *= rhs;
    }
}

fn lerp<S: Mul<f32, Output = S>, T: Clone + Add<S, Output = T> + Sub<T, Output = S>>(
    a: T,
    b: T,
    t: f32,
) -> T {
    a.clone() + (b - a) * t
}
