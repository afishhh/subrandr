//! Properties from the [css-writing-modes](https://drafts.csswg.org/css-writing-modes-4) spec.

use crate::style::computed::Direction;

use super::*;

impl PeekParse for Direction {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        Ok(Some(if lk.peek_skip("ltr", stream) {
            Self::Ltr
        } else if lk.peek_skip("rtl", stream) {
            Self::Rtl
        } else {
            return Ok(None);
        }))
    }
}
