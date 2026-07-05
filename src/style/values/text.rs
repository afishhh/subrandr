//! Properties from the [css-text](https://drafts.csswg.org/css-text-4) spec.
use icu_segmenter::options::{LineBreakStrictness, LineBreakWordOption};

use super::*;
use crate::style::computed::{HorizontalAlignment, WhiteSpaceCollapse};

// https://drafts.csswg.org/css-text-4/#line-break-property
#[derive(Debug, Clone, Copy)]
pub enum LineBreak {
    // auto not supported
    Loose,
    Normal,
    Strict,
    Anywhere,
}

impl Parse<'_> for Option<LineBreak> {
    fn parse(stream: &mut ParseStream) -> Result<Self, ParseError> {
        Ok(Some(if stream.peek_skip("loose") {
            LineBreak::Loose
        } else if stream.peek_skip("normal") {
            LineBreak::Normal
        } else if stream.peek_skip("strict") {
            LineBreak::Strict
        } else if stream.peek_skip("anywhere") {
            LineBreak::Anywhere
        } else {
            return Ok(None);
        }))
    }
}

impl PropertyValue<LineBreakStrictness> for LineBreak {
    fn compute(self, _parent: &LineBreakStrictness) -> LineBreakStrictness {
        match self {
            Self::Loose => LineBreakStrictness::Loose,
            Self::Normal => LineBreakStrictness::Normal,
            Self::Strict => LineBreakStrictness::Strict,
            Self::Anywhere => LineBreakStrictness::Anywhere,
        }
    }
}

// https://drafts.csswg.org/css-text-4/#word-break-property
#[derive(Debug, Clone, Copy)]
pub enum WordBreak {
    Normal,
    KeepAll,
    BreakAll,
    // break-word not supported
}

impl Parse<'_> for Option<WordBreak> {
    fn parse(stream: &mut ParseStream) -> Result<Self, ParseError> {
        Ok(Some(if stream.peek_skip("normal") {
            WordBreak::Normal
        } else if stream.peek_skip("keep-all") {
            WordBreak::KeepAll
        } else if stream.peek_skip("break-all") {
            WordBreak::BreakAll
        } else {
            return Ok(None);
        }))
    }
}

impl PropertyValue<LineBreakWordOption> for WordBreak {
    fn compute(self, _parent: &LineBreakWordOption) -> LineBreakWordOption {
        match self {
            Self::Normal => LineBreakWordOption::Normal,
            Self::KeepAll => LineBreakWordOption::KeepAll,
            Self::BreakAll => LineBreakWordOption::BreakAll,
        }
    }
}

impl Parse<'_> for Option<WhiteSpaceCollapse> {
    fn parse(stream: &mut ParseStream) -> Result<Self, ParseError> {
        Ok(Some(if stream.peek_skip("collapse") {
            WhiteSpaceCollapse::Collapse
        } else if stream.peek_skip("preserve") {
            WhiteSpaceCollapse::Preserve
        } else if stream.peek_skip("preserve-breaks") {
            WhiteSpaceCollapse::PreserveBreaks
        } else {
            return Ok(None);
        }))
    }
}

// https://drafts.csswg.org/css-text-4/#propdef-text-align
// TODO: consider supporting start and end values since those seem useful
#[derive(Debug, Clone, Copy)]
pub enum TextAlign {
    Left,
    Right,
    Center,
}

impl Parse<'_> for Option<TextAlign> {
    fn parse(stream: &mut ParseStream) -> Result<Self, ParseError> {
        Ok(Some(if stream.peek_skip("left") {
            TextAlign::Left
        } else if stream.peek_skip("right") {
            TextAlign::Right
        } else if stream.peek_skip("center") {
            TextAlign::Center
        } else {
            return Ok(None);
        }))
    }
}

impl PropertyValue<HorizontalAlignment> for TextAlign {
    fn compute(self, _parent: &HorizontalAlignment) -> HorizontalAlignment {
        match self {
            Self::Left => HorizontalAlignment::Left,
            Self::Right => HorizontalAlignment::Right,
            Self::Center => HorizontalAlignment::Center,
        }
    }
}
