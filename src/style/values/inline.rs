//! Properties from the [css-inline](https://drafts.csswg.org/css-inline-3) spec.
use crate::style::computed::InlineSizing;

use super::*;

impl Parse<'_> for Option<InlineSizing> {
    fn parse(stream: &mut ParseStream) -> Result<Self, ParseError> {
        Ok(Some(if stream.peek_skip("normal") {
            InlineSizing::Normal
        } else if stream.peek_skip("stretch") {
            InlineSizing::Stretch
        } else {
            return Ok(None);
        }))
    }
}
