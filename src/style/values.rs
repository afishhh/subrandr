//! Code for parsing and computing values of CSS properties.
//!
//! Note that a lot of properties are only partially implemented and it is
//! by design. subrandr ***is not** a browser project*.
//!
//! These parsers serve two main goals:
//! 1. To allow parsing values in WebVTT stylesheets.
//!    For this only a limited set of properties is required:
//!    - [X] 'color'
//!    - [ ] 'opacity'
//!    - [X] 'visibility'
//!    - [ ] 'text-decoration' and longhands
//!    - [X] 'text-shadow'
//!    - [ ] 'background' and longhands
//!    - [ ] 'outline' and longhands
//!    - [ ] 'font' and longhands
//!    - [ ] 'white-space'
//!    - [ ] 'text-combine-upright'
//!    - [ ] 'ruby-position'
//! 2. To allow parsing styles provided via a stable layout API.
//!    For this we don't strictly *need* anything but most computed style values
//!    should be exposed as a property using some subset of CSS-specified syntax.
//!
//! The above constriants mean that subrandr can get away ignoring some inconvenient parts
//! of CSS style computation like computing percentages relative to containing block sizes.
//! Thus, to keep style computation relatively simple, properties only required for the
//! layout API should only implement a reasonable subset and avoid complicated procedures
//! like keeping track of the containing block's width/height.
//!
//! Some things that are *not* happening to reduce complexity:
//! - Shorthands not required by WebVTT will not be implemented.
//!   With exceptions, for example `text-align` would be weird to omit even though it is
//!   a shorthand.
//! - Deprecated aliases / compatiblity values for properties not required by WebVTT
//!   will not be implemented.
use crate::{
    csssyn::{self, buffer::Cursor, value::*, ParseError},
    style::ComputedStyle,
};

pub(super) trait PeekParse: Sized {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError>;
}

pub(super) trait PropertyValue<V>: PeekParse + Sized {
    fn compute(self, parent: &V) -> V;
}

impl<T: PeekParse + Sized> PropertyValue<T> for T {
    fn compute(self, _parent: &T) -> T {
        self
    }
}

pub(super) type ParseAndComputeFn = fn(
    result: &mut ComputedStyle,
    source: Cursor,
    parent: &ComputedStyle,
) -> Result<(), ParseError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GlobalKeywordOr<I> {
    Initial,
    Inherit,
    Unset,
    Value(I),
}

impl<'a, I: PeekParse> Parse<'a> for GlobalKeywordOr<I> {
    fn parse(stream: &ParseStream<'a>) -> Result<Self, ParseError> {
        let mut lk = stream.lookahead1();
        Ok(if lk.peek_skip("initial", stream) {
            Self::Initial
        } else if lk.peek_skip("inherit", stream) {
            Self::Inherit
        } else if lk.peek_skip("unset", stream) {
            Self::Unset
        } else if let Some(value) = I::peek_parse(stream, &mut lk)? {
            Self::Value(value)
        } else {
            return Err(lk.error());
        })
    }
}

pub(super) fn parse_and_compute<P: super::ComputedProperty, PV: PropertyValue<P::Value>>(
    result: &mut ComputedStyle,
    source: Cursor,
    parent: &ComputedStyle,
) -> Result<(), ParseError> {
    // https://drafts.csswg.org/css-cascade/#defaulting-keywords
    match csssyn::value::parse_cursor::<GlobalKeywordOr<PV>>(source)? {
        GlobalKeywordOr::Initial => P::set(result, P::get(&ComputedStyle::DEFAULT).clone()),
        GlobalKeywordOr::Inherit => P::set(result, P::get(parent).clone()),
        GlobalKeywordOr::Unset => {
            if P::INHERITED {
                P::set(result, P::get(parent).clone())
            } else {
                P::set(result, P::get(&ComputedStyle::DEFAULT).clone())
            }
        }
        GlobalKeywordOr::Value(value) => P::set(result, PV::compute(value, P::get(parent))),
    }
    Ok(())
}

#[cfg(test)]
fn test_parse_and_compute_str<P: super::ComputedProperty, PV: PropertyValue<P::Value>>(
    source: &str,
) -> Result<P::Value, ParseError> {
    use crate::csssyn::TokenBuffer;

    let mut result = ComputedStyle::DEFAULT;
    parse_and_compute::<P, PV>(
        &mut result,
        TokenBuffer::from_source(source)?.start(),
        &ComputedStyle::DEFAULT,
    )?;

    Ok(P::get(&result).clone())
}

#[cfg(test)]
#[track_caller]
fn assert_compute_ok_and_eq<O: PartialEq + std::fmt::Debug, E: std::error::Error>(
    a: &str,
    b: &str,
    mut compute: impl FnMut(&str) -> Result<O, E>,
) {
    let mut must_compute = |s: &str| match compute(s) {
        Ok(v) => v,
        Err(e) => panic!("Failed to compute {s:?}: {e}"),
    };

    assert_eq!(
        must_compute(a),
        must_compute(b),
        "compute({a:?}) != compute({b:?})"
    );
}

mod color;
mod display;
mod font;
mod inline;
mod length;
mod text;
mod text_decor;
mod writing_modes;
pub use color::*;
pub use font::*;
pub use length::*;
pub use text::*;
pub use text_decor::*;
