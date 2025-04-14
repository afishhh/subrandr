//! Length and length-related values from the [css-values](https://drafts.csswg.org/css-values-3) spec.
use super::*;
use crate::{csssyn::token::*, layout::FixedL, style::computed::Length};

pub(super) struct LengthRange {
    min: Length,
    max: Length,
}

impl LengthRange {
    pub(super) const fn new(min: Length, max: Length) -> Self {
        Self { min, max }
    }

    pub(super) const MAX: Self = Self::new(Length::MIN, Length::MAX);
    pub(super) const NONNEGATIVE: Self = Self::new(Length::ZERO, Length::MAX);
}

// https://drafts.csswg.org/css-values-3/#lengths
// TODO: Consider implementing font-relative lengths
pub(super) fn try_take_length_in_range(
    stream: &mut ParseStream,
    range: LengthRange,
) -> Result<Option<Length>, ParseError> {
    Ok(if stream.peek_skip(Token![0]) {
        Some(Length::ZERO)
    } else if stream.peek(Dimension) {
        let dim = stream.parse::<Dimension>()?;
        let value = try_compute_absolute_length(dim)?
            .ok_or_else(|| ParseError::new(dim, "unknown length unit"))?;

        if value < range.min || value > range.max {
            return Err(ParseError::new(dim, "value outside of allowed range"));
        }

        Some(value)
    } else {
        None
    })
}

pub(super) fn try_take_length(stream: &mut ParseStream) -> Result<Option<Length>, ParseError> {
    try_take_length_in_range(stream, LengthRange::MAX)
}

pub(super) fn take_length_in_range(
    stream: &mut ParseStream,
    range: LengthRange,
) -> Result<Length, ParseError> {
    try_take_length_in_range(stream, range)?.ok_or_else(|| stream.lookahead_error())
}

pub(super) fn take_length(stream: &mut ParseStream) -> Result<Length, ParseError> {
    take_length_in_range(stream, LengthRange::MAX)
}

// https://drafts.csswg.org/css-values-3/#absolute-lengths
fn try_compute_absolute_length(dim: Dimension<'_>) -> Result<Option<Length>, ParseError> {
    for &(unit, factor) in ABSOLUTE_UNITS {
        if dim.unit().eq_ignore_ascii_case(unit) {
            let pixels = dim.value().to_finite_f64(dim)? * factor;

            return Ok(Some(Length::from_pixels(FixedL::from_f64(pixels))));
        }
    }

    Ok(None)
}

static ABSOLUTE_UNITS: &[(&str, f64)] = &[
    ("px", 1.0),           // pixels
    ("pt", 96.0 / 72.0),   // points
    ("in", 96.0),          // inches
    ("mm", 480.0 / 127.0), // millimeters
    ("cm", 96.0 / 2.54),   // centimeters
    ("Q", 120.0 / 127.0),  // quarter millimeters
    ("pc", 96.0 / 6.0),    // picas
];

#[cfg(test)]
mod test {
    use super::*;

    fn compute_as_padding_top(source: &str) -> Result<Length, ParseError> {
        test_parse_and_compute_str::<ComputedPaddingTop>(source, take_length)
    }

    // https://github.com/web-platform-tests/wpt/blob/c1350e3eade197000e49d3a7722a3765ee3d6818/css/css-values/absolute-length-units-001.html
    #[test]
    fn wpt_absolute_length_units_001() {
        let cases = [
            ("96px", "2.54cm"),
            ("2.54cm", "25.4mm"),
            ("25.4mm", "101.6q"),
            ("101.6q", "1in"),
            ("1in", "6pc"),
            ("6pc", "72pt"),
            ("72pt", "96px"),
        ];

        for (a, b) in cases {
            assert_compute_ok_and_eq(a, b, compute_as_padding_top);
        }
    }

    // https://github.com/web-platform-tests/wpt/blob/c1350e3eade197000e49d3a7722a3765ee3d6818/css/css-values/q-unit-case-insensitivity-001.html
    #[test]
    fn wpt_q_unit_case_insensitivity_001() {
        assert_compute_ok_and_eq("105.83333Q", "105.83333q", compute_as_padding_top);
    }
}
