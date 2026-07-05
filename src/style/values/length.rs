//! Length and length-related values from the [css-values](https://drafts.csswg.org/css-values-3) spec.
use super::*;
use crate::{csssyn::token::*, layout::FixedL, style::computed::Length as ComputedLength};

// https://drafts.csswg.org/css-values-3/#absolute-lengths
#[derive(Debug, Clone, Copy)]
pub enum AbsoluteLength {
    Zero,
    Centimeters(f64),
    Millimeters(f64),
    QuarterMillmeters(f64),
    Inches(f64),
    Picas(f64),
    Points(f64),
    Pixels(f64),
}

impl Parse<'_> for Option<AbsoluteLength> {
    fn parse(stream: &mut ParseStream) -> Result<Self, ParseError> {
        Ok(Some(if stream.peek_skip(Token![0]) {
            AbsoluteLength::Zero
        } else if stream.peek(Dimension) {
            // TODO: compute eagerly here
            // TODO: update lk here
            let dim = stream.parse::<Dimension>()?;
            if dim.unit().eq_ignore_ascii_case("px") {
                AbsoluteLength::Pixels(dim.value().to_finite_f64(dim)?)
            } else if dim.unit().eq_ignore_ascii_case("pt") {
                AbsoluteLength::Points(dim.value().to_finite_f64(dim)?)
            } else if dim.unit().eq_ignore_ascii_case("in") {
                AbsoluteLength::Inches(dim.value().to_finite_f64(dim)?)
            } else if dim.unit().eq_ignore_ascii_case("mm") {
                AbsoluteLength::Millimeters(dim.value().to_finite_f64(dim)?)
            } else if dim.unit().eq_ignore_ascii_case("cm") {
                AbsoluteLength::Centimeters(dim.value().to_finite_f64(dim)?)
            } else if dim.unit().eq_ignore_ascii_case("Q") {
                AbsoluteLength::QuarterMillmeters(dim.value().to_finite_f64(dim)?)
            } else if dim.unit().eq_ignore_ascii_case("pc") {
                AbsoluteLength::Picas(dim.value().to_finite_f64(dim)?)
            } else {
                return Err(ParseError::new(dim, "invalid absolute length unit"));
            }
        } else {
            return Ok(None);
        }))
    }
}

// https://drafts.csswg.org/css-values-3/#lengths
// TODO: Consider implementing font-relative lengths
#[derive(Debug, Clone, Copy)]
pub enum Length {
    Absolute(AbsoluteLength),
}

impl Parse<'_> for Option<Length> {
    fn parse(stream: &mut ParseStream) -> Result<Self, ParseError> {
        Ok(stream
            .parse::<Option<AbsoluteLength>>()?
            .map(Length::Absolute))
    }
}

impl PropertyValue<ComputedLength> for Length {
    fn compute(self, _parent: &ComputedLength) -> ComputedLength {
        self.compute()
    }
}

impl Length {
    pub fn compute(self) -> ComputedLength {
        match self {
            Length::Absolute(absolute) => absolute.compute(),
        }
    }
}

impl AbsoluteLength {
    pub fn compute(self) -> ComputedLength {
        ComputedLength::from_pixels(FixedL::from_f64(match self {
            Self::Zero => return ComputedLength::ZERO,
            Self::Centimeters(centimeters) => centimeters * const { 96.0 / 2.54 },
            Self::Millimeters(millimeters) => millimeters * const { 480.0 / 127.0 },
            Self::QuarterMillmeters(qs) => qs * const { 120.0 / 127.0 },
            Self::Inches(inches) => inches * 96.0,
            Self::Picas(picas) => picas * const { 96.0 / 6.0 },
            Self::Points(points) => points * const { 96.0 / 72.0 },
            Self::Pixels(pixels) => pixels,
        }))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::style::properties;

    fn compute_as_padding_top(source: &str) -> Result<ComputedLength, ParseError> {
        test_parse_and_compute_str::<properties::ComputedPaddingTop, Length>(source)
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
