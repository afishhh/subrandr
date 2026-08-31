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
//!    - [ ] 'visibility'
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
//! The above constriants mean that subrandr can get away implementing only a subset of
//! CSS style computation and avoid a lot of complexity.
//!
//! Some things that are *not* happening to reduce complexity:
//! - Shorthands not required by WebVTT.
//!   With exceptions, `text-align` would be weird to omit even though it is
//!   a shorthand.
//! - Deprecated aliases / compatiblity values for properties not required by WebVTT.
use crate::{
    csssyn::{buffer::Cursor, parse_stream::*, ParseError},
    style::{properties::*, ComputedProperty, ComputedStyle},
};

pub(super) type ParseAndComputeFn = fn(
    result: &mut ComputedStyle,
    source: Cursor,
    parent: &ComputedStyle,
) -> Result<(), ParseError>;

pub(super) struct DeclarationHandler {
    pub(super) name: &'static str,
    pub(super) parse_and_compute: ParseAndComputeFn,
}

#[inline]
pub(super) fn parse_and_compute<P: ComputedProperty>(
    result: &mut ComputedStyle,
    source: Cursor,
    parent: &ComputedStyle,
    inner: impl Fn(&mut ParseStream) -> Result<P::Value, ParseError>,
) -> Result<(), ParseError> {
    parse_cursor_with(source, |stream| {
        // https://drafts.csswg.org/css-cascade/#defaulting-keywords
        if stream.peek_skip("initial") {
            P::set(result, P::get(&ComputedStyle::DEFAULT).clone())
        } else if stream.peek_skip("inherit") {
            P::set(result, P::get(parent).clone())
        } else if stream.peek_skip("unset") {
            if P::INHERITED {
                P::set(result, P::get(parent).clone())
            } else {
                P::set(result, P::get(&ComputedStyle::DEFAULT).clone())
            }
        } else {
            P::set(result, inner(stream)?)
        }

        Ok(())
    })
}

#[cfg(test)]
fn test_parse_and_compute_str<P: ComputedProperty>(
    source: &str,
    inner: impl Fn(&mut ParseStream) -> Result<P::Value, ParseError>,
) -> Result<P::Value, ParseError> {
    use crate::csssyn::TokenBuffer;

    let mut result = ComputedStyle::DEFAULT;
    parse_and_compute::<P>(
        &mut result,
        TokenBuffer::from_source(source)?.start(),
        &ComputedStyle::DEFAULT,
        inner,
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
mod quantities;
mod text;
mod text_decor;
mod transforms;
mod writing_modes;

macro_rules! longhand {
    ($property: ident, $name: literal, with parent $inner: expr) => {
        DeclarationHandler {
            name: $name,
            parse_and_compute: |result, source, parent| {
                parse_and_compute::<$property>(result, source, parent, |stream| {
                    $inner(stream, parent)
                })
            },
        }
    };
    ($property: ident, $name: literal, $inner: expr) => {
        DeclarationHandler {
            name: $name,
            parse_and_compute: |result, source, parent| {
                parse_and_compute::<$property>(result, source, parent, $inner)
            },
        }
    };
}

pub(super) static DECLARATION_HANDLERS: &[DeclarationHandler] = &[
    longhand!(ComputedFontFamily, "font-family", font::take_font_family),
    longhand!(ComputedFontWeight, "font-weight", font::take_font_weight),
    longhand!(ComputedFontSize, "font-size", font::take_font_size),
    longhand!(ComputedFontSlant, "font-style", font::take_font_style),
    longhand!(
        ComputedFontFeatureSettings,
        "font-feature-settings",
        font::take_font_feature_settings
    ),
    // This is a draft property and the CSSWG seems to want to change its name
    // so expose it as a vendor-specific property for now.
    longhand!(
        ComputedInlineSizing,
        "-sbr-inline-sizing",
        inline::take_inline_sizing
    ),
    longhand!(ComputedLineBreak, "line-break", text::take_line_break),
    longhand!(ComputedWordBreak, "word-break", text::take_word_break),
    longhand!(ComputedTextAlign, "text-align", text::take_text_align),
    // > This section is still under discussion and may change in future drafts.
    longhand!(
        ComputedWhiteSpaceCollapse,
        "-sbr-white-space-collapse",
        text::take_white_space_collapse
    ),
    longhand!(
        ComputedTextDecorationLine,
        "text-decoration-line",
        text_decor::take_text_decoration_line
    ),
    longhand!(
        ComputedTextDecorationColor,
        "text-decoration-color",
        color::take_color
    ),
    longhand!(
        ComputedTextShadows,
        "text-shadow",
        text_decor::take_text_shadow
    ),
    longhand!(
        ComputedDirection,
        "direction",
        writing_modes::take_direction
    ),
    longhand!(
        ComputedColor,
        "color",
        with parent color::take_color_for_color_property
    ),
    longhand!(
        ComputedBackgroundColor,
        "background-color",
        color::take_color
    ),
    longhand!(ComputedWidth, "width", length::take_length_or_auto),
    longhand!(ComputedHeight, "height", length::take_length_or_auto),
    longhand!(ComputedPaddingLeft, "padding-left", length::take_length),
    longhand!(ComputedPaddingRight, "padding-right", length::take_length),
    longhand!(ComputedPaddingTop, "padding-top", length::take_length),
    longhand!(ComputedPaddingBottom, "padding-bottom", length::take_length),
    longhand!(
        ComputedMarginLeft,
        "margin-left",
        length::take_length_or_auto
    ),
    longhand!(
        ComputedMarginRight,
        "margin-right",
        length::take_length_or_auto
    ),
    longhand!(ComputedMarginTop, "margin-top", length::take_length_or_auto),
    longhand!(
        ComputedMarginBottom,
        "margin-bottom",
        length::take_length_or_auto
    ),
    longhand!(
        ComputedWritingMode,
        "writing-mode",
        writing_modes::take_writing_mode
    ),
    longhand!(ComputedTransform, "transform", transforms::take_transform),
];
