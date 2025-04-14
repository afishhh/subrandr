//! Properties from the [css-display](https://drafts.csswg.org/css-display-3) spec.
use crate::style::computed::Visibility;

use super::*;

impl Parse<'_> for Option<Visibility> {
    fn parse<'a>(stream: &mut ParseStream<'a>) -> Result<Self, ParseError> {
        Ok(Some(if stream.peek_skip("visible") {
            Visibility::Visible
        } else if stream.peek_skip("hidden") {
            Visibility::Hidden
        } else {
            return Ok(None);
        }))
    }
}
