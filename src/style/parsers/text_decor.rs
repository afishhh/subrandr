//! Properties from the [css-text-decor](https://drafts.csswg.org/css-text-decor-3) spec.

use util::{math::Vec2, rc::Rc};

use crate::{
    csssyn::token::{End, Token},
    style::computed::TextDecorationLines,
};

use super::*;
use crate::style::computed::{Length, TextShadow};

pub(super) fn take_text_decoration_line(
    stream: &mut ParseStream,
) -> Result<TextDecorationLines, ParseError> {
    let mut result = TextDecorationLines::NONE;

    if stream.peek_skip("none") {
        return Ok(result);
    }

    loop {
        if stream.peek_skip("underline") {
            result.underline = true;
        } else if stream.peek_skip("line-through") {
            result.line_through = true;
        } else {
            return Err(stream.lookahead_error());
        }

        if stream.peek(End) {
            break Ok(result);
        } else if !stream.peek_skip(Token![,]) {
            return Err(stream.lookahead_error());
        }
    }
}

pub(super) fn take_text_shadow(stream: &mut ParseStream) -> Result<Rc<[TextShadow]>, ParseError> {
    if stream.peek_skip("none") {
        return Ok(Rc::default());
    }

    let mut result = Vec::new();
    loop {
        let (lengths, color) = if let Some(color) = color::try_take_color(stream)? {
            let lengths = stream
                .parse::<Option<ShadowLengths>>()?
                .ok_or_else(|| stream.lookahead_error())?;
            (lengths, color)
        } else if let Some(lengths) = stream.parse()? {
            let color = color::take_color(stream)?;
            (lengths, color)
        } else {
            return Err(stream.lookahead_error());
        };

        result.push(TextShadow {
            offset: lengths.offset,
            blur_radius: lengths.radius.unwrap_or(Length::ZERO),
            color,
        });

        if stream.peek(End) {
            break Ok(result.into());
        }
        stream.parse::<Token![,]>()?;
    }
}

struct ShadowLengths {
    offset: Vec2<Length>,
    radius: Option<Length>,
}

impl Parse<'_> for Option<ShadowLengths> {
    fn parse(stream: &mut ParseStream) -> Result<Self, ParseError> {
        let Some(off_x) = length::try_take_length(stream)? else {
            return Ok(None);
        };
        let off_y = length::take_length(stream)?;
        let radius = length::try_take_length_in_range(stream, length::LengthRange::NONNEGATIVE)?;

        Ok(Some(ShadowLengths {
            offset: Vec2::new(off_x, off_y),
            radius,
        }))
    }
}

#[cfg(test)]
mod test {
    use rasterize::color::BGRA8;
    use util::rc_static;

    use super::*;
    use crate::{layout::FixedL, style::computed::Color};

    fn compute_as_text_shadow(source: &str) -> Result<Rc<[TextShadow]>, ParseError> {
        test_parse_and_compute_str::<ComputedTextShadows>(source, take_text_shadow)
    }

    #[test]
    fn text_shadows() {
        let expected1: Rc<[TextShadow]> = rc_static!([TextShadow {
            offset: Vec2::new(
                Length::from_pixels(FixedL::new(1)),
                Length::from_pixels(FixedL::new(2)),
            ),
            blur_radius: Length::from_pixels(FixedL::new(3)),
            color: Color::CurrentColor
        }]);
        assert_eq!(
            compute_as_text_shadow("1px 2px 3px currentcolor").unwrap(),
            expected1
        );

        let expected2: Rc<[TextShadow]> = rc_static!([TextShadow {
            offset: Vec2::new(
                Length::from_pixels(FixedL::new(1)),
                Length::from_pixels(FixedL::new(2)),
            ),
            blur_radius: Length::ZERO,
            color: Color::Srgb(BGRA8::RED)
        }]);
        assert_eq!(compute_as_text_shadow("red 1px 2px 0").unwrap(), expected2);

        let expected3: Rc<[TextShadow]> = expected2
            .iter()
            .cloned()
            .chain(expected1.iter().cloned())
            .collect::<Vec<_>>()
            .into();
        assert_eq!(
            compute_as_text_shadow("red 1px 2px, 1px 2px 3px currentcolor").unwrap(),
            expected3
        );
    }
}
