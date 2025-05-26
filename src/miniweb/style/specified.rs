use util::math::Fixed;

use super::{computed, restyle::StylingContext, ComputedStyle};
use crate::miniweb::layout::FixedL;

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum LengthUnit {
    // Absolute units
    Centimeter,
    Millimeter,
    QuarterMillimeter,
    Inches,
    Picas,
    Points,
    Pixels,

    // (supported) Viewport relative units
    ViewportWidth,
    ViewportHeight,
    ViewportMin,
}

/// A number associated with a [`LengthUnit`].
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct Length {
    value: FixedL,
    unit: LengthUnit,
}

impl Length {
    fn to_pixels(&self, ctx: &StylingContext) -> FixedL {
        match self.unit {
            LengthUnit::Centimeter => self.value * FixedL::from_f32(2.54) / 96,
            LengthUnit::Millimeter => self.value * FixedL::from_f32(0.254) / 96,
            LengthUnit::QuarterMillimeter => self.value * FixedL::from_f32(0.254) / (4 * 96),
            LengthUnit::Inches => self.value * 96,
            LengthUnit::Picas => self.value * 96 / 6,
            LengthUnit::Points => self.value * 96 / 72,
            LengthUnit::Pixels => self.value,

            LengthUnit::ViewportWidth => self.value * ctx.viewport_size.x / 100,
            LengthUnit::ViewportHeight => self.value * ctx.viewport_size.y / 100,
            LengthUnit::ViewportMin => {
                self.value * ctx.viewport_size.x.min(ctx.viewport_size.y) / 100
            }
        }
    }

    pub(super) fn compute(
        ctx: &StylingContext,
        _parent: &ComputedStyle,
        value: &Self,
    ) -> computed::Pixels {
        computed::Pixels(value.to_pixels(ctx))
    }
}

/// A percentage value in the range `[-1, 1]`.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct Percentage(pub Fixed<24, i32>);

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum LengthOrPercentage {
    Length(Length),
    Percentage(Percentage),
}

impl LengthOrPercentage {
    pub(super) fn compute(
        ctx: &StylingContext,
        parent: &ComputedStyle,
        value: &Self,
    ) -> computed::PixelsOrPercentage {
        match value {
            LengthOrPercentage::Length(length) => {
                computed::PixelsOrPercentage::Pixels(Length::compute(ctx, parent, length))
            }
            &LengthOrPercentage::Percentage(percentage) => {
                computed::PixelsOrPercentage::Percentage(percentage)
            }
        }
    }
}
