use std::{collections::BTreeMap, ops::Add};

use rasterize::color::BGRA8;
use util::math::{I26Dot6, Vec2f};

use crate::{layout::FixedL, text::OpenTypeTag};

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

    #[cfg_attr(not(all(test, feature = "_layout_tests")), expect(dead_code))]
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

impl Add for Length {
    type Output = Length;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
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
