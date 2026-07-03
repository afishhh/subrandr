//! Properties from the [css-text-decor](https://drafts.csswg.org/css-text-decor-3) spec.

use util::{math::Vec2, rc::Rc};

use crate::{
    csssyn::token::{End, Token},
    style::computed::TextDecorationLines,
};

use super::*;
use crate::style::computed::{Length as ComputedLength, TextShadow as ComputedTextShadow};

impl PeekParse for TextDecorationLines {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
        // TODO: don't make property peekparse return option
    ) -> Result<Option<Self>, ParseError> {
        let mut result = Self::NONE;

        if lk.peek_skip("none", stream) {
            return Ok(Some(result));
        }

        loop {
            if lk.peek_skip("underline", stream) {
                result.underline = true;
            } else if lk.peek_skip("line-through", stream) {
                result.line_through = true;
            } else {
                return Err(lk.error());
            }

            *lk = stream.lookahead1();
            if lk.peek(End) {
                break Ok(Some(result));
            } else if !lk.peek_skip(Token![,], stream) {
                return Err(lk.error());
            }
        }
    }
}

pub struct TextShadows(Vec<(Color, ShadowLengths)>);

struct ShadowLengths {
    offset: Vec2<Length>,
    radius: Option<Length>,
}

impl ShadowLengths {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        let Some(off_x) = Length::peek_parse(stream, lk)? else {
            return Ok(None);
        };
        let off_y = Length::peek_parse(stream, lk)?.ok_or_else(|| lk.error())?;

        // TODO: range check
        let radius = Length::peek_parse(stream, lk)?;

        Ok(Some(ShadowLengths {
            offset: Vec2::new(off_x, off_y),
            radius,
        }))
    }
}

impl PeekParse for TextShadows {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        if lk.peek_skip("none", stream) {
            return Ok(Some(Self(Vec::new())));
        }

        let mut result = Vec::new();
        loop {
            if let Some(color) = Color::peek_parse(stream, lk)? {
                let lengths = ShadowLengths::peek_parse(stream, lk)?.ok_or_else(|| lk.error())?;
                result.push((color, lengths));
            } else if let Some(lengths) = ShadowLengths::peek_parse(stream, lk)? {
                let color = Color::peek_parse(stream, lk)?.ok_or_else(|| lk.error())?;
                result.push((color, lengths));
            }

            *lk = stream.lookahead1();
            if lk.peek(End) {
                break Ok(Some(Self(result)));
            }
        }
    }
}

impl PropertyValue<Rc<[ComputedTextShadow]>> for TextShadows {
    fn compute(self, _parent: &Rc<[ComputedTextShadow]>) -> Rc<[ComputedTextShadow]> {
        let mut result = Vec::with_capacity(self.0.len());
        for (color, lengths) in self.0 {
            result.push(ComputedTextShadow {
                offset: Vec2::new(lengths.offset.x.compute(), lengths.offset.y.compute()),
                blur_radius: lengths.radius.map_or(ComputedLength::ZERO, Length::compute),
                color: color.compute(),
            });
        }
        result.into()
    }
}

#[cfg(test)]
mod test {
    use rasterize::color::BGRA8;
    use util::rc_static;

    use super::*;
    use crate::{
        layout::FixedL,
        style::{computed::Color as ComputedColor, properties},
    };

    fn compute_as_text_shadow(source: &str) -> Result<Rc<[ComputedTextShadow]>, ParseError> {
        test_parse_and_compute_str::<properties::ComputedTextShadows, TextShadows>(source)
    }

    #[test]
    fn text_shadows() {
        let expected1: Rc<[ComputedTextShadow]> = rc_static!([ComputedTextShadow {
            offset: Vec2::new(
                ComputedLength::from_pixels(FixedL::new(1)),
                ComputedLength::from_pixels(FixedL::new(2)),
            ),
            blur_radius: ComputedLength::from_pixels(FixedL::new(3)),
            color: ComputedColor::CurrentColor
        }]);
        assert_eq!(
            compute_as_text_shadow("1px 2px 3px currentcolor").unwrap(),
            expected1
        );

        let expected2: Rc<[ComputedTextShadow]> = rc_static!([ComputedTextShadow {
            offset: Vec2::new(
                ComputedLength::from_pixels(FixedL::new(1)),
                ComputedLength::from_pixels(FixedL::new(2)),
            ),
            blur_radius: ComputedLength::ZERO,
            color: ComputedColor::Srgb(BGRA8::RED)
        }]);
        assert_eq!(compute_as_text_shadow("red 1px 2px 0").unwrap(), expected2);

        let expected3: Rc<[ComputedTextShadow]> = expected2
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
