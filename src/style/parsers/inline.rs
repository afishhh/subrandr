//! Properties from the [css-inline](https://drafts.csswg.org/css-inline-3) spec.

use super::*;
use crate::{
    csssyn::token::Number,
    layout::FixedL,
    style::computed::{InlineSizing, LineHeight},
};

// https://drafts.csswg.org/css-inline-3/#line-fill
pub(super) fn take_inline_sizing(stream: &mut ParseStream) -> Result<InlineSizing, ParseError> {
    Ok(if stream.peek_skip("normal") {
        InlineSizing::Normal
    } else if stream.peek_skip("stretch") {
        InlineSizing::Stretch
    } else {
        return Err(stream.lookahead_error());
    })
}

// https://drafts.csswg.org/css-inline-3/#line-height-property
pub(super) fn take_line_height(stream: &mut ParseStream) -> Result<LineHeight, ParseError> {
    Ok(if stream.peek_skip("normal") {
        LineHeight::Normal
    } else if stream.peek(Number) {
        let number = stream.parse::<Number>()?;
        let value = number.value().to_finite_f64(number)?;
        if value < 0.0 {
            return Err(ParseError::new(number, "line-height cannot be negative"));
        }
        return Ok(LineHeight::Value(FixedL::from_f64(value)));
    } else {
        return Err(stream.lookahead_error());
    })
}
