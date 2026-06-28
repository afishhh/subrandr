use util::math::{I16Dot16, I26Dot6};

use crate::{
    csssyn::value::*,
    layout::FixedL,
    style::computed::{FontSlant as ComputedFontSlant, Length as ComputedLength},
    text::OpenTypeTag,
};

trait PeekParse: Sized {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError>;
}

// https://www.w3.org/TR/css-values-3/#absolute-lengths
#[derive(Debug, Clone, Copy)]
pub enum AbsoluteLength {
    Zero,
    Centimeters(f64),
    Millimeters(f64),
    QuarterMillmeters(f64),
    Inches(f64),
    Picas(f64),
    Points(f64),
    Pixels(f64),
}

impl AbsoluteLength {
    pub fn compute(self) -> ComputedLength {
        match self {
            Self::Zero => ComputedLength::ZERO,
            Self::Centimeters(centimeters) => {
                ComputedLength::from_pixels(FixedL::from_f64(centimeters * const { 96.0 / 2.54 }))
            }
            Self::Millimeters(millimeters) => ComputedLength::from_pixels(FixedL::from_f64(
                millimeters * const { (96.0 / 2.54) / 10.0 },
            )),
            Self::QuarterMillmeters(qs) => {
                ComputedLength::from_pixels(FixedL::from_f64(qs * const { (96.0 / 2.54) / 40.0 }))
            }
            Self::Inches(inches) => ComputedLength::from_pixels(FixedL::from_f64(inches * 96.0)),
            Self::Picas(picas) => {
                ComputedLength::from_pixels(FixedL::from_f64(picas * const { 96.0 / 6.0 }))
            }
            Self::Points(points) => ComputedLength::from_points(FixedL::from_f64(points)),
            Self::Pixels(pixels) => {
                ComputedLength::from_pixels(FixedL::from_f64(pixels * const { 96.0 / 72.0 }))
            }
        }
    }
}

impl PeekParse for AbsoluteLength {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        Ok(Some(if lk.peek(Token![0]) {
            Self::Zero
        } else if lk.peek(Dimension) {
            let dim = stream.parse::<Dimension>()?;
            if dim.unit().eq_ignore_ascii_case("px") {
                Self::Pixels(dim.value().to_f64())
            } else if dim.unit().eq_ignore_ascii_case("pt") {
                Self::Points(dim.value().to_f64())
            } else if dim.unit().eq_ignore_ascii_case("in") {
                Self::Inches(dim.value().to_f64())
            } else if dim.unit().eq_ignore_ascii_case("mm") {
                Self::Millimeters(dim.value().to_f64())
            } else if dim.unit().eq_ignore_ascii_case("cm") {
                Self::Centimeters(dim.value().to_f64())
            } else if dim.unit().eq_ignore_ascii_case("Q") {
                Self::QuarterMillmeters(dim.value().to_f64())
            } else if dim.unit().eq_ignore_ascii_case("pc") {
                Self::Picas(dim.value().to_f64())
            } else {
                return Err(ParseError::new(dim, "invalid absolute length unit"));
            }
        } else {
            return Ok(None);
        }))
    }
}

// https://www.w3.org/TR/css-fonts-4/#font-family-name-value
// TODO: This does not treat generic families as specified by the spec.
//       I don't think this is really worth it for us to implement though.
pub struct FontFamily {
    pub families: Vec<String>,
}

impl PeekParse for FontFamily {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        let mut result = Vec::new();
        loop {
            let mut current = String::new();
            let mut first = true;

            if !result.is_empty() && !current.is_empty() && lk.peek(End) {
                result.push(std::mem::take(&mut current));
                return Ok(Some(Self { families: result }));
            } else if !current.is_empty() && lk.peek(Token![,]) {
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
                    return Err(std::mem::replace(lk, stream.lookahead1()).error());
                }
            }

            *lk = stream.lookahead1();
        }
    }
}

// https://www.w3.org/TR/css-fonts-4/#font-weight-prop but without relative variants.
#[derive(Debug, Clone, Copy)]
pub enum AbsoluteFontWeight {
    Normal,
    Bold,
    Value(I16Dot16),
}

impl AbsoluteFontWeight {
    pub fn compute(self) -> I16Dot16 {
        match self {
            Self::Normal => I16Dot16::new(400),
            Self::Bold => I16Dot16::new(700),
            Self::Value(value) => value,
        }
    }
}

impl PeekParse for AbsoluteFontWeight {
    // `bolder` and `lighter` relative keywords not supported
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        mut lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        Ok(Some(if lk.peek("normal") {
            stream.skip();
            Self::Normal
        } else if lk.peek("bold") {
            stream.skip();
            Self::Bold
        } else if lk.peek(Number) {
            Self::Value(I16Dot16::from_f64(
                stream.parse::<Number>()?.value().to_f64(),
            ))
        } else {
            return Ok(None);
        }))
    }
}

// https://www.w3.org/TR/css-fonts-4/#font-size-prop but without relative variants.
#[derive(Debug, Clone, Copy)]
pub enum AbsoluteFontSize {
    Length(AbsoluteLength),
}

impl AbsoluteFontSize {
    pub fn compute(self) -> I26Dot6 {
        match self {
            Self::Length(absolute_length) => absolute_length.compute().to_unscaled_pixels(),
        }
    }
}

impl PeekParse for AbsoluteFontSize {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        Ok(Some(
            if let Some(absolute_length) = AbsoluteLength::peek_parse(stream, lk)? {
                Self::Length(absolute_length)
            } else {
                return Ok(None);
            },
        ))
    }
}

// https://www.w3.org/TR/css-fonts-4/#font-style-prop
// TODO: Most variants unimplemented.
#[derive(Debug, Clone, Copy)]
pub enum FontSlant {
    Normal,
    Italic,
}

impl FontSlant {
    pub fn compute(self) -> ComputedFontSlant {
        match self {
            Self::Normal => ComputedFontSlant::Regular,
            Self::Italic => ComputedFontSlant::Italic,
        }
    }
}

impl PeekParse for FontSlant {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        Ok(Some(if lk.peek("normal") {
            stream.skip();
            Self::Normal
        } else if lk.peek("italic") {
            stream.skip();
            Self::Italic
        } else {
            return Ok(None);
        }))
    }
}

// https://www.w3.org/TR/css-fonts-4/#propdef-font-feature-settings
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
        Ok(Some(if lk.peek("normal") {
            stream.skip();
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

        // TODO: wrong, these won't be candidates in Lookahead1 later!!
        let value = if stream.peek(LitInt) {
            let int = stream.parse::<LitInt>()?;
            Some(FontFeatureTagValue::Integer(int.to_u32().ok_or_else(
                || ParseError::new(string, "OpenType tag value too large (must be < 2^32)"),
            )?))
        } else if stream.peek("on") {
            stream.skip();
            Some(FontFeatureTagValue::On)
        } else if stream.peek("off") {
            stream.skip();
            Some(FontFeatureTagValue::Off)
        } else {
            None
        };

        Ok(Some(Self { tag, value }))
    }
}

// https://www.w3.org/TR/css-text-3/#line-break-property
#[derive(Debug, Clone, Copy)]
pub enum LineBreak {
    Auto,
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
        Ok(Some(if lk.peek("auto") {
            stream.skip();
            Self::Auto
        } else if lk.peek("loose") {
            stream.skip();
            Self::Loose
        } else if lk.peek("normal") {
            stream.skip();
            Self::Normal
        } else if lk.peek("strict") {
            stream.skip();
            Self::Strict
        } else if lk.peek("anywhere") {
            stream.skip();
            Self::Anywhere
        } else {
            return Ok(None);
        }))
    }
}

// https://www.w3.org/TR/css-text-3/#word-break-property
#[derive(Debug, Clone, Copy)]
pub enum WordBreak {
    Normal,
    KeepAll,
    BreakAll,
    BreakWord,
}

impl PeekParse for WordBreak {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        Ok(Some(if lk.peek("normal") {
            stream.skip();
            Self::Normal
        } else if lk.peek("keep-all") {
            stream.skip();
            Self::KeepAll
        } else if lk.peek("break-all") {
            stream.skip();
            Self::BreakAll
        } else if lk.peek("break-word") {
            stream.skip();
            Self::BreakWord
        } else {
            return Ok(None);
        }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Specified<I> {
    Initial,
    Inherit,
    Unset,
    Value(I),
}

impl<'a, I: PeekParse> Parse<'a> for Specified<I> {
    fn parse(stream: &ParseStream<'a>) -> Result<Self, ParseError> {
        let mut lk = stream.lookahead1();
        Ok(if lk.peek("initial") {
            stream.skip();
            Self::Initial
        } else if lk.peek("inherit") {
            stream.skip();
            Self::Inherit
        } else if lk.peek("unset") {
            stream.skip();
            Self::Unset
        } else if let Some(value) = I::peek_parse(stream, &mut lk)? {
            Self::Value(value)
        } else {
            return Err(lk.error());
        })
    }
}

#[cfg(test)]
mod test {
    use crate::csssyn;

    use super::*;

    #[test]
    fn abcd() {
        assert_eq!(
            csssyn::value::parse_str::<Specified::<FontFeatureSettings>>(
                r#"'ruby' 12 "silf" off "ab\63 d" 'AAAA' on"#
            )
            .unwrap(),
            Specified::Value(FontFeatureSettings::Tags(vec![
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
