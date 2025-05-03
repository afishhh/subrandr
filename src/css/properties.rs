use std::{collections::HashMap, sync::LazyLock};

use super::{
    component::ComponentStream,
    parse::{Parse, ParseError, ParseStream},
};

type PropertyValueParserFn = fn(stream: &mut ParseStream) -> Result<AnyProperty, ParseError>;

macro_rules! make_properties {
    (
        $($css_name: literal $name: ident;)*
    ) => {
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub enum AnyProperty {
            $($name($name),)*
        }

        const PROPERTY_LIST: &[(&str, PropertyValueParserFn)] = &[
            $(
                ($css_name, (|stream| {
                    stream.parse().map(AnyProperty::$name)
                }) as PropertyValueParserFn),
            )*
        ];
    };
}

make_properties! {
    "color" Color;
    // "background-color" BackgroundColor;
    // "font-size" FontSize;
}

pub static PROPERTY_MAP: LazyLock<HashMap<&'static str, PropertyValueParserFn>> =
    LazyLock::new(|| PROPERTY_LIST.iter().copied().collect());

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Color {
    now_to_implement_value_parsing_smiley_face: bool,
}

impl Parse<'_> for Color {
    fn parse(stream: &mut ParseStream<'_>) -> Result<Self, ParseError> {
        stream.parse::<ComponentStream>()?;
        Ok(Self {
            now_to_implement_value_parsing_smiley_face: true,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundColor {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontSize {}
