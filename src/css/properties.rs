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

subrandr_macros::make_css_value_parser!(
    FontStyle = normal | italic | oblique;
    WhiteSpace = normal | pre | nowrap | pre-wrap | break-spaces | pre-line;
    RubyPosition = [alternate || [over | under]] | inter-character;
);
