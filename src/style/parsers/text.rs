//! Properties from the [css-text](https://drafts.csswg.org/css-text-4) spec.
use icu_segmenter::options::{LineBreakStrictness, LineBreakWordOption};

use super::*;
use crate::style::computed::{HorizontalAlignment, WhiteSpaceCollapse};

// https://drafts.csswg.org/css-text-4/#line-break-property
// auto not supported
pub(super) fn take_line_break(stream: &mut ParseStream) -> Result<LineBreakStrictness, ParseError> {
    Ok(if stream.peek_skip("loose") {
        LineBreakStrictness::Loose
    } else if stream.peek_skip("normal") {
        LineBreakStrictness::Normal
    } else if stream.peek_skip("strict") {
        LineBreakStrictness::Strict
    } else if stream.peek_skip("anywhere") {
        LineBreakStrictness::Anywhere
    } else {
        return Err(stream.lookahead_error());
    })
}

// https://drafts.csswg.org/css-text-4/#word-break-property
pub(super) fn take_word_break(stream: &mut ParseStream) -> Result<LineBreakWordOption, ParseError> {
    Ok(if stream.peek_skip("normal") {
        LineBreakWordOption::Normal
    } else if stream.peek_skip("keep-all") {
        LineBreakWordOption::KeepAll
    } else if stream.peek_skip("break-all") {
        LineBreakWordOption::BreakAll
    } else {
        return Err(stream.lookahead_error());
    })
}

// https://drafts.csswg.org/css-text-4/#propdef-white-space-collapse
pub(super) fn take_white_space_collapse(
    stream: &mut ParseStream,
) -> Result<WhiteSpaceCollapse, ParseError> {
    Ok(if stream.peek_skip("collapse") {
        WhiteSpaceCollapse::Collapse
    } else if stream.peek_skip("preserve") {
        WhiteSpaceCollapse::Preserve
    } else if stream.peek_skip("preserve-breaks") {
        WhiteSpaceCollapse::PreserveBreaks
    } else {
        return Err(stream.lookahead_error());
    })
}

// https://drafts.csswg.org/css-text-4/#propdef-text-align
// TODO: consider supporting start and end values since those seem useful
pub(super) fn take_text_align(stream: &mut ParseStream) -> Result<HorizontalAlignment, ParseError> {
    Ok(if stream.peek_skip("left") {
        HorizontalAlignment::Left
    } else if stream.peek_skip("right") {
        HorizontalAlignment::Right
    } else if stream.peek_skip("center") {
        HorizontalAlignment::Center
    } else {
        return Err(stream.lookahead_error());
    })
}
