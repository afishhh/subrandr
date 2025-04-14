//! Properties from the [css-inline](https://drafts.csswg.org/css-inline-3) spec.
use crate::style::computed::InlineSizing;

use super::*;

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
