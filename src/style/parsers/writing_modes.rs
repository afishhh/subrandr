//! Properties from the [css-writing-modes](https://drafts.csswg.org/css-writing-modes-4) spec.

use crate::style::computed::Direction;

use super::*;

pub(super) fn take_direction(stream: &mut ParseStream) -> Result<Direction, ParseError> {
    Ok(if stream.peek_skip("ltr") {
        Direction::Ltr
    } else if stream.peek_skip("rtl") {
        Direction::Rtl
    } else {
        return Err(stream.lookahead_error());
    })
}
