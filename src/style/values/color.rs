//! Values from the [css-color](https://drafts.csswg.org/css-color-5/#typedef-color) spec.
use rasterize::color::BGRA8;

use super::*;
use crate::{csssyn::token::*, style::computed::Color as ComputedColor};

// https://drafts.csswg.org/css-color-5/#typedef-color
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Color {
    Base(ColorBase),
    CurrentColor,
}

// https://drafts.csswg.org/css-color-5/#typedef-color-base
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColorBase {
    Hex(BGRA8),
    ColorFunction(ColorFunction),
    Named(&'static (&'static str, BGRA8)),
}

// https://drafts.csswg.org/css-color-5/#typedef-color-function
// only rgb{,a} for now
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColorFunction {
    Rgb(RgbColorFunctionContent),
    Rgba(RgbColorFunctionContent),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RgbColorFunctionContent {
    r: RgbComponent,
    g: RgbComponent,
    b: RgbComponent,
    a: Option<RgbComponent>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum RgbComponent {
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
        } else if let Some((ident, _)) = stream.cursor().take::<Ident>() {
            let name = {
                let mut result = ident.value().to_string();
                result.make_ascii_lowercase();
                result
            };

            let Ok(found) = NAMED_COLORS.binary_search_by_key(&name.as_str(), |x| x.0) else {
                lk.extend_attempted(["<named-color>", "transparent"]);
                return Ok(None);
            };

            stream.parse::<Ident>()?;
            Some(ColorBase::Named(&NAMED_COLORS[found]))
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

// https://drafts.csswg.org/css-color-4/#rgb-functions
impl Parse<'_> for RgbColorFunctionContent {
    fn parse(stream: &ParseStream<'_>) -> Result<Self, ParseError> {
        let (r, g, b, a);
        r = stream.parse::<RgbComponent>()?;

        let mut lk = stream.lookahead1();
        if matches!(r, RgbComponent::Number(_) | RgbComponent::Percentage(_))
            && lk.peek_skip(Token![,], stream)
        {
            // legacy syntax
            if matches!(r, RgbComponent::Percentage(_)) {
                g = RgbComponent::from(stream.parse::<Percentage>()?);
                stream.parse::<Token![,]>()?;
                b = RgbComponent::from(stream.parse::<Percentage>()?);
            } else {
                g = RgbComponent::from(stream.parse::<Number>()?);
                stream.parse::<Token![,]>()?;
                b = RgbComponent::from(stream.parse::<Number>()?);
            }

            a = if stream.peek_skip(Token![,]) {
                lk = stream.lookahead1();
                Some(if lk.peek(Percentage) {
                    RgbComponent::from(stream.parse::<Percentage>()?)
                } else if lk.peek(Number) {
                    RgbComponent::from(stream.parse::<Number>()?)
                } else {
                    return Err(lk.error());
                })
            } else {
                None
            };
        } else {
            // modern syntax
            g = RgbComponent::lk_parse(stream, lk)?;
            b = stream.parse::<RgbComponent>()?;
            a = if stream.peek_skip(Token![/]) {
                Some(stream.parse::<RgbComponent>()?)
            } else {
                None
            };
        };

        // Values outside these ranges are not invalid, but are clamped to the ranges defined here at parsed-value time.
        let clamp_component = |c: RgbComponent, num_min: f32, num_max: f32| match c {
            RgbComponent::Number(n) => RgbComponent::Number(n.clamp(num_min, num_max)),
            RgbComponent::Percentage(p) => RgbComponent::Percentage(p.clamp(0.0, 100.0)),
            RgbComponent::None => RgbComponent::None,
        };
        Ok(Self {
            r: clamp_component(r, 0.0, 255.0),
            g: clamp_component(g, 0.0, 255.0),
            b: clamp_component(b, 0.0, 255.0),
            a: a.map(|c| clamp_component(c, 0.0, 1.0)),
        })
    }
}

impl From<Number<'_>> for RgbComponent {
    fn from(number: Number) -> Self {
        Self::Number(number.value().to_f32())
    }
}

impl From<Percentage<'_>> for RgbComponent {
    fn from(percentage: Percentage) -> Self {
        Self::Percentage(percentage.value().to_f32())
    }
}

impl RgbComponent {
    fn lk_parse(stream: &ParseStream, mut lk: Lookahead) -> Result<Self, ParseError> {
        Ok(if lk.peek(Number) {
            stream.parse::<Number>().map(Self::from)?
        } else if lk.peek(Percentage) {
            stream.parse::<Percentage>().map(Self::from)?
        } else if lk.peek_skip("none", stream) {
            RgbComponent::None
        } else {
            return Err(lk.error());
        })
    }
}

impl Parse<'_> for RgbComponent {
    fn parse(stream: &ParseStream<'_>) -> Result<Self, ParseError> {
        Self::lk_parse(stream, stream.lookahead1())
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
        self.compute()
    }
}

impl Color {
    pub(super) fn compute(self) -> ComputedColor {
        match self {
            Self::Base(ColorBase::Hex(value)) | Self::Base(ColorBase::Named(&(_, value))) => {
                ComputedColor::Srgb(value)
            }
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
        let r = self.r.color_to_u8();
        let g = self.g.color_to_u8();
        let b = self.b.color_to_u8();
        let a = self.a.map_or(255, RgbComponent::alpha_to_u8);

        ComputedColor::Srgb(BGRA8::new(r, g, b, a))
    }
}

impl RgbComponent {
    fn color_to_u8(self) -> u8 {
        // For all other purposes, a missing component behaves as a zero value.
        match self {
            // Implementations should honor the precision of the component as authored or calculated wherever possible.
            // If this is not possible, the component should be rounded towards +∞.
            RgbComponent::Number(n) => n.ceil() as u8,
            RgbComponent::Percentage(p) => (p * const { 255.0 / 100.0 }).ceil() as u8,
            RgbComponent::None => 0,
        }
    }

    fn alpha_to_u8(self) -> u8 {
        match self {
            RgbComponent::Number(n) => (n * 255.0).ceil() as u8,
            RgbComponent::Percentage(p) => (p * const { 255.0 / 100.0 }).ceil() as u8,
            RgbComponent::None => 0,
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

// From table in https://www.w3.org/TR/css-color-4/#typedef-named-color
// + transparent
const NAMED_COLORS: &[(&str, BGRA8)] = &[
    ("aliceblue", BGRA8::from_rgba32(0xF0F8FF)),
    ("antiquewhite", BGRA8::from_rgba32(0xFAEBD7)),
    ("aqua", BGRA8::from_rgba32(0x00FFFF)),
    ("aquamarine", BGRA8::from_rgba32(0x7FFFD4)),
    ("azure", BGRA8::from_rgba32(0xF0FFFF)),
    ("beige", BGRA8::from_rgba32(0xF5F5DC)),
    ("bisque", BGRA8::from_rgba32(0xFFE4C4)),
    ("black", BGRA8::from_rgba32(0x000000)),
    ("blanchedalmond", BGRA8::from_rgba32(0xFFEBCD)),
    ("blue", BGRA8::from_rgba32(0x0000FF)),
    ("blueviolet", BGRA8::from_rgba32(0x8A2BE2)),
    ("brown", BGRA8::from_rgba32(0xA52A2A)),
    ("burlywood", BGRA8::from_rgba32(0xDEB887)),
    ("cadetblue", BGRA8::from_rgba32(0x5F9EA0)),
    ("chartreuse", BGRA8::from_rgba32(0x7FFF00)),
    ("chocolate", BGRA8::from_rgba32(0xD2691E)),
    ("coral", BGRA8::from_rgba32(0xFF7F50)),
    ("cornflowerblue", BGRA8::from_rgba32(0x6495ED)),
    ("cornsilk", BGRA8::from_rgba32(0xFFF8DC)),
    ("crimson", BGRA8::from_rgba32(0xDC143C)),
    ("cyan", BGRA8::from_rgba32(0x00FFFF)),
    ("darkblue", BGRA8::from_rgba32(0x00008B)),
    ("darkcyan", BGRA8::from_rgba32(0x008B8B)),
    ("darkgoldenrod", BGRA8::from_rgba32(0xB8860B)),
    ("darkgray", BGRA8::from_rgba32(0xA9A9A9)),
    ("darkgreen", BGRA8::from_rgba32(0x006400)),
    ("darkgrey", BGRA8::from_rgba32(0xA9A9A9)),
    ("darkkhaki", BGRA8::from_rgba32(0xBDB76B)),
    ("darkmagenta", BGRA8::from_rgba32(0x8B008B)),
    ("darkolivegreen", BGRA8::from_rgba32(0x556B2F)),
    ("darkorange", BGRA8::from_rgba32(0xFF8C00)),
    ("darkorchid", BGRA8::from_rgba32(0x9932CC)),
    ("darkred", BGRA8::from_rgba32(0x8B0000)),
    ("darksalmon", BGRA8::from_rgba32(0xE9967A)),
    ("darkseagreen", BGRA8::from_rgba32(0x8FBC8F)),
    ("darkslateblue", BGRA8::from_rgba32(0x483D8B)),
    ("darkslategray", BGRA8::from_rgba32(0x2F4F4F)),
    ("darkslategrey", BGRA8::from_rgba32(0x2F4F4F)),
    ("darkturquoise", BGRA8::from_rgba32(0x00CED1)),
    ("darkviolet", BGRA8::from_rgba32(0x9400D3)),
    ("deeppink", BGRA8::from_rgba32(0xFF1493)),
    ("deepskyblue", BGRA8::from_rgba32(0x00BFFF)),
    ("dimgray", BGRA8::from_rgba32(0x696969)),
    ("dimgrey", BGRA8::from_rgba32(0x696969)),
    ("dodgerblue", BGRA8::from_rgba32(0x1E90FF)),
    ("firebrick", BGRA8::from_rgba32(0xB22222)),
    ("floralwhite", BGRA8::from_rgba32(0xFFFAF0)),
    ("forestgreen", BGRA8::from_rgba32(0x228B22)),
    ("fuchsia", BGRA8::from_rgba32(0xFF00FF)),
    ("gainsboro", BGRA8::from_rgba32(0xDCDCDC)),
    ("ghostwhite", BGRA8::from_rgba32(0xF8F8FF)),
    ("gold", BGRA8::from_rgba32(0xFFD700)),
    ("goldenrod", BGRA8::from_rgba32(0xDAA520)),
    ("gray", BGRA8::from_rgba32(0x808080)),
    ("green", BGRA8::from_rgba32(0x008000)),
    ("greenyellow", BGRA8::from_rgba32(0xADFF2F)),
    ("grey", BGRA8::from_rgba32(0x808080)),
    ("honeydew", BGRA8::from_rgba32(0xF0FFF0)),
    ("hotpink", BGRA8::from_rgba32(0xFF69B4)),
    ("indianred", BGRA8::from_rgba32(0xCD5C5C)),
    ("indigo", BGRA8::from_rgba32(0x4B0082)),
    ("ivory", BGRA8::from_rgba32(0xFFFFF0)),
    ("khaki", BGRA8::from_rgba32(0xF0E68C)),
    ("lavender", BGRA8::from_rgba32(0xE6E6FA)),
    ("lavenderblush", BGRA8::from_rgba32(0xFFF0F5)),
    ("lawngreen", BGRA8::from_rgba32(0x7CFC00)),
    ("lemonchiffon", BGRA8::from_rgba32(0xFFFACD)),
    ("lightblue", BGRA8::from_rgba32(0xADD8E6)),
    ("lightcoral", BGRA8::from_rgba32(0xF08080)),
    ("lightcyan", BGRA8::from_rgba32(0xE0FFFF)),
    ("lightgoldenrodyellow", BGRA8::from_rgba32(0xFAFAD2)),
    ("lightgray", BGRA8::from_rgba32(0xD3D3D3)),
    ("lightgreen", BGRA8::from_rgba32(0x90EE90)),
    ("lightgrey", BGRA8::from_rgba32(0xD3D3D3)),
    ("lightpink", BGRA8::from_rgba32(0xFFB6C1)),
    ("lightsalmon", BGRA8::from_rgba32(0xFFA07A)),
    ("lightseagreen", BGRA8::from_rgba32(0x20B2AA)),
    ("lightskyblue", BGRA8::from_rgba32(0x87CEFA)),
    ("lightslategray", BGRA8::from_rgba32(0x778899)),
    ("lightslategrey", BGRA8::from_rgba32(0x778899)),
    ("lightsteelblue", BGRA8::from_rgba32(0xB0C4DE)),
    ("lightyellow", BGRA8::from_rgba32(0xFFFFE0)),
    ("lime", BGRA8::from_rgba32(0x00FF00)),
    ("limegreen", BGRA8::from_rgba32(0x32CD32)),
    ("linen", BGRA8::from_rgba32(0xFAF0E6)),
    ("magenta", BGRA8::from_rgba32(0xFF00FF)),
    ("maroon", BGRA8::from_rgba32(0x800000)),
    ("mediumaquamarine", BGRA8::from_rgba32(0x66CDAA)),
    ("mediumblue", BGRA8::from_rgba32(0x0000CD)),
    ("mediumorchid", BGRA8::from_rgba32(0xBA55D3)),
    ("mediumpurple", BGRA8::from_rgba32(0x9370DB)),
    ("mediumseagreen", BGRA8::from_rgba32(0x3CB371)),
    ("mediumslateblue", BGRA8::from_rgba32(0x7B68EE)),
    ("mediumspringgreen", BGRA8::from_rgba32(0x00FA9A)),
    ("mediumturquoise", BGRA8::from_rgba32(0x48D1CC)),
    ("mediumvioletred", BGRA8::from_rgba32(0xC71585)),
    ("midnightblue", BGRA8::from_rgba32(0x191970)),
    ("mintcream", BGRA8::from_rgba32(0xF5FFFA)),
    ("mistyrose", BGRA8::from_rgba32(0xFFE4E1)),
    ("moccasin", BGRA8::from_rgba32(0xFFE4B5)),
    ("navajowhite", BGRA8::from_rgba32(0xFFDEAD)),
    ("navy", BGRA8::from_rgba32(0x000080)),
    ("oldlace", BGRA8::from_rgba32(0xFDF5E6)),
    ("olive", BGRA8::from_rgba32(0x808000)),
    ("olivedrab", BGRA8::from_rgba32(0x6B8E23)),
    ("orange", BGRA8::from_rgba32(0xFFA500)),
    ("orangered", BGRA8::from_rgba32(0xFF4500)),
    ("orchid", BGRA8::from_rgba32(0xDA70D6)),
    ("palegoldenrod", BGRA8::from_rgba32(0xEEE8AA)),
    ("palegreen", BGRA8::from_rgba32(0x98FB98)),
    ("paleturquoise", BGRA8::from_rgba32(0xAFEEEE)),
    ("palevioletred", BGRA8::from_rgba32(0xDB7093)),
    ("papayawhip", BGRA8::from_rgba32(0xFFEFD5)),
    ("peachpuff", BGRA8::from_rgba32(0xFFDAB9)),
    ("peru", BGRA8::from_rgba32(0xCD853F)),
    ("pink", BGRA8::from_rgba32(0xFFC0CB)),
    ("plum", BGRA8::from_rgba32(0xDDA0DD)),
    ("powderblue", BGRA8::from_rgba32(0xB0E0E6)),
    ("purple", BGRA8::from_rgba32(0x800080)),
    ("rebeccapurple", BGRA8::from_rgba32(0x663399)),
    ("red", BGRA8::from_rgba32(0xFF0000)),
    ("rosybrown", BGRA8::from_rgba32(0xBC8F8F)),
    ("royalblue", BGRA8::from_rgba32(0x4169E1)),
    ("saddlebrown", BGRA8::from_rgba32(0x8B4513)),
    ("salmon", BGRA8::from_rgba32(0xFA8072)),
    ("sandybrown", BGRA8::from_rgba32(0xF4A460)),
    ("seagreen", BGRA8::from_rgba32(0x2E8B57)),
    ("seashell", BGRA8::from_rgba32(0xFFF5EE)),
    ("sienna", BGRA8::from_rgba32(0xA0522D)),
    ("silver", BGRA8::from_rgba32(0xC0C0C0)),
    ("skyblue", BGRA8::from_rgba32(0x87CEEB)),
    ("slateblue", BGRA8::from_rgba32(0x6A5ACD)),
    ("slategray", BGRA8::from_rgba32(0x708090)),
    ("slategrey", BGRA8::from_rgba32(0x708090)),
    ("snow", BGRA8::from_rgba32(0xFFFAFA)),
    ("springgreen", BGRA8::from_rgba32(0x00FF7F)),
    ("steelblue", BGRA8::from_rgba32(0x4682B4)),
    ("tan", BGRA8::from_rgba32(0xD2B48C)),
    ("teal", BGRA8::from_rgba32(0x008080)),
    ("thistle", BGRA8::from_rgba32(0xD8BFD8)),
    ("tomato", BGRA8::from_rgba32(0xFF6347)),
    ("turquoise", BGRA8::from_rgba32(0x40E0D0)),
    ("violet", BGRA8::from_rgba32(0xEE82EE)),
    ("wheat", BGRA8::from_rgba32(0xF5DEB3)),
    ("white", BGRA8::from_rgba32(0xFFFFFF)),
    ("whitesmoke", BGRA8::from_rgba32(0xF5F5F5)),
    ("yellow", BGRA8::from_rgba32(0xFFFF00)),
    ("yellowgreen", BGRA8::from_rgba32(0x9ACD32)),
    ("transparent", BGRA8::ZERO),
];

#[cfg(test)]
mod test {
    use super::*;
    use crate::style::properties;

    fn compute_as_background_color(source: &str) -> Result<ComputedColor, ParseError> {
        test_parse_and_compute_str::<properties::ComputedBackgroundColor, Color>(source)
    }

    #[test]
    fn hex() {
        assert_eq!(
            compute_as_background_color(r#"#ABC"#).unwrap(),
            ComputedColor::Srgb(BGRA8::from_rgba32(0xAABBCCFF))
        );

        assert_eq!(
            compute_as_background_color(r#"#ABCD"#).unwrap(),
            ComputedColor::Srgb(BGRA8::from_rgba32(0xAABBCCDD))
        );

        assert_eq!(
            compute_as_background_color(r#"#aabbcc"#).unwrap(),
            ComputedColor::Srgb(BGRA8::from_rgba32(0xAABBCCFF))
        );

        assert_eq!(
            compute_as_background_color(r#"#abcddcba"#).unwrap(),
            ComputedColor::Srgb(BGRA8::from_rgba32(0xABCDDCBA))
        );
    }

    // https://github.com/web-platform-tests/wpt/blob/c1350e3eade197000e49d3a7722a3765ee3d6818/css/css-color/parsing/color-invalid-hex-color.html
    #[test]
    fn wpt_invalid_hex() {
        let cases = [
            ["#", "Should not parse invalid hex"],
            ["#f", "Should not parse invalid hex"],
            ["#ff", "Should not parse invalid hex"],
            ["#ffg", "Should not parse invalid hex"],
            ["#fffg", "Should not parse invalid hex"],
            ["#fffff", "Should not parse invalid hex"],
            ["#fffffg", "Should not parse invalid hex"],
            ["#fffffff", "Should not parse invalid hex"],
            ["#fffffffg", "Should not parse invalid hex"],
            ["#fffffffff", "Should not parse invalid hex"],
        ];

        for [s, _] in cases {
            assert!(compute_as_background_color(s).is_err());
        }
    }

    #[test]
    fn currentcolor() {
        assert_eq!(
            compute_as_background_color(r#"CurrentColor"#).unwrap(),
            ComputedColor::CurrentColor
        );
    }

    #[test]
    fn modern_rgb_functions() {
        assert_eq!(
            compute_as_background_color("rgb(0% 50% 0% / 75%)").unwrap(),
            ComputedColor::Srgb(BGRA8::from_rgba32(0x008000C0))
        );
        assert_eq!(
            compute_as_background_color("rgba(200 20 100 / 0.25)").unwrap(),
            ComputedColor::Srgb(BGRA8::from_rgba32(0xC8146440))
        );
        assert!(compute_as_background_color("rgba(200 20 100").is_err());
    }

    // https://github.com/web-platform-tests/wpt/blob/75b089d70448e1da425ab8cf38971b6af41f3cea/css/css-color/parsing/color-valid-rgb.html
    // `calc()`-containing tests omitted since we don't support that
    #[test]
    fn wpt_valid_rgb() {
        let cases = [
            ["rgb(none none none)", "rgb(0, 0, 0)"],
            ["rgb(none none none / none)", "rgba(0, 0, 0, 0)"],
            ["rgb(128 none none)", "rgb(128, 0, 0)"],
            ["rgb(128 none none / none)", "rgba(128, 0, 0, 0)"],
            ["rgb(none none none / .5)", "rgba(0, 0, 0, 0.5)"],
            ["rgb(20% none none)", "rgb(51, 0, 0)"],
            ["rgb(20% none none / none)", "rgba(51, 0, 0, 0)"],
            ["rgb(none none none / 50%)", "rgba(0, 0, 0, 0.5)"],
            ["rgba(none none none)", "rgb(0, 0, 0)"],
            ["rgba(none none none / none)", "rgba(0, 0, 0, 0)"],
            ["rgba(128 none none)", "rgb(128, 0, 0)"],
            ["rgba(128 none none / none)", "rgba(128, 0, 0, 0)"],
            ["rgba(none none none / .5)", "rgba(0, 0, 0, 0.5)"],
            ["rgba(20% none none)", "rgb(51, 0, 0)"],
            ["rgba(20% none none / none)", "rgba(51, 0, 0, 0)"],
            ["rgba(none none none / 50%)", "rgba(0, 0, 0, 0.5)"],
            ["rgb(-2 3 4)", "rgb(0, 3, 4)"],
            ["rgb(-20% 20% 40%)", "rgb(0, 51, 102)"],
            ["rgb(257 30 40)", "rgb(255, 30, 40)"],
            ["rgb(250% 20% 40%)", "rgb(255, 51, 102)"],
            ["rgba(-2 3 4)", "rgb(0, 3, 4)"],
            ["rgba(-20% 20% 40%)", "rgb(0, 51, 102)"],
            ["rgba(257 30 40)", "rgb(255, 30, 40)"],
            ["rgba(250% 20% 40%)", "rgb(255, 51, 102)"],
            ["rgba(-2 3 4 / .5)", "rgba(0, 3, 4, 0.5)"],
            ["rgba(-20% 20% 40% / 50%)", "rgba(0, 51, 102, 0.5)"],
            ["rgba(257 30 40 / 50%)", "rgba(255, 30, 40, 0.5)"],
            ["rgba(250% 20% 40% / .5)", "rgba(255, 51, 102, 0.5)"],
            // Test with mixed components.
            ["rgb(250% 51 40%)", "rgb(255, 51, 102)"],
            ["rgb(255 20% 102)", "rgb(255, 51, 102)"],
            // rgb are in the range [0, 255], alpha is in the range [0, 1].
            // Values above or below these numbers should get resolved to the upper/lower bound.
            ["rgb(500, 0, 0)", "rgb(255, 0, 0)"],
            ["rgb(-500, 64, 128)", "rgb(0, 64, 128)"],
        ];

        for [a, b] in cases {
            assert_compute_ok_and_eq(a, b, compute_as_background_color);
        }
    }

    // https://github.com/web-platform-tests/wpt/blob/c1350e3eade197000e49d3a7722a3765ee3d6818/css/css-color/parsing/color-invalid-rgb.html
    #[test]
    fn wpt_invalid_rgb() {
        #[rustfmt::skip]
        let cases = [
            ["rgb(none, none, none)", "The none keyword is invalid in legacy color syntax"],
            ["rgba(none, none, none, none)", "The none keyword is invalid in legacy color syntax"],
            ["rgb(128, 0, none)", "The none keyword is invalid in legacy color syntax"],
            ["rgb(255, 255, 255, none)", "The none keyword is invalid in legacy color syntax"],

            ["rgb(10%, 50%, 0)", "Values must be all numbers or all percentages"],
            ["rgb(255, 50%, 0%)", "Values must be all numbers or all percentages"],
            ["rgb(0, 0 0)", "Comma optional syntax requires no commas at all"],
            ["rgb(0 0, 0)", "Comma optional syntax requires no commas at all"],
            ["rgb(,0, 0, 0)", "Leading commas are invalid"],
            ["rgb(0, 0, 0,)", "Trailing commas are invalid"],
            ["rgb(0, 0,, 0)", "Double commas are invalid"],
            ["rgb(0, 0, 0deg)", "Angles are not accepted in the rgb function"],
            ["rgb(0, 0, light)", "Keywords are not accepted in the rgb function"],
            ["rgb()", "The rgb function requires 3 or 4 arguments"],
            ["rgb(0)", "The rgb function requires 3 or 4 arguments"],
            ["rgb(0, 0)", "The rgb function requires 3 or 4 arguments"],
            ["rgb(0%)", "The rgb function requires 3 or 4 arguments"],
            ["rgb(0%, 0%)", "The rgb function requires 3 or 4 arguments"],
            ["rgba(10%, 50%, 0, 1)", "Values must be all numbers or all percentages"],
            ["rgba(255, 50%, 0%, 1)", "Values must be all numbers or all percentages"],
            ["rgba(0, 0, 0 0)", "Comma optional syntax requires no commas at all"],
            ["rgba(0, 0, 0, 0deg)", "Angles are not accepted in the rgb function"],
            ["rgba(0, 0, 0, light)", "Keywords are not accepted in the rgb function"],
            ["rgba()", "The rgba function requires 3 or 4 arguments"],
            ["rgba(0)", "The rgba function requires 3 or 4 arguments"],
            ["rgba(0, 0, 0, 0, 0)", "The rgba function requires 3 or 4 arguments"],
            ["rgba(0%)", "The rgba function requires 3 or 4 arguments"],
            ["rgba(0%, 0%)", "The rgba function requires 3 or 4 arguments"],
            ["rgba(0%, 0%, 0%, 0%, 0%)", "The rgba function requires 3 or 4 arguments"],
            ["rgb(257, 0, 5 / 0)", "Cannot mix legacy and non-legacy formats"],
        ];

        for [s, _] in cases {
            assert!(compute_as_background_color(s).is_err());
        }
    }
}
