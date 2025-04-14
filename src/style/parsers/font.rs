//! Properties from the [css-fonts](https://drafts.csswg.org/css-fonts-4/#basic-font-props) spec.
use util::{
    math::{I16Dot16, I26Dot6},
    rc::Rc,
};

use super::*;
use crate::{
    csssyn::token::*,
    style::computed::{FontFeatureSettings, FontSlant},
    text::OpenTypeTag,
};

// https://drafts.csswg.org/css-fonts-4/#font-family-prop
// TODO: This does not treat generic families as specified by the spec.
//       I don't think that is really worth it for us to implement though.
pub(super) fn take_font_family(stream: &mut ParseStream) -> Result<Rc<[Rc<str>]>, ParseError> {
    let mut result = Vec::new();

    let mut current = String::new();
    loop {
        if stream.peek(LitString) {
            current.extend(stream.parse::<LitString>()?.value().unescape_iter());
        } else {
            loop {
                if !current.is_empty() {
                    current.push(' ');
                }

                if stream.peek(Ident) {
                    current.extend(stream.parse::<Ident>()?.value().unescape_iter());
                } else {
                    return Err(stream.lookahead_error());
                }

                if stream.peek(End) || stream.peek(Token![,]) {
                    break;
                }
            }
        }

        result.push(current.as_str().into());
        current.clear();

        if stream.peek(End) {
            return Ok(result.into());
        }

        if !stream.peek_skip(Token![,]) {
            return Err(stream.lookahead_error());
        }
    }
}

// https://drafts.csswg.org/css-fonts-4/#font-weight-prop
// `bolder` and `lighter` relative keywords omitted
pub(super) fn take_font_weight(stream: &mut ParseStream) -> Result<I16Dot16, ParseError> {
    Ok(if stream.peek_skip("normal") {
        I16Dot16::new(400)
    } else if stream.peek_skip("bold") {
        I16Dot16::new(700)
    } else if stream.peek(Number) {
        let number = stream.parse::<Number>()?;
        let value = number.value().to_f64();
        if !(1.0..=1000.0).contains(&value) {
            return Err(ParseError::new(
                number,
                "weight outside allowed range [1, 1000]",
            ));
        }
        I16Dot16::from_f64(value)
    } else {
        return Err(stream.lookahead_error());
    })
}

// https://drafts.csswg.org/css-fonts-4/#font-size-prop
// TODO: consider relative sizes and length-percentage
pub(super) fn take_font_size(stream: &mut ParseStream) -> Result<I26Dot6, ParseError> {
    let length = length::take_length_in_range(stream, length::LengthRange::NONNEGATIVE)?;
    Ok(length.to_unscaled_pixels())
}

// https://drafts.csswg.org/css-fonts-4/#font-style-prop
// TODO: Most variants left unimplemented
pub(super) fn take_font_style(stream: &mut ParseStream) -> Result<FontSlant, ParseError> {
    Ok(if stream.peek_skip("normal") {
        FontSlant::Regular
    } else if stream.peek_skip("italic") {
        FontSlant::Italic
    } else {
        return Err(stream.lookahead_error());
    })
}

// https://drafts.csswg.org/css-fonts-4/#propdef-font-feature-settings
pub(super) fn take_font_feature_settings(
    stream: &mut ParseStream,
) -> Result<FontFeatureSettings, ParseError> {
    let mut result = FontFeatureSettings::empty();

    if !stream.peek_skip("normal") {
        loop {
            let tag: FontFeatureSetting = stream.parse()?;

            result.set(
                tag.tag,
                match tag.value {
                    Some(FontFeatureValue::Integer(v)) => v,
                    Some(FontFeatureValue::On) | None => 1,
                    Some(FontFeatureValue::Off) => 0,
                },
            );

            if stream.peek(End) {
                break;
            }
        }
    }

    Ok(result)
}

// https://www.w3.org/TR/css-fonts-4/#feature-tag-value
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FontFeatureSetting {
    pub tag: OpenTypeTag,
    pub value: Option<FontFeatureValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontFeatureValue {
    Integer(u32),
    On,
    Off,
}

impl Parse<'_> for FontFeatureSetting {
    fn parse(stream: &mut ParseStream) -> Result<Self, ParseError> {
        let string = stream.parse::<LitString>()?;
        let name = string.value().to_string();
        let Some(ascii_bytes) = <[u8; 4]>::try_from(name.as_bytes())
            .ok()
            .filter(|b| b.iter().all(|b| (0x20..0x7E).contains(b)))
        else {
            return Err(ParseError::new(
                string,
                "OpenType tag must consist of exactly four ASCII characters",
            ));
        };
        let tag = OpenTypeTag::from_bytes(ascii_bytes);

        let value = if stream.peek(LitInt) {
            let int = stream.parse::<LitInt>()?;
            Some(FontFeatureValue::Integer(int.to_u32().ok_or_else(
                || ParseError::new(string, "OpenType tag value outside allowed range [0, 2^32)"),
            )?))
        } else if stream.peek_skip("on") {
            Some(FontFeatureValue::On)
        } else if stream.peek_skip("off") {
            Some(FontFeatureValue::Off)
        } else {
            None
        };

        Ok(FontFeatureSetting { tag, value })
    }
}

#[cfg(test)]
mod test {
    use util::rc_static;

    use super::*;

    fn compute_as_font_family(source: &str) -> Result<Rc<[Rc<str>]>, ParseError> {
        test_parse_and_compute_str::<ComputedFontFamily>(source, take_font_family)
    }

    #[test]
    fn font_family() {
        let expected: Rc<[Rc<str>]> =
            rc_static!([rc_static!(str b"Ahem"), rc_static!(str b"Noto Sans")]);
        assert_eq!(
            compute_as_font_family(r#"Ahem, Noto Sans"#).unwrap(),
            expected
        );

        assert!(compute_as_font_family(r#"Ahem,"#).is_err());

        // https://drafts.csswg.org/css-fonts-4/#ex-no-unquoted-punctuation
        assert!(compute_as_font_family(r#"Red/Black, sans-serif"#).is_err());
        assert!(compute_as_font_family(r#""Lucida" Grande, sans-serif"#).is_err());
        assert!(compute_as_font_family(r#"Ahem!, sans-serif"#).is_err());
        assert!(compute_as_font_family(r#"test@foo, sans-serif"#).is_err());
        assert!(compute_as_font_family(r#"#POUND, sans-serif"#).is_err());
        assert!(compute_as_font_family(r#"Hawaii 5-0, sans-serif"#).is_err());

        // https://drafts.csswg.org/css-fonts-4/#ex-best-quote
        let expected: Rc<[Rc<str>]> = rc_static!([
            rc_static!(str b"New Century Schoolbook"),
            rc_static!(str b"serif")
        ]);
        assert_eq!(
            compute_as_font_family(r#""New Century Schoolbook", serif"#).unwrap(),
            expected
        );
        let expected: Rc<[Rc<str>]> =
            rc_static!([rc_static!(str b"21st Century"), rc_static!(str b"fantasy")]);
        assert_eq!(
            compute_as_font_family(r#"'21st Century', fantasy"#).unwrap(),
            expected
        );
    }

    fn compute_as_font_feature_settings(source: &str) -> Result<FontFeatureSettings, ParseError> {
        test_parse_and_compute_str::<ComputedFontFeatureSettings>(
            source,
            take_font_feature_settings,
        )
    }

    #[test]
    fn font_feature_settings() {
        assert_eq!(
            compute_as_font_feature_settings(r#"'ruby' 12 "silf" off "ab\63 d" 'AAAA' on"#)
                .unwrap(),
            {
                let mut result = FontFeatureSettings::empty();
                result.set(OpenTypeTag::FEAT_RUBY, 12);
                result.set(OpenTypeTag::from_bytes(*b"silf"), 0);
                result.set(OpenTypeTag::from_bytes(*b"abcd"), 1);
                result.set(OpenTypeTag::from_bytes(*b"AAAA"), 1);
                result
            }
        )
    }
}
