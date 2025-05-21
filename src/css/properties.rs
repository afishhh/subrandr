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

make_properties! {
    "color" Color;
    "font-style" FontStyle;
    "white-space" WhiteSpace;
    "ruby-position" RubyPosition;
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
pub enum FontStyle {
    Normal(Token![normal]),
    Italic(Token![italic]),
    Oblique(Token![oblique]),
}

impl Parse<'_> for FontStyle {
    fn parse(stream: &mut ParseStream<'_>) -> Result<Self, ParseError> {
        let mut lk = stream.lookahead1();
        if lk.peek::<Token![normal]>() {
            Ok(Self::Normal(stream.parse()?))
        } else if lk.peek::<Token![italic]>() {
            Ok(Self::Italic(stream.parse()?))
        } else if lk.peek::<Token![oblique]>() {
            Ok(Self::Oblique(stream.parse()?))
        } else {
            Err(lk.error())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhiteSpace {
    Normal(Token![normal]),
    Pre(Token![pre]),
    NoWrap(Token![nowrap]),
    PreWrap(Token![pre-wrap]),
    BreakSpaces(Token![break-spaces]),
    PreLine(Token![pre-line]),
}

impl Parse<'_> for WhiteSpace {
    fn parse(stream: &mut ParseStream<'_>) -> Result<Self, ParseError> {
        let mut lk = stream.lookahead1();
        if lk.peek::<Token![normal]>() {
            Ok(Self::Normal(stream.parse()?))
        } else if lk.peek::<Token![pre]>() {
            Ok(Self::Pre(stream.parse()?))
        } else if lk.peek::<Token![nowrap]>() {
            Ok(Self::NoWrap(stream.parse()?))
        } else if lk.peek::<Token![pre-wrap]>() {
            Ok(Self::PreWrap(stream.parse()?))
        } else if lk.peek::<Token![break-spaces]>() {
            Ok(Self::BreakSpaces(stream.parse()?))
        } else if lk.peek::<Token![pre-line]>() {
            Ok(Self::PreLine(stream.parse()?))
        } else {
            Err(lk.error())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RubyPosition {
    Alternate(Token![alternate], Option<OverOrUnder>),
    OverOrUnder(OverOrUnder),
    InterCharacter(Token![inter-character]),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverOrUnder {
    Over(Token![over]),
    Under(Token![under]),
}

impl Parse<'_> for OverOrUnder {
    fn parse(stream: &mut ParseStream<'_>) -> Result<Self, ParseError> {
        let mut lk = stream.lookahead1();
        if lk.peek::<Token![over]>() {
            Ok(Self::Over(stream.parse()?))
        } else if lk.peek::<Token![under]>() {
            Ok(Self::Under(stream.parse()?))
        } else {
            Err(lk.error())
        }
    }
}

impl Parse<'_> for RubyPosition {
    fn parse(stream: &mut ParseStream<'_>) -> Result<Self, ParseError> {
        let mut lk = stream.lookahead1();
        if lk.peek::<Token![alternate]>() {
            Ok(Self::Alternate(
                stream.parse()?,
                if stream.is_empty() {
                    None
                } else {
                    stream.skip_whitespace();
                    Some(stream.parse()?)
                },
            ))
        } else if lk.peek::<Token![over]>() || lk.peek::<Token![under]>() {
            let over_or_under = stream.parse()?;
            stream.skip_whitespace();
            if stream.peek::<Token![alternate]>() {
                Ok(Self::Alternate(stream.parse()?, Some(over_or_under)))
            } else {
                Ok(Self::OverOrUnder(over_or_under))
            }
        } else if lk.peek::<Token![inter-character]>() {
            Ok(Self::InterCharacter(stream.parse()?))
        } else {
            Err(lk.error())
        }
    }
}
