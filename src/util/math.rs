use std::{
    fmt::Debug,
    iter::Sum,
    ops::{Add, AddAssign, Mul, Sub, SubAssign},
};

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
    x: f32,
    y: f32,
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

macro_rules! impl_binop {
    ($trait: ident, $trait_assign: ident, $fn: ident, $fn_assign: ident, $dst: ident, $operator: tt, $operator_assign: tt, $src: ident) => {
        impl $trait<$src> for $dst {
            type Output = Self;

            fn $fn(self, rhs: $src)-> Self::Output {
                Self {
                    x: self.x $operator rhs.x,
                    y: self.y $operator rhs.y,
                }
            }
        }

        impl $trait_assign<$src> for $dst {
            fn $fn_assign(&mut self, rhs: $src) {
                self.x $operator_assign rhs.x;
                self.y $operator_assign rhs.y;
            }
        }
    };
}

impl_binop!(
    Add, AddAssign, add, add_assign,
    Vec2, +, +=, Vec2
);

impl_binop!(
    Sub, SubAssign, sub, sub_assign,
    Vec2, -, -=, Vec2
);

impl_binop!(
    Add, AddAssign, add, add_assign,
    Point2, +, +=, Vec2
);

impl_binop!(
    Sub, SubAssign, sub, sub_assign,
    Point2, -, -=, Vec2
);

impl Sum for Vec2 {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.reduce(Self::add).unwrap_or(Vec2::default())
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

#[derive(Debug, Clone)]
pub struct BoundingBox {
    min: Point2,
    max: Point2,
}

impl BoundingBox {
    pub const fn new() -> Self {
        Self {
            min: Point2::new(f32::MAX, f32::MAX),
            max: Point2::new(f32::MIN, f32::MIN),
        }
    }

    pub fn add(&mut self, point: &Point2) {
        self.min.x = self.min.x.min(point.x);
        self.min.y = self.min.y.min(point.y);
        self.max.x = self.max.x.max(point.x);
        self.max.y = self.max.y.max(point.y);
    }

    pub fn is_empty(&self) -> bool {
        self.min.x == f32::MAX
            && self.min.y == f32::MAX
            && self.max.x == f32::MIN
            && self.max.y == f32::MIN
    }

    pub fn minmax(&self) -> Option<(Point2, Point2)> {
        if self.is_empty() {
            None
        } else {
            Some((self.min, self.max))
        }
    }
}

impl Default for BoundingBox {
    fn default() -> Self {
        Self::new()
    }
}
