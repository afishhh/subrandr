use std::collections::BTreeMap;

use rasterize::color::BGRA8;
use util::math::{Number, Signed, Vec2};

use crate::{layout::FixedL, text::OpenTypeTag};

pub trait ToPhysicalPixels {
    type Output;

    fn to_physical_pixels(self, dpi: u32) -> Self::Output;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Length(FixedL);

impl Length {
    pub const ONE: Self = Self(FixedL::ONE);
    pub const HALF: Length = Length::from_pixels(FixedL::HALF);
    pub const ZERO: Self = Self(FixedL::ZERO);

    pub const fn from_pixels(pixels: FixedL) -> Self {
        Self(pixels)
    }

    pub const fn from_points(pixels: FixedL) -> Self {
        // 96 / 72 = 4/3
        Self(FixedL::from_raw(pixels.into_raw() + pixels.into_raw() / 3))
    }
}

impl ToPhysicalPixels for Length {
    type Output = FixedL;

    fn to_physical_pixels(self, dpi: u32) -> Self::Output {
        self.0 * dpi as i32 / 72
    }
}

macro_rules! impl_length_op {
    ($trait: ident, $fun: ident, $trait_assign: ident, $fun_assign: ident, $op: tt, $op_assign: tt, $rhs_ty: ty, $($rhs_field: tt)*) => {
        impl std::ops::$trait<$rhs_ty> for Length {
            type Output = Self;

            #[track_caller]
            fn $fun(self, rhs: $rhs_ty) -> Self::Output {
                Self(self.0 $op rhs $($rhs_field)*)
            }
        }

        impl std::ops::$trait_assign<$rhs_ty> for Length {
            #[track_caller]
            fn $fun_assign(&mut self, rhs: $rhs_ty) {
                self.0 $op_assign rhs $($rhs_field)*;
            }
        }
    };
}

impl_length_op!(Add, add, AddAssign, add_assign, +, +=, Self, .0);
impl_length_op!(Sub, sub, SubAssign, sub_assign, -, -=, Self, .0);
impl_length_op!(Mul, mul, MulAssign, mul_assign, *, *=, Self, .0);
impl_length_op!(Div, div, DivAssign, div_assign, /, /=, Self, .0);
impl_length_op!(Mul, mul, MulAssign, mul_assign, *, *=, i32,);
impl_length_op!(Div, div, DivAssign, div_assign, /, /=, i32,);
impl_length_op!(Mul, mul, MulAssign, mul_assign, *, *=, f32,);
impl_length_op!(Div, div, DivAssign, div_assign, /, /=, f32,);

impl std::ops::Neg for Length {
    type Output = Self;

    #[track_caller]
    fn neg(self) -> Self::Output {
        Self(-self.0)
    }
}

impl Number for Length {
    const MIN: Self = Self(FixedL::MIN);
    const MAX: Self = Self(FixedL::MAX);
    const ONE: Self = Self::ONE;
    const ZERO: Self = Self::ZERO;
}

impl Signed for Length {}

impl ToPhysicalPixels for Vec2<Length> {
    type Output = Vec2<FixedL>;

    fn to_physical_pixels(self, dpi: u32) -> Self::Output {
        Vec2::new(
            self.x.to_physical_pixels(dpi),
            self.y.to_physical_pixels(dpi),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Alignment(pub HorizontalAlignment, pub VerticalAlignment);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalAlignment {
    Top,
    Center,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HorizontalAlignment {
    Left,
    Center,
    Right,
}

#[derive(Default, Debug, Clone, Copy)]
pub enum FontSlant {
    #[default]
    Regular,
    Italic,
}

#[derive(Debug, Clone, Copy)]
pub enum Ruby {
    None,
    Base,
    Over,
}

#[derive(Debug, Clone)]
pub struct TextShadow {
    pub offset: Vec2<Length>,
    pub blur_radius: Length,
    pub color: BGRA8,
}

#[derive(Debug, Clone, Copy)]
pub struct TextDecorations {
    pub underline: bool,
    pub underline_color: BGRA8,
    pub line_through: bool,
    pub line_through_color: BGRA8,
}

impl TextDecorations {
    pub const NONE: Self = Self {
        underline: false,
        underline_color: BGRA8::ZERO,
        line_through: false,
        line_through_color: BGRA8::ZERO,
    };
}

impl Default for TextDecorations {
    fn default() -> Self {
        Self::NONE
    }
}

#[derive(Debug, Clone)]
pub struct FontFeatureSettings(BTreeMap<OpenTypeTag, u32>);

impl FontFeatureSettings {
    pub const fn empty() -> Self {
        Self(BTreeMap::new())
    }

    pub fn set(&mut self, tag: OpenTypeTag, value: u32) {
        self.0.insert(tag, value);
    }

    pub fn iter(&self) -> impl Iterator<Item = (OpenTypeTag, u32)> + use<'_> {
        self.0.iter().map(|(&t, &v)| (t, v))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Debug, Clone, Copy)]
pub enum InlineSizing {
    Normal,
    Stretch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaselineSource {
    Last,
}

#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Ltr,
    Rtl,
}

#[derive(Debug, Clone, Copy)]
pub enum WhiteSpaceCollapse {
    Preserve,
}
