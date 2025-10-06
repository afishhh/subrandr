use rasterize::color::BGRA8;
use util::math::{I26Dot6, Vec2f};

use crate::layout::FixedL;

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
    pub offset: Vec2f,
    pub blur_radius: I26Dot6,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Length(FixedL);

impl Length {
    pub const ZERO: Self = Self(FixedL::ZERO);

    #[expect(dead_code)]
    pub const fn from_pixels(pixels: FixedL) -> Self {
        Self(pixels)
    }

    pub const fn from_points(pixels: FixedL) -> Self {
        // 96 / 72 = 4/3
        Self(FixedL::from_raw(pixels.into_raw() + pixels.into_raw() / 3))
    }

    pub fn to_physical_pixels(self, dpi: u32) -> FixedL {
        self.0 * dpi as i32 / 72
    }
}
