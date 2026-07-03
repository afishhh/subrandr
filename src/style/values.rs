use icu_segmenter::options::{LineBreakStrictness, LineBreakWordOption};
use rasterize::color::BGRA8;
use util::{
    math::{I16Dot16, I26Dot6},
    rc::Rc,
};

use crate::{
    csssyn::{
        self,
        buffer::Cursor,
        peek::{End, Token},
        token::{
            Dimension, FunctionalNotation, Hash, Ident, LitInt, LitString, Number, Percentage,
        },
        value::*,
        ParseError,
    },
    layout::FixedL,
    style::{
        computed::{
            Color as ComputedColor, FontFeatureSettings as ComputedFontFeatureSettings,
            FontSlant as ComputedFontSlant, Length as ComputedLength,
        },
        ComputedStyle,
    },
    text::OpenTypeTag,
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

pub(super) type ParseAndComputeFn = fn(
    result: &mut ComputedStyle,
    source: Cursor,
    parent: &ComputedStyle,
) -> Result<(), ParseError>;

pub(super) fn parse_and_compute<P: super::ComputedProperty, PV: PropertyValue<P::Value>>(
    result: &mut ComputedStyle,
    source: Cursor,
    parent: &ComputedStyle,
) -> Result<(), ParseError> {
    let specified = csssyn::value::parse_cursor::<Specified<PV>>(source)?;
    // https://drafts.csswg.org/css-cascade/#defaulting-keywords
    match specified {
        Specified::Initial => P::set(result, P::get(&ComputedStyle::DEFAULT).clone()),
        Specified::Inherit => P::set(result, P::get(parent).clone()),
        Specified::Unset => {
            if P::INHERITED {
                P::set(result, P::get(parent).clone())
            } else {
                P::set(result, P::get(&ComputedStyle::DEFAULT).clone())
            }
        }
        Specified::Value(value) => P::set(result, PV::compute(value, P::get(parent))),
    }
    Ok(())
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

// https://www.w3.org/TR/css-values-3/#lengths
// TODO: relative lengths
#[derive(Debug, Clone, Copy)]
pub enum Length {
    Absolute(AbsoluteLength),
}

impl PeekParse for Length {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        AbsoluteLength::peek_parse(stream, lk).map(|x| x.map(Length::Absolute))
    }
}

impl PropertyValue<ComputedLength> for Length {
    fn compute(self, _parent: &ComputedLength) -> ComputedLength {
        match self {
            Length::Absolute(absolute) => absolute.compute(),
        }
    }
}

// https://www.w3.org/TR/css-fonts-4/#font-family-name-value
// TODO: This does not treat generic families as specified by the spec.
//       I don't think this is really worth it for us to implement though.
pub struct FontFamilies {
    pub families: Rc<[Rc<str>]>,
}

impl PeekParse for FontFamilies {
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
                    return Err(std::mem::replace(lk, stream.lookahead1()).error());
                }
            }

            *lk = stream.lookahead1();
        }
    }
}

impl PropertyValue<Rc<[Rc<str>]>> for FontFamilies {
    fn compute(self, _parent: &Rc<[Rc<str>]>) -> Rc<[Rc<str>]> {
        self.families
    }
}

// https://www.w3.org/TR/css-fonts-4/#font-weight-prop
// TODO: relative weights
#[derive(Debug, Clone, Copy)]
pub enum FontWeight {
    Normal,
    Bold,
    Value(I16Dot16),
}

impl PeekParse for FontWeight {
    // `bolder` and `lighter` relative keywords not supported
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        Ok(Some(if lk.peek_skip("normal", stream) {
            Self::Normal
        } else if lk.peek_skip("bold", stream) {
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

impl PropertyValue<I16Dot16> for FontWeight {
    fn compute(self, _parent: &I16Dot16) -> I16Dot16 {
        match self {
            Self::Normal => I16Dot16::new(400),
            Self::Bold => I16Dot16::new(700),
            Self::Value(value) => value,
        }
    }
}

// https://www.w3.org/TR/css-fonts-4/#font-size-prop
// TODO: relative variants
#[derive(Debug, Clone, Copy)]
pub enum FontSize {
    Length(Length),
}

impl PeekParse for FontSize {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        Ok(Some(
            if let Some(length) = Length::peek_parse(stream, lk)? {
                Self::Length(length)
            } else {
                return Ok(None);
            },
        ))
    }
}

impl PropertyValue<I26Dot6> for FontSize {
    fn compute(self, &parent: &I26Dot6) -> I26Dot6 {
        match self {
            Self::Length(length) => length
                .compute(&ComputedLength::from_pixels(parent))
                .to_unscaled_pixels(),
        }
    }
}

// https://www.w3.org/TR/css-fonts-4/#font-style-prop
// TODO: Most variants unimplemented.
#[derive(Debug, Clone, Copy)]
pub enum FontSlant {
    Normal,
    Italic,
}

impl PeekParse for FontSlant {
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

impl PropertyValue<ComputedFontSlant> for FontSlant {
    fn compute(self, _parent: &ComputedFontSlant) -> ComputedFontSlant {
        match self {
            FontSlant::Normal => ComputedFontSlant::Regular,
            FontSlant::Italic => ComputedFontSlant::Italic,
        }
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
        Ok(Some(if lk.peek_skip("auto", stream) {
            Self::Auto
        } else if lk.peek_skip("loose", stream) {
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
            LineBreak::Auto => LineBreakStrictness::Normal,
            LineBreak::Loose => LineBreakStrictness::Loose,
            LineBreak::Normal => LineBreakStrictness::Normal,
            LineBreak::Strict => LineBreakStrictness::Strict,
            LineBreak::Anywhere => LineBreakStrictness::Anywhere,
        }
    }
}

// https://www.w3.org/TR/css-text-3/#word-break-property
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
            WordBreak::Normal => LineBreakWordOption::Normal,
            WordBreak::KeepAll => LineBreakWordOption::KeepAll,
            WordBreak::BreakAll => LineBreakWordOption::BreakAll,
        }
    }
}

// https://drafts.csswg.org/css-color-5/#typedef-color
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Color {
    Base(ColorBase),
    CurrentColor,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColorBase {
    Hex(BGRA8),
    ColorFunction(ColorFunction),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColorFunction {
    Rgb(RgbColorFunctionContent),
    Rgba(RgbColorFunctionContent),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RgbColorFunctionContent {
    r: RgbColorComponent,
    g: RgbColorComponent,
    b: RgbColorComponent,
    a: Option<RgbColorComponent>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum RgbColorComponent {
    Number(f32),
    Percentage(f32),
    None,
}

impl PeekParse for ColorBase {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        Ok(if lk.peek(Hash) {
            let hash = stream.parse::<Hash>()?;
            let make_error = || {
                ParseError::new(
                    hash,
                    "hex color code must consist of exactly 3, 4, 6, or 8 ASCII hex digits",
                )
            };

            let value_str = hash.value().to_string();
            if value_str.bytes().any(|c| !c.is_ascii_hexdigit()) {
                return Err(make_error());
            }

            let Ok(mut value) = u32::from_str_radix(&value_str, 16) else {
                return Err(make_error());
            };

            let mut expand = false;
            match value_str.len() {
                8 => (),
                6 => value = (value << 8) | 0xFF,
                4 => expand = true,
                3 => {
                    expand = true;
                    value = (value << 4) | 0xF;
                }
                _ => return Err(make_error()),
            }

            if expand {
                let interleaved = (value & 0xF)
                    | (value & 0xF0) << 4
                    | (value & 0xF00) << 8
                    | (value & 0xF000) << 12;

                value = interleaved | (interleaved << 4)
            }

            Some(ColorBase::Hex(BGRA8::from_rgba32(value)))
        } else if stream.peek(FunctionalNotation) {
            ColorFunction::peek_parse(stream, lk)?.map(ColorBase::ColorFunction)
        } else {
            None
        })
    }
}

impl PeekParse for ColorFunction {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        let is_rgb = lk.peek("rgb(");
        Ok(if is_rgb || lk.peek("rgba(") {
            let fun = stream.parse::<FunctionalNotation>()?;
            let content = parse_cursor(fun.content())?;
            Some(if is_rgb {
                Self::Rgb(content)
            } else {
                Self::Rgba(content)
            })
        } else {
            None
        })
    }
}

impl Parse<'_> for RgbColorFunctionContent {
    fn parse(stream: &ParseStream<'_>) -> Result<Self, ParseError> {
        let r = stream.parse::<RgbColorComponent>()?;
        let g = stream.parse::<RgbColorComponent>()?;
        let b = stream.parse::<RgbColorComponent>()?;
        let a = if stream.peek_skip(Token![/]) {
            Some(stream.parse::<RgbColorComponent>()?)
        } else {
            None
        };

        // Values outside these ranges are not invalid, but are clamped to the ranges defined here at parsed-value time.
        let clamp_component = |c: RgbColorComponent, num_min: f32, num_max: f32| match c {
            RgbColorComponent::Number(n) => RgbColorComponent::Number(n.clamp(num_min, num_max)),
            RgbColorComponent::Percentage(p) => RgbColorComponent::Percentage(p.clamp(0.0, 100.0)),
            RgbColorComponent::None => RgbColorComponent::None,
        };
        Ok(Self {
            r: clamp_component(r, 0.0, 255.0),
            g: clamp_component(g, 0.0, 255.0),
            b: clamp_component(b, 0.0, 255.0),
            a: a.map(|c| clamp_component(c, 0.0, 1.0)),
        })
    }
}

impl Parse<'_> for RgbColorComponent {
    fn parse(stream: &ParseStream<'_>) -> Result<Self, ParseError> {
        let mut lk = stream.lookahead1();
        Ok(if lk.peek(Number) {
            RgbColorComponent::Number(stream.parse::<Number>()?.value().to_f32())
        } else if lk.peek(Percentage) {
            RgbColorComponent::Percentage(stream.parse::<Percentage>()?.value().to_f32())
        } else if lk.peek_skip("none", stream) {
            RgbColorComponent::None
        } else {
            return Err(lk.error());
        })
    }
}

impl PeekParse for Color {
    fn peek_parse<'a>(
        stream: &ParseStream<'a>,
        lk: &mut Lookahead<'a>,
    ) -> Result<Option<Self>, ParseError> {
        Ok(if lk.peek("currentcolor") {
            stream.skip();
            Some(Color::CurrentColor)
        } else {
            ColorBase::peek_parse(stream, lk)?.map(Color::Base)
        })
    }
}

impl PropertyValue<ComputedColor> for Color {
    fn compute(self, _parent: &ComputedColor) -> ComputedColor {
        match self {
            Self::Base(ColorBase::Hex(value)) => ComputedColor::Srgb(value),
            Self::Base(ColorBase::ColorFunction(fun)) => match fun {
                ColorFunction::Rgb(rgb) | ColorFunction::Rgba(rgb) => rgb.compute(),
            },
            // https://www.w3.org/TR/css-color-4/#resolving-other-colors
            // The currentcolor keyword computes to itself.
            Self::CurrentColor => ComputedColor::CurrentColor,
        }
    }
}

impl RgbColorFunctionContent {
    fn compute(self) -> ComputedColor {
        let r = self.r.to_u8();
        let g = self.g.to_u8();
        let b = self.b.to_u8();
        let a = self.a.map_or(0, RgbColorComponent::to_u8);

        ComputedColor::Srgb(BGRA8::new(r, g, b, a))
    }
}

impl RgbColorComponent {
    fn to_u8(self) -> u8 {
        // For all other purposes, a missing component behaves as a zero value.
        match self {
            RgbColorComponent::Number(n) => n as u8,
            RgbColorComponent::Percentage(p) => (p * const { 255.0 / 100.0 }) as u8,
            RgbColorComponent::None => 0,
        }
    }
}

impl PropertyValue<BGRA8> for Color {
    fn compute(self, &parent: &BGRA8) -> BGRA8 {
        match PropertyValue::<ComputedColor>::compute(self, &ComputedColor::Srgb(parent)) {
            ComputedColor::Srgb(value) => value,
            // https://www.w3.org/TR/css-color-4/#resolving-other-colors
            // > In the color property, the used value of currentcolor is the resolved inherited value.
            // It doesn't really make sense for us to store the computed currentcolor
            // value of 'color' since we don't really have a good way to get the inherited
            // value later so we just compute color to its used value here.
            ComputedColor::CurrentColor => parent,
        }
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

    #[test]
    fn hex_colors() {
        assert_eq!(
            csssyn::value::parse_str::<Specified::<ColorBase>>(r#"#ABC"#).unwrap(),
            Specified::Value(ColorBase::Hex(BGRA8::from_rgba32(0xAABBCCFF)))
        );

        assert_eq!(
            csssyn::value::parse_str::<Specified::<ColorBase>>(r#"#ABCD"#).unwrap(),
            Specified::Value(ColorBase::Hex(BGRA8::from_rgba32(0xAABBCCDD)))
        );

        assert_eq!(
            csssyn::value::parse_str::<Specified::<ColorBase>>(r#"#aabbcc"#).unwrap(),
            Specified::Value(ColorBase::Hex(BGRA8::from_rgba32(0xAABBCCFF)))
        );

        assert_eq!(
            csssyn::value::parse_str::<Specified::<ColorBase>>(r#"#aabbccdd"#).unwrap(),
            Specified::Value(ColorBase::Hex(BGRA8::from_rgba32(0xAABBCCDD)))
        );
    }

    #[test]
    #[should_panic]
    fn hex_color_invalid_digit() {
        csssyn::value::parse_str::<Specified<ColorBase>>(r#"#hello"#).unwrap();
    }

    #[test]
    #[should_panic]
    fn hex_color_invalid_length() {
        csssyn::value::parse_str::<Specified<ColorBase>>(r#"#12"#).unwrap();
    }
}
