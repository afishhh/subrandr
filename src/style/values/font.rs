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

impl PeekParse for FontFamily {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        let mut result = Vec::new();

        let mut current = String::new();
        let mut first = true;
        loop {
            if !current.is_empty() && lk.peek(End) {
                result.reserve_exact(1);
                result.push(current.as_str().into());
                return Ok(Some(Self {
                    families: result.into(),
                }));
            } else if !current.is_empty() && lk.peek_skip(Token![,], stream) {
                result.push(current.as_str().into());
                current.clear();
                first = true;
            } else {
                if !first {
                    current.push(' ');
                }
                first = false;

                if lk.peek(Ident) {
                    current.extend(stream.parse::<Ident>()?.value().unescape_iter());
                } else if lk.peek(LitString) {
                    current.extend(stream.parse::<LitString>()?.value().unescape_iter());
                } else {
                    if current.is_empty() && first {
                        return Ok(None);
                    }
                    return Err(lk.error());
                }
            }

            *lk = stream.lookahead1();
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

impl PeekParse for FontWeight {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        Ok(Some(if lk.peek_skip("normal", stream) {
            Self::Normal
        } else if lk.peek_skip("bold", stream) {
            Self::Bold
        } else if lk.peek(Number) {
            let number = stream.parse::<Number>()?;
            let value = number.value().to_f64();
            if value < 1.0 || value > 1000.0 {
                return Err(ParseError::new(
                    number,
                    "number outside allowed range [1, 1000]",
                ));
            }
            Self::Value(I16Dot16::from_f64(value))
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

impl PeekParse for FontSize {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        Ok(Some(if let Some(lp) = Length::peek_parse(stream, lk)? {
            // TODO: must be > 0
            Self::Length(lp)
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

impl PeekParse for FontStyle {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        Ok(Some(if lk.peek_skip("normal", stream) {
            Self::Normal
        } else if lk.peek_skip("italic", stream) {
            Self::Italic
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

impl PeekParse for FontFeatureSettings {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        Ok(Some(if lk.peek_skip("normal", stream) {
            Self::Normal
        } else if let Some(tag) = FontFeatureTag::peek_parse(stream, lk)? {
            let mut tags = vec![tag];
            loop {
                let mut lk = stream.lookahead1();
                if lk.peek(End) {
                    break;
                }
                let Some(tag) = FontFeatureTag::peek_parse(stream, &mut lk)? else {
                    return Err(lk.error());
                };
                tags.push(tag);
            }

            Self::Tags(tags)
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

impl PeekParse for FontFeatureTag {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        if !lk.peek(LitString) {
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

        *lk = stream.lookahead1();
        let value = if lk.peek(LitInt) {
            let int = stream.parse::<LitInt>()?;
            Some(FontFeatureTagValue::Integer(int.to_u32().ok_or_else(
                || ParseError::new(string, "OpenType tag value too large (must be < 2^32)"),
            )?))
        } else if lk.peek_skip("on", stream) {
            Some(FontFeatureTagValue::On)
        } else if lk.peek_skip("off", stream) {
            Some(FontFeatureTagValue::Off)
        } else {
            None
        };

        Ok(Some(Self { tag, value }))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn abcd() {
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
