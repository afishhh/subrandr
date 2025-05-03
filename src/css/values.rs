use std::fmt::Debug;

use super::parse::{Parse, Token};

pub enum CssWideKeywordOr<T> {
    Inherit(Token![inherit]),
    Initial(Token![initial]),
    Unset(Token![unset]),
    Value(T),
}

impl<T: Debug> Debug for CssWideKeywordOr<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CssWideKeywordOr::Inherit(inherit) => {
                write!(f, "CssWideKeywordOr::Inherit({inherit:?})")
            }
            CssWideKeywordOr::Initial(initial) => {
                write!(f, "CssWideKeywordOr::Initial({initial:?})")
            }
            CssWideKeywordOr::Unset(unset) => {
                write!(f, "CssWideKeywordOr::Unset({unset:?})")
            }
            CssWideKeywordOr::Value(value) => {
                write!(f, "CssWideKeywordOr::Value({value:?})")
            }
        }
    }
}

impl<T: Clone> Clone for CssWideKeywordOr<T> {
    fn clone(&self) -> Self {
        match self {
            CssWideKeywordOr::Inherit(inherit) => CssWideKeywordOr::Inherit(inherit.clone()),
            CssWideKeywordOr::Initial(initial) => CssWideKeywordOr::Initial(initial.clone()),
            CssWideKeywordOr::Unset(unset) => CssWideKeywordOr::Unset(unset.clone()),
            CssWideKeywordOr::Value(value) => CssWideKeywordOr::Value(value.clone()),
        }
    }
}

impl<T: PartialEq> PartialEq for CssWideKeywordOr<T> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (CssWideKeywordOr::Inherit(inherit1), CssWideKeywordOr::Inherit(inherit2)) => {
                inherit1 == inherit2
            }
            (CssWideKeywordOr::Initial(initial1), CssWideKeywordOr::Initial(initial2)) => {
                initial1 == initial2
            }
            (CssWideKeywordOr::Unset(unset1), CssWideKeywordOr::Unset(unset2)) => unset1 == unset2,
            (CssWideKeywordOr::Value(value1), CssWideKeywordOr::Value(value2)) => value1 == value2,
            _ => false,
        }
    }
}

impl<T: Eq> Eq for CssWideKeywordOr<T> {}

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
