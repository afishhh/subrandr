//! Properties from the [css-fonts](https://drafts.csswg.org/css-fonts-4/#basic-font-props) spec.
use util::{
    math::{I16Dot16, I26Dot6},
    rc::Rc,
};

use super::*;
use crate::{
    csssyn::token::*,
    style::computed::{
        FontFeatureSettings as ComputedFontFeatureSettings, FontSlant as ComputedFontSlant,
    },
    text::OpenTypeTag,
};

// https://drafts.csswg.org/css-fonts-4/#font-family-prop
// TODO: This does not treat generic families as specified by the spec.
//       I don't think that is really worth it for us to implement though.
pub struct FontFamily {
    pub families: Rc<[Rc<str>]>,
}

impl Parse<'_> for Option<FontFamily> {
    fn parse(stream: &mut ParseStream) -> Result<Self, ParseError> {
        let mut result = Vec::new();

        let mut current = String::new();
        let mut first = true;
        loop {
            if !current.is_empty() && stream.peek(End) {
                result.reserve_exact(1);
                result.push(current.as_str().into());
                return Ok(Some(FontFamily {
                    families: result.into(),
                }));
            } else if !current.is_empty() && stream.peek_skip(Token![,]) {
                result.push(current.as_str().into());
                current.clear();
                first = true;
                continue;
            }

            if !first {
                current.push(' ');
            }
            first = false;

            if stream.peek(Ident) {
                current.extend(stream.parse::<Ident>()?.value().unescape_iter());
            } else if stream.peek(LitString) {
                current.extend(stream.parse::<LitString>()?.value().unescape_iter());
            } else {
                return Err(stream.lookahead_error());
            }
        }
    }
}

impl PropertyValue<Rc<[Rc<str>]>> for FontFamily {
    fn compute(self, _parent: &Rc<[Rc<str>]>) -> Rc<[Rc<str>]> {
        self.families
    }
}

// https://drafts.csswg.org/css-fonts-4/#font-weight-prop
// `bolder` and `lighter` relative keywords omitted
#[derive(Debug, Clone, Copy)]
pub enum FontWeight {
    Normal,
    Bold,
    Value(I16Dot16),
}

impl Parse<'_> for Option<FontWeight> {
    fn parse(stream: &mut ParseStream) -> Result<Self, ParseError> {
        Ok(Some(if stream.peek_skip("normal") {
            FontWeight::Normal
        } else if stream.peek_skip("bold") {
            FontWeight::Bold
        } else if stream.peek(Number) {
            let number = stream.parse::<Number>()?;
            let value = number.value().to_f64();
            if value < 1.0 || value > 1000.0 {
                return Err(ParseError::new(
                    number,
                    "number outside allowed range [1, 1000]",
                ));
            }
            FontWeight::Value(I16Dot16::from_f64(value))
        } else {
            return Ok(None);
        }))
    }
}

impl PropertyValue<I16Dot16> for FontWeight {
    fn compute(self, _parent: &I16Dot16) -> I16Dot16 {
        match self {
            Self::Normal => I16Dot16::new(400),
            Self::Bold => I16Dot16::new(700),
            Self::Value(value) => value,
        }
    }
}

// https://drafts.csswg.org/css-fonts-4/#font-size-prop
// TODO: consider relative sizes and length-percentage
#[derive(Debug, Clone, Copy)]
pub enum FontSize {
    Length(Length),
}

impl Parse<'_> for Option<FontSize> {
    fn parse<'a>(stream: &mut ParseStream<'a>) -> Result<Self, ParseError> {
        Ok(Some(if let Some(lp) = stream.parse()? {
            // TODO: must be > 0
            FontSize::Length(lp)
        } else {
            return Ok(None);
        }))
    }
}

impl PropertyValue<I26Dot6> for FontSize {
    fn compute(self, _parent: &I26Dot6) -> I26Dot6 {
        match self {
            Self::Length(lp) => lp.compute().to_unscaled_pixels(),
        }
    }
}

// https://drafts.csswg.org/css-fonts-4/#font-style-prop
// TODO: Most variants left unimplemented
#[derive(Debug, Clone, Copy)]
pub enum FontStyle {
    Normal,
    Italic,
}

impl Parse<'_> for Option<FontStyle> {
    fn parse(stream: &mut ParseStream) -> Result<Self, ParseError> {
        Ok(Some(if stream.peek_skip("normal") {
            FontStyle::Normal
        } else if stream.peek_skip("italic") {
            FontStyle::Italic
        } else {
            return Ok(None);
        }))
    }
}

impl PropertyValue<ComputedFontSlant> for FontStyle {
    fn compute(self, _parent: &ComputedFontSlant) -> ComputedFontSlant {
        match self {
            FontStyle::Normal => ComputedFontSlant::Regular,
            FontStyle::Italic => ComputedFontSlant::Italic,
        }
    }
}

// https://drafts.csswg.org/css-fonts-4/#propdef-font-feature-settings
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontFeatureSettings {
    Normal,
    Tags(Vec<FontFeatureTag>),
}

impl Parse<'_> for Option<FontFeatureSettings> {
    fn parse(stream: &mut ParseStream) -> Result<Self, ParseError> {
        Ok(Some(if stream.peek_skip("normal") {
            FontFeatureSettings::Normal
        } else if let Some(tag) = stream.parse()? {
            let mut tags = vec![tag];
            while !stream.peek(End) {
                let Some(tag) = stream.parse()? else {
                    return Err(stream.lookahead_error());
                };
                tags.push(tag);
            }

            FontFeatureSettings::Tags(tags)
        } else {
            return Ok(None);
        }))
    }
}

impl PropertyValue<ComputedFontFeatureSettings> for FontFeatureSettings {
    fn compute(self, _parent: &ComputedFontFeatureSettings) -> ComputedFontFeatureSettings {
        let mut result = ComputedFontFeatureSettings::empty();
        match self {
            FontFeatureSettings::Normal => (),
            FontFeatureSettings::Tags(tags) => {
                for tag in tags {
                    result.set(
                        tag.tag,
                        match tag.value {
                            Some(FontFeatureTagValue::Integer(v)) => v,
                            Some(FontFeatureTagValue::On) | None => 1,
                            Some(FontFeatureTagValue::Off) => 0,
                        },
                    )
                }
            }
        }
        result
    }
}

// https://www.w3.org/TR/css-fonts-4/#feature-tag-value
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontFeatureTag {
    pub tag: OpenTypeTag,
    pub value: Option<FontFeatureTagValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontFeatureTagValue {
    Integer(u32),
    On,
    Off,
}

impl Parse<'_> for Option<FontFeatureTag> {
    fn parse(stream: &mut ParseStream) -> Result<Self, ParseError> {
        if !stream.peek(LitString) {
            return Ok(None);
        }

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
            Some(FontFeatureTagValue::Integer(int.to_u32().ok_or_else(
                || ParseError::new(string, "OpenType tag value too large (must be < 2^32)"),
            )?))
        } else if stream.peek_skip("on") {
            Some(FontFeatureTagValue::On)
        } else if stream.peek_skip("off") {
            Some(FontFeatureTagValue::Off)
        } else {
            None
        };

        Ok(Some(FontFeatureTag { tag, value }))
    }
}

#[cfg(test)]
mod test {
    use util::rc_static;

    use super::*;
    use crate::style::properties;

    fn compute_as_font_family(source: &str) -> Result<Rc<[Rc<str>]>, ParseError> {
        test_parse_and_compute_str::<properties::ComputedFontFamily, FontFamily>(source)
    }

    #[test]
    fn font_family() {
        let expected1: Rc<[Rc<str>]> = rc_static!([rc_static!(str b"Noto Sans Emoji")]);
        assert_eq!(
            compute_as_font_family(r#""Noto" Sans 'Emoji'"#).unwrap(),
            expected1
        );

        let expected2: Rc<[Rc<str>]> =
            rc_static!([rc_static!(str b"Ahem"), rc_static!(str b"Noto Sans")]);
        assert_eq!(
            compute_as_font_family(r#"Ahem, Noto Sans"#).unwrap(),
            expected2
        );

        assert!(compute_as_font_family(r#"Ahem,"#).is_err());
    }

    #[test]
    fn font_feature_settings() {
        assert_eq!(
            csssyn::value::parse_str::<GlobalKeywordOr::<FontFeatureSettings>>(
                r#"'ruby' 12 "silf" off "ab\63 d" 'AAAA' on"#
            )
            .unwrap(),
            GlobalKeywordOr::Value(FontFeatureSettings::Tags(vec![
                FontFeatureTag {
                    tag: OpenTypeTag::FEAT_RUBY,
                    value: Some(FontFeatureTagValue::Integer(12))
                },
                FontFeatureTag {
                    tag: OpenTypeTag::from_bytes(*b"silf"),
                    value: Some(FontFeatureTagValue::Off)
                },
                FontFeatureTag {
                    tag: OpenTypeTag::from_bytes(*b"abcd"),
                    value: None
                },
                FontFeatureTag {
                    tag: OpenTypeTag::from_bytes(*b"AAAA"),
                    value: Some(FontFeatureTagValue::On)
                }
            ]))
        )
    }
}
