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

impl PeekParse for LineBreak {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        Ok(Some(if lk.peek_skip("loose", stream) {
            Self::Loose
        } else if lk.peek_skip("normal", stream) {
            Self::Normal
        } else if lk.peek_skip("strict", stream) {
            Self::Strict
        } else if lk.peek_skip("anywhere", stream) {
            Self::Anywhere
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

impl PeekParse for WordBreak {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        Ok(Some(if lk.peek_skip("normal", stream) {
            Self::Normal
        } else if lk.peek_skip("keep-all", stream) {
            Self::KeepAll
        } else if lk.peek_skip("break-all", stream) {
            Self::BreakAll
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

impl PeekParse for WhiteSpaceCollapse {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        Ok(Some(if lk.peek_skip("collapse", stream) {
            Self::Collapse
        } else if lk.peek_skip("preserve", stream) {
            Self::Preserve
        } else if lk.peek_skip("preserve-breaks", stream) {
            Self::PreserveBreaks
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

impl PeekParse for TextAlign {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        Ok(Some(if lk.peek_skip("left", stream) {
            Self::Left
        } else if lk.peek_skip("right", stream) {
            Self::Right
        } else if lk.peek_skip("center", stream) {
            Self::Center
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
