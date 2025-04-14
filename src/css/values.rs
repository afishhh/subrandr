use std::fmt::Debug;

use super::parse::{Parse, Token};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CssWideKeywordOr<T> {
    // NOTE: This cannot use the Token![] macro because the derives
    //       stop working so it's easier to just use the manual way
    //       in this one place.
    Inherit(super::parse::tokens::Inherit),
    Initial(super::parse::tokens::Initial),
    Unset(super::parse::tokens::Unset),
    Value(T),
}

impl<'a, T: Parse<'a>> Parse<'a> for CssWideKeywordOr<T> {
    fn parse(stream: &mut super::parse::ParseStream<'a>) -> Result<Self, super::parse::ParseError> {
        // TODO: if lookahead errors are implemented something has to be done here to show that
        //       inherit, initial, or unset may also be valid tokens
        if stream.peek::<Token![inherit]>() {
            Ok(Self::Inherit(stream.parse()?))
        } else if stream.peek::<Token![initial]>() {
            Ok(Self::Initial(stream.parse()?))
        } else if stream.peek::<Token![unset]>() {
            Ok(Self::Unset(stream.parse()?))
        } else {
            Ok(Self::Value(stream.parse()?))
        }
    }
}

// https://drafts.csswg.org/css-values/#component-combinators
