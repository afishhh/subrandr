//! Properties from the [css-inline](https://drafts.csswg.org/css-inline-3) spec.
use crate::style::computed::InlineSizing;

use super::*;

impl PeekParse for InlineSizing {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        Ok(Some(if lk.peek_skip("normal", stream) {
            Self::Normal
        } else if lk.peek_skip("stretch", stream) {
            Self::Stretch
        } else {
            return Ok(None);
        }))
    }
}
