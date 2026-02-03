use std::fmt::Debug;

use log::{AsLogger, LogContext};
use util::math::{BoolExt, I26Dot6, Point2, Rect2, Vec2};

use crate::{
    style::{computed::ToPhysicalPixels, ComputedStyle},
    text::FontDb,
};

pub type FixedL = I26Dot6;
pub type Point2L = Point2<FixedL>;
pub type Vec2L = Vec2<FixedL>;
pub type Rect2L = Rect2<FixedL>;

#[derive(Debug, Clone, Copy)]
pub struct EdgeExtents {
    pub top: FixedL,
    pub bottom: FixedL,
    pub left: FixedL,
    pub right: FixedL,
}

impl EdgeExtents {
    const ZERO: Self = Self {
        top: FixedL::ZERO,
        bottom: FixedL::ZERO,
        left: FixedL::ZERO,
        right: FixedL::ZERO,
    };

    fn compute_fragmented(
        part: BoxFragmentationPart,
        top: impl FnOnce() -> FixedL,
        bottom: impl FnOnce() -> FixedL,
        left: impl FnOnce() -> FixedL,
        right: impl FnOnce() -> FixedL,
    ) -> Self {
        Self {
            top: part.is_top().then_or_zero(top),
            bottom: part.is_bottom().then_or_zero(bottom),
            left: part.is_leftmost().then_or_zero(left),
            right: part.is_rightmost().then_or_zero(right),
        }
    }
}

#[derive(Clone, Copy)]
struct BoxFragmentationPart(u8);

impl BoxFragmentationPart {
    const HORIZONTAL_FIRST: Self = Self(0b01);
    const HORIZONTAL_LAST: Self = Self(0b10);
    const HORIZONTAL_FULL: Self = Self(0b11);

    const VERTICAL_FIRST: Self = Self(0b01 << 2);
    const VERTICAL_LAST: Self = Self(0b10 << 2);
    const VERTICAL_FULL: Self = Self(0b11 << 2);

    const FULL: Self = Self(Self::HORIZONTAL_FULL.0 | Self::VERTICAL_FULL.0);

    fn is_top(self) -> bool {
        self.0 & Self::VERTICAL_FIRST.0 != 0
    }

    fn is_bottom(self) -> bool {
        self.0 & Self::VERTICAL_LAST.0 != 0
    }

    fn is_leftmost(self) -> bool {
        self.0 & Self::HORIZONTAL_FIRST.0 != 0
    }

    fn is_rightmost(self) -> bool {
        self.0 & Self::HORIZONTAL_LAST.0 != 0
    }
}

impl std::ops::BitOr for BoxFragmentationPart {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for BoxFragmentationPart {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl Debug for BoxFragmentationPart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BoxFragmentationPart(")?;
        let mut first = true;
        for (bit, name) in [
            (Self::VERTICAL_FIRST, "VERTICAL_FIRST"),
            (Self::VERTICAL_LAST, "VERTICAL_LAST"),
            (Self::HORIZONTAL_FIRST, "HORIZONTAL_FIRST"),
            (Self::HORIZONTAL_LAST, "HORIZONTAL_LAST"),
        ] {
            if self.0 & bit.0 != 0 {
                if !first {
                    write!(f, " | ")?;
                } else {
                    first = false;
                }
                write!(f, "{name}")?
            }
        }
        write!(f, ")")
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FragmentBox {
    pub content_size: Vec2L,
    pub padding: EdgeExtents,
}

impl FragmentBox {
    pub const ZERO: Self = Self {
        content_size: Vec2L::ZERO,
        padding: EdgeExtents::ZERO,
    };

    const fn new_content_only(content_size: Vec2L) -> Self {
        Self {
            content_size,
            padding: EdgeExtents::ZERO,
        }
    }

    // TODO: Make a newtype for dpi everywhere
    fn new_styled(content_size: Vec2L, dpi: u32, style: &ComputedStyle) -> Self {
        Self::new_styled_fragmented(content_size, dpi, style, BoxFragmentationPart::FULL)
    }

    fn new_styled_fragmented(
        content_size: Vec2L,
        dpi: u32,
        style: &ComputedStyle,
        part: BoxFragmentationPart,
    ) -> Self {
        Self {
            content_size,
            padding: EdgeExtents::compute_fragmented(
                part,
                || style.padding_top().to_physical_pixels(dpi),
                || style.padding_bottom().to_physical_pixels(dpi),
                || style.padding_left().to_physical_pixels(dpi),
                || style.padding_right().to_physical_pixels(dpi),
            ),
        }
    }

    pub fn content_offset(&self) -> Vec2L {
        Vec2L::new(self.padding.left, self.padding.top)
    }

    pub fn padding_box(&self) -> Rect2L {
        Rect2L::from_min_size(
            Point2L::ZERO,
            self.content_size
                + Vec2L::new(
                    self.padding.left + self.padding.right,
                    self.padding.top + self.padding.bottom,
                ),
        )
    }

    pub fn margin_box(&self) -> Rect2L {
        self.padding_box()
    }

    pub fn size_for_layout(&self) -> Vec2L {
        self.margin_box().max.to_vec()
    }
}

#[derive(Debug)]
pub struct LayoutContext<'l> {
    pub log: &'l LogContext<'l>,
    pub dpi: u32,
    pub fonts: &'l mut FontDb,
}

impl AsLogger for LayoutContext<'_> {
    fn as_logger(&self) -> &impl log::Logger {
        self.log.as_logger()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LayoutConstraints {
    pub size: Vec2L,
}

impl LayoutConstraints {
    pub const NONE: Self = Self {
        size: Vec2L::splat(FixedL::MAX),
    };
}

pub mod inline;
pub use inline::InlineLayoutError;
pub mod block;
