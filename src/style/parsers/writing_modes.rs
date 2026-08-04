//! Properties from the [css-writing-modes](https://drafts.csswg.org/css-writing-modes-4) spec.

use crate::style::computed::{Direction, WritingMode};

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

pub(super) fn take_writing_mode(stream: &mut ParseStream) -> Result<WritingMode, ParseError> {
    Ok(if stream.peek_skip("horizontal-tb") {
        WritingMode::HorizontalTtb
    } else if stream.peek_skip("vertical-rl") {
        WritingMode::VerticalRtl
    } else if stream.peek_skip("vertical-lr") {
        WritingMode::VerticalLtr
    } else if stream.peek_skip("sideways-rl") {
        WritingMode::SidewaysRtl
    } else {
        return Err(stream.lookahead_error());
    })
}
