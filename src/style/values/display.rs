//! Properties from the [css-display](https://drafts.csswg.org/css-display-3) spec.
use crate::style::computed::Visibility;

use super::*;

impl PeekParse for Visibility {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        Ok(Some(if lk.peek_skip("visible", stream) {
            Self::Visible
        } else if lk.peek_skip("hidden", stream) {
            Self::Hidden
        } else {
            return Ok(None);
        }))
    }
}
