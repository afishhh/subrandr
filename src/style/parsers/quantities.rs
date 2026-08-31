//! Values from the [other quantities] section of the [css-values](https://drafts.csswg.org/css-values-3) spec.
//!
//! [other quantities]: https://drafts.csswg.org/css-values-4/#other-units

use rasterize::scene::Rotation;

use super::*;
use crate::csssyn::token::*;

// https://drafts.csswg.org/css-values-4/#angles
pub(super) fn take_angle(stream: &mut ParseStream) -> Result<Rotation, ParseError> {
    let dim = stream.parse::<Dimension>()?;
    Ok(if dim.unit().eq_ignore_ascii_case("deg") {
        match dim.value().as_str() {
            "90" => Rotation::Clockwise90,
            "180" => Rotation::FlipXY,
            "270" => Rotation::CounterClockwise90,
            _ => return Err(ParseError::new(dim, "unsupported angle")),
        }
    } else {
        return Err(ParseError::new(dim, "unknown angle unit"));
    })
}

pub(super) fn take_angle_or_zero(stream: &mut ParseStream) -> Result<Rotation, ParseError> {
    if stream.peek_skip(Token![0]) {
        return Ok(Rotation::None);
    }

    take_angle(stream)
}
