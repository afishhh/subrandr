//! Properties from the [css-transforms](https://drafts.csswg.org/css-transforms-1) spec.

use super::*;
use crate::{csssyn::token::*, style::computed::Transform};

// https://drafts.csswg.org/css-transforms/#transform-property
pub(super) fn take_transform(stream: &mut ParseStream) -> Result<Option<Transform>, ParseError> {
    if stream.peek_skip("none") {
        return Ok(None);
    }

    take_transform_function(stream).map(Some)
}

fn take_transform_function(stream: &mut ParseStream) -> Result<Transform, ParseError> {
    Ok(if stream.peek("rotate(") {
        let func = stream.parse::<FunctionalNotation>()?;

        Transform(parse_cursor_with(func.content(), |stream| {
            quantities::take_angle_or_zero(stream)
        })?)
    } else {
        return Err(stream.lookahead_error());
    })
}
