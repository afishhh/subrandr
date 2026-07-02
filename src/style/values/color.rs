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

impl Parse<'_> for Option<ColorBase> {
    fn parse(stream: &mut ParseStream<'_>) -> Result<Self, ParseError> {
        Ok(if stream.peek(Hash) {
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
                stream.extend_attempted(["<named-color>"]);
                return Ok(None);
            };

            stream.skip();
            Some(ColorBase::Named(&NAMED_COLORS[found]))
        } else {
            stream
                .parse::<Option<ColorFunction>>()?
                .map(ColorBase::ColorFunction)
        })
    }
}

impl Parse<'_> for Option<ColorFunction> {
    fn parse<'a>(stream: &mut ParseStream<'a>) -> Result<Self, ParseError> {
        let is_rgb = stream.peek("rgb(");
        Ok(if is_rgb || stream.peek("rgba(") {
            let fun = stream.parse::<FunctionalNotation>()?;
            let content = parse_cursor(fun.content())?;
            Some(if is_rgb {
                ColorFunction::Rgb(content)
            } else {
                ColorFunction::Rgba(content)
            })
        } else {
            None
        })
    }
}

// https://drafts.csswg.org/css-color-4/#rgb-functions
impl Parse<'_> for RgbColorFunctionContent {
    fn parse(stream: &mut ParseStream<'_>) -> Result<Self, ParseError> {
        let (r, g, b, a);
        r = stream.parse::<RgbComponent>()?;

        if matches!(r, RgbComponent::Number(_) | RgbComponent::Percentage(_))
            && stream.peek_skip(Token![,])
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
                Some(if stream.peek(Percentage) {
                    RgbComponent::from(stream.parse::<Percentage>()?)
                } else if stream.peek(Number) {
                    RgbComponent::from(stream.parse::<Number>()?)
                } else {
                    return Err(stream.lookahead_error());
                })
            } else {
                None
            };
        } else {
            // modern syntax
            g = stream.parse::<RgbComponent>()?;
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

impl Parse<'_> for RgbComponent {
    fn parse(stream: &mut ParseStream) -> Result<Self, ParseError> {
        Ok(if stream.peek(Number) {
            stream.parse::<Number>().map(Self::from)?
        } else if stream.peek(Percentage) {
            stream.parse::<Percentage>().map(Self::from)?
        } else if stream.peek_skip("none") {
            RgbComponent::None
        } else {
            return Err(stream.lookahead_error());
        })
    }
}

impl Parse<'_> for Option<Color> {
    fn parse(stream: &mut ParseStream) -> Result<Self, ParseError> {
        Ok(if stream.peek("currentcolor") {
            stream.skip();
            Some(Color::CurrentColor)
        } else {
            stream.parse::<Option<ColorBase>>()?.map(Color::Base)
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
    ("aliceblue", BGRA8::from_rgba32(0xF0F8FFFF)),
    ("antiquewhite", BGRA8::from_rgba32(0xFAEBD7FF)),
    ("aqua", BGRA8::from_rgba32(0x00FFFFFF)),
    ("aquamarine", BGRA8::from_rgba32(0x7FFFD4FF)),
    ("azure", BGRA8::from_rgba32(0xF0FFFFFF)),
    ("beige", BGRA8::from_rgba32(0xF5F5DCFF)),
    ("bisque", BGRA8::from_rgba32(0xFFE4C4FF)),
    ("black", BGRA8::from_rgba32(0x000000FF)),
    ("blanchedalmond", BGRA8::from_rgba32(0xFFEBCDFF)),
    ("blue", BGRA8::from_rgba32(0x0000FFFF)),
    ("blueviolet", BGRA8::from_rgba32(0x8A2BE2FF)),
    ("brown", BGRA8::from_rgba32(0xA52A2AFF)),
    ("burlywood", BGRA8::from_rgba32(0xDEB887FF)),
    ("cadetblue", BGRA8::from_rgba32(0x5F9EA0FF)),
    ("chartreuse", BGRA8::from_rgba32(0x7FFF00FF)),
    ("chocolate", BGRA8::from_rgba32(0xD2691EFF)),
    ("coral", BGRA8::from_rgba32(0xFF7F50FF)),
    ("cornflowerblue", BGRA8::from_rgba32(0x6495EDFF)),
    ("cornsilk", BGRA8::from_rgba32(0xFFF8DCFF)),
    ("crimson", BGRA8::from_rgba32(0xDC143CFF)),
    ("cyan", BGRA8::from_rgba32(0x00FFFFFF)),
    ("darkblue", BGRA8::from_rgba32(0x00008BFF)),
    ("darkcyan", BGRA8::from_rgba32(0x008B8BFF)),
    ("darkgoldenrod", BGRA8::from_rgba32(0xB8860BFF)),
    ("darkgray", BGRA8::from_rgba32(0xA9A9A9FF)),
    ("darkgreen", BGRA8::from_rgba32(0x006400FF)),
    ("darkgrey", BGRA8::from_rgba32(0xA9A9A9FF)),
    ("darkkhaki", BGRA8::from_rgba32(0xBDB76BFF)),
    ("darkmagenta", BGRA8::from_rgba32(0x8B008BFF)),
    ("darkolivegreen", BGRA8::from_rgba32(0x556B2FFF)),
    ("darkorange", BGRA8::from_rgba32(0xFF8C00FF)),
    ("darkorchid", BGRA8::from_rgba32(0x9932CCFF)),
    ("darkred", BGRA8::from_rgba32(0x8B0000FF)),
    ("darksalmon", BGRA8::from_rgba32(0xE9967AFF)),
    ("darkseagreen", BGRA8::from_rgba32(0x8FBC8FFF)),
    ("darkslateblue", BGRA8::from_rgba32(0x483D8BFF)),
    ("darkslategray", BGRA8::from_rgba32(0x2F4F4FFF)),
    ("darkslategrey", BGRA8::from_rgba32(0x2F4F4FFF)),
    ("darkturquoise", BGRA8::from_rgba32(0x00CED1FF)),
    ("darkviolet", BGRA8::from_rgba32(0x9400D3FF)),
    ("deeppink", BGRA8::from_rgba32(0xFF1493FF)),
    ("deepskyblue", BGRA8::from_rgba32(0x00BFFFFF)),
    ("dimgray", BGRA8::from_rgba32(0x696969FF)),
    ("dimgrey", BGRA8::from_rgba32(0x696969FF)),
    ("dodgerblue", BGRA8::from_rgba32(0x1E90FFFF)),
    ("firebrick", BGRA8::from_rgba32(0xB22222FF)),
    ("floralwhite", BGRA8::from_rgba32(0xFFFAF0FF)),
    ("forestgreen", BGRA8::from_rgba32(0x228B22FF)),
    ("fuchsia", BGRA8::from_rgba32(0xFF00FFFF)),
    ("gainsboro", BGRA8::from_rgba32(0xDCDCDCFF)),
    ("ghostwhite", BGRA8::from_rgba32(0xF8F8FFFF)),
    ("gold", BGRA8::from_rgba32(0xFFD700FF)),
    ("goldenrod", BGRA8::from_rgba32(0xDAA520FF)),
    ("gray", BGRA8::from_rgba32(0x808080FF)),
    ("green", BGRA8::from_rgba32(0x008000FF)),
    ("greenyellow", BGRA8::from_rgba32(0xADFF2FFF)),
    ("grey", BGRA8::from_rgba32(0x808080FF)),
    ("honeydew", BGRA8::from_rgba32(0xF0FFF0FF)),
    ("hotpink", BGRA8::from_rgba32(0xFF69B4FF)),
    ("indianred", BGRA8::from_rgba32(0xCD5C5CFF)),
    ("indigo", BGRA8::from_rgba32(0x4B0082FF)),
    ("ivory", BGRA8::from_rgba32(0xFFFFF0FF)),
    ("khaki", BGRA8::from_rgba32(0xF0E68CFF)),
    ("lavender", BGRA8::from_rgba32(0xE6E6FAFF)),
    ("lavenderblush", BGRA8::from_rgba32(0xFFF0F5FF)),
    ("lawngreen", BGRA8::from_rgba32(0x7CFC00FF)),
    ("lemonchiffon", BGRA8::from_rgba32(0xFFFACDFF)),
    ("lightblue", BGRA8::from_rgba32(0xADD8E6FF)),
    ("lightcoral", BGRA8::from_rgba32(0xF08080FF)),
    ("lightcyan", BGRA8::from_rgba32(0xE0FFFFFF)),
    ("lightgoldenrodyellow", BGRA8::from_rgba32(0xFAFAD2FF)),
    ("lightgray", BGRA8::from_rgba32(0xD3D3D3FF)),
    ("lightgreen", BGRA8::from_rgba32(0x90EE90FF)),
    ("lightgrey", BGRA8::from_rgba32(0xD3D3D3FF)),
    ("lightpink", BGRA8::from_rgba32(0xFFB6C1FF)),
    ("lightsalmon", BGRA8::from_rgba32(0xFFA07AFF)),
    ("lightseagreen", BGRA8::from_rgba32(0x20B2AAFF)),
    ("lightskyblue", BGRA8::from_rgba32(0x87CEFAFF)),
    ("lightslategray", BGRA8::from_rgba32(0x778899FF)),
    ("lightslategrey", BGRA8::from_rgba32(0x778899FF)),
    ("lightsteelblue", BGRA8::from_rgba32(0xB0C4DEFF)),
    ("lightyellow", BGRA8::from_rgba32(0xFFFFE0FF)),
    ("lime", BGRA8::from_rgba32(0x00FF00FF)),
    ("limegreen", BGRA8::from_rgba32(0x32CD32FF)),
    ("linen", BGRA8::from_rgba32(0xFAF0E6FF)),
    ("magenta", BGRA8::from_rgba32(0xFF00FFFF)),
    ("maroon", BGRA8::from_rgba32(0x800000FF)),
    ("mediumaquamarine", BGRA8::from_rgba32(0x66CDAAFF)),
    ("mediumblue", BGRA8::from_rgba32(0x0000CDFF)),
    ("mediumorchid", BGRA8::from_rgba32(0xBA55D3FF)),
    ("mediumpurple", BGRA8::from_rgba32(0x9370DBFF)),
    ("mediumseagreen", BGRA8::from_rgba32(0x3CB371FF)),
    ("mediumslateblue", BGRA8::from_rgba32(0x7B68EEFF)),
    ("mediumspringgreen", BGRA8::from_rgba32(0x00FA9AFF)),
    ("mediumturquoise", BGRA8::from_rgba32(0x48D1CCFF)),
    ("mediumvioletred", BGRA8::from_rgba32(0xC71585FF)),
    ("midnightblue", BGRA8::from_rgba32(0x191970FF)),
    ("mintcream", BGRA8::from_rgba32(0xF5FFFAFF)),
    ("mistyrose", BGRA8::from_rgba32(0xFFE4E1FF)),
    ("moccasin", BGRA8::from_rgba32(0xFFE4B5FF)),
    ("navajowhite", BGRA8::from_rgba32(0xFFDEADFF)),
    ("navy", BGRA8::from_rgba32(0x000080FF)),
    ("oldlace", BGRA8::from_rgba32(0xFDF5E6FF)),
    ("olive", BGRA8::from_rgba32(0x808000FF)),
    ("olivedrab", BGRA8::from_rgba32(0x6B8E23FF)),
    ("orange", BGRA8::from_rgba32(0xFFA500FF)),
    ("orangered", BGRA8::from_rgba32(0xFF4500FF)),
    ("orchid", BGRA8::from_rgba32(0xDA70D6FF)),
    ("palegoldenrod", BGRA8::from_rgba32(0xEEE8AAFF)),
    ("palegreen", BGRA8::from_rgba32(0x98FB98FF)),
    ("paleturquoise", BGRA8::from_rgba32(0xAFEEEEFF)),
    ("palevioletred", BGRA8::from_rgba32(0xDB7093FF)),
    ("papayawhip", BGRA8::from_rgba32(0xFFEFD5FF)),
    ("peachpuff", BGRA8::from_rgba32(0xFFDAB9FF)),
    ("peru", BGRA8::from_rgba32(0xCD853FFF)),
    ("pink", BGRA8::from_rgba32(0xFFC0CBFF)),
    ("plum", BGRA8::from_rgba32(0xDDA0DDFF)),
    ("powderblue", BGRA8::from_rgba32(0xB0E0E6FF)),
    ("purple", BGRA8::from_rgba32(0x800080FF)),
    ("rebeccapurple", BGRA8::from_rgba32(0x663399FF)),
    ("red", BGRA8::from_rgba32(0xFF0000FF)),
    ("rosybrown", BGRA8::from_rgba32(0xBC8F8FFF)),
    ("royalblue", BGRA8::from_rgba32(0x4169E1FF)),
    ("saddlebrown", BGRA8::from_rgba32(0x8B4513FF)),
    ("salmon", BGRA8::from_rgba32(0xFA8072FF)),
    ("sandybrown", BGRA8::from_rgba32(0xF4A460FF)),
    ("seagreen", BGRA8::from_rgba32(0x2E8B57FF)),
    ("seashell", BGRA8::from_rgba32(0xFFF5EEFF)),
    ("sienna", BGRA8::from_rgba32(0xA0522DFF)),
    ("silver", BGRA8::from_rgba32(0xC0C0C0FF)),
    ("skyblue", BGRA8::from_rgba32(0x87CEEBFF)),
    ("slateblue", BGRA8::from_rgba32(0x6A5ACDFF)),
    ("slategray", BGRA8::from_rgba32(0x708090FF)),
    ("slategrey", BGRA8::from_rgba32(0x708090FF)),
    ("snow", BGRA8::from_rgba32(0xFFFAFAFF)),
    ("springgreen", BGRA8::from_rgba32(0x00FF7FFF)),
    ("steelblue", BGRA8::from_rgba32(0x4682B4FF)),
    ("tan", BGRA8::from_rgba32(0xD2B48CFF)),
    ("teal", BGRA8::from_rgba32(0x008080FF)),
    ("thistle", BGRA8::from_rgba32(0xD8BFD8FF)),
    ("tomato", BGRA8::from_rgba32(0xFF6347FF)),
    ("turquoise", BGRA8::from_rgba32(0x40E0D0FF)),
    ("violet", BGRA8::from_rgba32(0xEE82EEFF)),
    ("wheat", BGRA8::from_rgba32(0xF5DEB3FF)),
    ("white", BGRA8::from_rgba32(0xFFFFFFFF)),
    ("whitesmoke", BGRA8::from_rgba32(0xF5F5F5FF)),
    ("yellow", BGRA8::from_rgba32(0xFFFF00FF)),
    ("yellowgreen", BGRA8::from_rgba32(0x9ACD32FF)),
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
