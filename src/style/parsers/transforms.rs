//! Properties from the [css-transforms](https://drafts.csswg.org/css-transforms-1) spec.

use rasterize::scene::Transform;
use util::math::{I16Dot16, I26Dot6, Vec2};

use super::*;
use crate::{csssyn::token::*, style::computed::UnscaledTransform};

// https://drafts.csswg.org/css-transforms-1/#transform-property
pub(super) fn take_transform(
    stream: &mut ParseStream,
) -> Result<Option<UnscaledTransform>, ParseError> {
    if stream.peek_skip("none") {
        return Ok(None);
    }

    let mut result = Transform::IDENTITY;
    loop {
        let next = take_transform_function(stream)?;
        result = result * next;

        if stream.peek_skip(End) {
            return Ok(Some(UnscaledTransform(result)));
        }
    }
}

fn take_transform_function(stream: &mut ParseStream) -> Result<Transform, ParseError> {
    Ok(if stream.peek("rotate(") {
        // https://drafts.csswg.org/css-transforms-1/#funcdef-transform-rotate
        let func = stream.parse::<FunctionalNotation>()?;

        Transform::rotate(parse_cursor_with(func.content(), |stream| {
            quantities::take_angle_or_zero(stream)
        })?)
    } else if stream.peek("scale(") {
        // https://drafts.csswg.org/css-transforms-1/#funcdef-transform-scale
        let func = stream.parse::<FunctionalNotation>()?;

        Transform::scale(parse_cursor_with(func.content(), |stream| {
            let sxn = stream.parse::<Number>()?;
            let syn = if stream.peek_skip(Token![,]) {
                Some(stream.parse::<Number>()?)
            } else {
                None
            };

            // Currently the rasterizer internally requires that scale is non-negative in
            // some places so reject negative scale early.
            const NEGATIVE_SCALE_MSG: &str = "negative scale currently not supported";

            let sx = sxn.value().to_finite_f64(sxn)?;
            if sx < 0.0 {
                return Err(ParseError::new(sxn, NEGATIVE_SCALE_MSG));
            }

            let sy = match syn {
                None => sx,
                Some(n) => {
                    let v = n.value().to_finite_f64(n)?;
                    if v < 0.0 {
                        return Err(ParseError::new(n, NEGATIVE_SCALE_MSG));
                    }
                    v
                }
            };

            Ok(Vec2::new(I16Dot16::from_f64(sx), I16Dot16::from_f64(sy)))
        })?)
    } else if stream.peek("translate(") {
        // https://drafts.csswg.org/css-transforms-1/#funcdef-transform-translate
        let func = stream.parse::<FunctionalNotation>()?;

        Transform::translate(parse_cursor_with(func.content(), |stream| {
            let tx = length::take_length(stream)?.to_f32();
            let ty = if stream.peek_skip(Token![,]) {
                length::take_length(stream)?.to_f32()
            } else {
                0.0
            };

            Ok(Vec2::new(I26Dot6::from_f32(tx), I26Dot6::from_f32(ty)))
        })?)
    } else {
        return Err(stream.lookahead_error());
    })
}

#[cfg(test)]
mod test {
    use rasterize::scene::{LinearTransform, Rotation};
    use util::math::I16Dot16;

    use super::*;

    fn compute_as_transform(source: &str) -> Result<Option<Transform>, ParseError> {
        test_parse_and_compute_str::<ComputedTransform>(source, take_transform)
            .map(|x| x.map(|x| x.0))
    }

    #[test]
    fn simple_compositions() {
        assert_eq!(
            compute_as_transform("translate(20px, 40px) scale(2) translate(10px) scale(1, 2)")
                .unwrap(),
            Some(Transform {
                linear: LinearTransform {
                    rotation: Rotation::None,
                    scale: Vec2::new(I16Dot16::new(2), I16Dot16::new(4))
                },
                translation: Vec2::new(I26Dot6::new(40), I26Dot6::new(40))
            })
        );

        assert_eq!(
            compute_as_transform("translate(40px, 20px) rotate(90deg) scale(2, 1) translate(10px)")
                .unwrap(),
            Some(Transform {
                linear: LinearTransform {
                    rotation: Rotation::Clockwise90,
                    scale: Vec2::new(I16Dot16::ONE, I16Dot16::new(2))
                },
                translation: Vec2::new(I26Dot6::new(40), I26Dot6::new(40))
            })
        );

        assert_compute_ok_and_eq(
            "translate(20px, 40px) scale(2) translate(10px) scale(1, 2)",
            "translate(40px, 40px) scale(2, 4)",
            compute_as_transform,
        );
    }
}
