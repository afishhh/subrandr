use std::convert::Infallible;

use crate::csssyn::{
    buffer::{Cursor, TokenView},
    tokenizer::{Escaped, HashTypeFlag, TokenKind},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

pub trait Spanned {
    fn span(&self) -> Span;
}

impl Spanned for Span {
    fn span(&self) -> Span {
        *self
    }
}

macro_rules! impl_spanned {
    ($name: ty) => {
        impl Spanned for $name {
            fn span(&self) -> Span {
                self.span
            }
        }
    };
}

pub trait Token: Sized {
    #[doc(hidden)]
    fn name() -> &'static str;

    #[doc(hidden)]
    fn peek(cursor: Cursor) -> bool;
}

pub trait TokenParse<'a>: Token + Sized {
    #[doc(hidden)]
    fn take(cursor: Cursor<'a>) -> Option<(Self, Cursor<'a>)>;
}

macro_rules! impl_token {
    (
        <$lt: lifetime> $name: ident $(<$ltarg: lifetime>)?;

        name = $err_name: literal;
        matches TokenView { $($pattern_body: tt)* };
        parse $body: expr;
    ) => {
        impl<$lt> Token for $name$(<$ltarg>)? {
            fn name() -> &'static str {
                $err_name
            }

            #[allow(unused_variables)] // for pattern variables that are only used in `take` below
            fn peek(cursor: Cursor) -> bool {
                matches!(cursor.token(), Some((TokenView { $($pattern_body)* }, _)))
            }
        }

        impl<$lt> TokenParse<$lt> for $name$(<$ltarg>)? {
            fn take(cursor: Cursor<$lt>) -> Option<(Self, Cursor<$lt>)> {
                match cursor.token() {
                    Some((
                        TokenView { $($pattern_body)* },
                        next,
                    )) => Some(($body, next)),
                    _ => None,
                }
            }
        }

        #[doc(hidden)]
        #[allow(non_snake_case)]
        pub fn $name<$lt>(marker: Infallible) -> $name$(<$ltarg>)? {
            match marker {}
        }
    };
}

#[derive(Debug, Clone, Copy)]
pub struct Ident<'a> {
    pub(super) span: Span,
    pub(super) value: Escaped<'a>,
}

impl_spanned!(Ident<'_>);

impl_token!(
    <'a> Ident<'a>;

    name = "<ident>";
    matches TokenView { span, source, kind: TokenKind::Ident };
    parse Ident { span, value: Escaped::new(source) };
);

impl<'a> Ident<'a> {
    pub fn value(&self) -> Escaped<'a> {
        self.value
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LitString<'a> {
    pub(super) span: Span,
    pub(super) value: Escaped<'a>,
}

impl_spanned!(LitString<'_>);

impl_token!(
    <'a> LitString<'a>;

    name = "<string>";
    matches TokenView { span, source, kind: TokenKind::String };
    parse LitString { span, value: Escaped::new(&source[1..source.len()-1]) };
);

impl<'a> LitString<'a> {
    pub fn value(&self) -> Escaped<'a> {
        self.value
    }
}

#[derive(Debug, Clone, Copy)]
pub struct NumericTokenValue<'a> {
    pub(super) value: &'a str,
    #[expect(dead_code)]
    pub(super) integer: bool,
}

impl<'a> NumericTokenValue<'a> {
    pub fn to_f32(self) -> f32 {
        self.value.parse().unwrap()
    }

    pub fn to_f64(self) -> f64 {
        self.value.parse().unwrap()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Number<'a> {
    pub(super) span: Span,
    pub(super) value: NumericTokenValue<'a>,
}

impl_spanned!(Number<'_>);

impl_token!(
    <'a> Number<'a>;

    name = "<number>";
    matches TokenView { span, source, kind: TokenKind::Number { integer } };
    parse Number { span, value: NumericTokenValue { value: source, integer } };
);

impl<'a> Number<'a> {
    pub fn value(&self) -> NumericTokenValue<'a> {
        self.value
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LitInt<'a> {
    span: Span,
    value: &'a str,
}

impl_spanned!(LitInt<'_>);

impl_token! {
    <'a> LitInt<'a>;

    name = "<integer>";
    matches TokenView { span, source, kind: TokenKind::Number { integer: true } };
    parse LitInt { span, value: source };
}

impl LitInt<'_> {
    pub fn to_u32(self) -> Option<u32> {
        self.value.parse().ok()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Percentage<'a> {
    pub(super) span: Span,
    pub(super) value: NumericTokenValue<'a>,
}

impl_spanned!(Percentage<'_>);

impl_token!(
    <'a> Percentage<'a>;

    name = "<percentage>";
    matches TokenView { span, source, kind: TokenKind::Percentage { integer } };
    parse Percentage { span, value: NumericTokenValue { value: &source[..source.len()-1], integer } };
);

impl<'a> Percentage<'a> {
    pub fn value(&self) -> NumericTokenValue<'a> {
        self.value
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Dimension<'a> {
    pub(super) span: Span,
    pub(super) text: &'a str,
    pub(super) integer: bool,
    pub(super) unit_offset: u32,
}

impl_spanned!(Dimension<'_>);

impl_token!(
    <'a> Dimension<'a>;

    name = "<dimension>";
    matches TokenView { span, source, kind: TokenKind::Dimension { integer, unit_offset } };
    parse Dimension { span, text: source, integer, unit_offset };
);

impl<'a> Dimension<'a> {
    pub fn value(&self) -> NumericTokenValue<'a> {
        NumericTokenValue {
            value: &self.text[..self.unit_offset as usize],
            integer: self.integer,
        }
    }

    pub fn unit(&self) -> Escaped<'a> {
        Escaped::new(&self.text[self.unit_offset as usize..])
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FunctionalNotation<'a> {
    pub(super) span: Span,
    #[expect(dead_code)]
    pub(super) function: Escaped<'a>,
    pub(super) content: Cursor<'a>,
}

impl_spanned!(FunctionalNotation<'_>);

impl<'a> FunctionalNotation<'a> {
    pub fn content(&self) -> Cursor<'a> {
        self.content
    }
}

impl<'a> Token for FunctionalNotation<'a> {
    fn name() -> &'static str {
        "<function>"
    }

    fn peek(cursor: Cursor) -> bool {
        matches!(
            cursor.token(),
            Some((
                TokenView {
                    kind: TokenKind::Function,
                    ..
                },
                _
            ))
        )
    }
}

#[doc(hidden)]
#[allow(non_snake_case)]
pub fn FunctionalNotation<'a>(marker: Infallible) -> FunctionalNotation<'a> {
    match marker {}
}

#[derive(Debug, Clone, Copy)]
pub struct Hash<'a> {
    span: Span,
    value: Escaped<'a>,
    type_flag: HashTypeFlag,
}

impl_spanned!(Hash<'_>);

impl_token! {
    <'a> Hash<'a>;

    name = "<hash>";
    matches TokenView { span, source, kind: TokenKind::Hash { type_flag } };
    parse Hash { span, value: Escaped::new(&source[1..]), type_flag };
}

impl<'a> Hash<'a> {
    pub fn value(&self) -> Escaped<'a> {
        self.value
    }

    #[expect(dead_code)]
    pub fn type_flag(&self) -> HashTypeFlag {
        self.type_flag
    }
}
