use std::{collections::HashMap, sync::LazyLock};

use super::{
    component::ComponentStream,
    parse::{Lookahead1, Parse, ParseError, ParseStream, Token},
    values::CssWideKeywordOr,
};

type PropertyValueParserFn = fn(stream: &mut ParseStream) -> Result<AnyPropertyValue, ParseError>;

macro_rules! make_properties {
    (
        $($css_name: literal $name: ident;)*
    ) => {
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub enum AnyPropertyValue {
            $($name(CssWideKeywordOr<$name>),)*
        }

        const PROPERTY_LIST: &[(&str, PropertyValueParserFn)] = &[
            $(
                ($css_name, (|stream| {
                    Ok(AnyPropertyValue::$name(stream.parse()?))
                }) as PropertyValueParserFn),
            )*
        ];
    };
}

pub fn property_to_parser(name: &str) -> Option<PropertyValueParserFn> {
    let index = PROPERTY_LIST
        .binary_search_by_key(&name, |&(name, _)| name)
        .ok()?;

    Some(PROPERTY_LIST[index].1)
}

subrandr_macros::make_css_property_parser_list! {
    // "color" Color;
    "font-style" FontStyle;
    "white-space" WhiteSpace;
    "ruby-position" RubyPosition;
}

mod pst {
    use crate::css::parse::*;

    subrandr_macros::make_css_value_parser! {
        FontStyle = { normal | italic | oblique };
        WhiteSpace = { normal | pre | nowrap | pre-wrap | break-spaces | pre-line };
        RubyPosition = { [alternate || [over | under]] | inter-character };

        // https://drafts.csswg.org/css-display-3/#the-display-properties
        <display-outside>  = { block | inline | run-in  };
        <display-inside>   = { flow | flow-root | table | flex | grid | ruby };
        // this is an LL(2) grammar
        // <display-list-item> = { <display-outside>? && [ flow | flow-root ]? && list-item };
        // This is the subset we support
        <display-internal> = { ruby-base | ruby-text | ruby-base-container | ruby-text-container };
        <display-box>      = {  contents | none };
        <display-legacy>   = { inline-block | inline-table | inline-flex | inline-grid };
        Display = { [ <display-outside> || <display-inside> ] /*| <display-list-item> */| <display-internal> | <display-box> | <display-legacy> };
    }
}

#[derive(Debug, Clone, Copy)]
enum FontStyle {
    Normal,
    Italic,
    Oblique,
}

impl pst::FontStyle {
    fn convert(self) -> FontStyle {
        match self {
            Self::Normal(_) => FontStyle::Normal,
            Self::Italic(_) => FontStyle::Italic,
            Self::Oblique(_) => FontStyle::Oblique,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum WhiteSpace {
    Normal,
    Pre,
    NoWrap,
    PreWrap,
    BreakSpaces,
    PreLine,
}

impl pst::WhiteSpace {
    fn convert(self) -> WhiteSpace {
        match self {
            Self::Normal(_) => WhiteSpace::Normal,
            Self::Pre(_) => WhiteSpace::Pre,
            Self::Nowrap(_) => WhiteSpace::NoWrap,
            Self::PreWrap(_) => WhiteSpace::PreWrap,
            Self::BreakSpaces(_) => WhiteSpace::BreakSpaces,
            Self::PreLine(_) => WhiteSpace::PreLine,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum OverUnder {
    Over,
    Under,
}

#[derive(Debug, Clone, Copy)]
enum RubyPosition {
    OverUnder { alternate: bool, value: OverUnder },
    InterCharacter,
}

impl pst::RubyPosition {
    fn convert(self) -> RubyPosition {
        match self {
            Self::Unnamed0(pst::Unnamed0(alternate, b)) => RubyPosition::OverUnder {
                alternate: alternate.is_some(),
                value: match b {
                    Some(pst::Unnamed1::Under(_)) => OverUnder::Under,
                    None | Some(pst::Unnamed1::Over(_)) => OverUnder::Over,
                },
            },
            Self::InterCharacter(_) => RubyPosition::InterCharacter,
        }
    }
}
