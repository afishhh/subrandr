//! Properties from the [css-writing-modes](https://drafts.csswg.org/css-writing-modes-4) spec.

use crate::style::computed::Direction;

use super::*;

impl Parse<'_> for Option<Direction> {
    fn parse(stream: &mut ParseStream) -> Result<Self, ParseError> {
        Ok(Some(if stream.peek_skip("ltr") {
            Direction::Ltr
        } else if stream.peek_skip("rtl") {
            Direction::Rtl
        } else {
            return Ok(None);
        }))
    }
}
