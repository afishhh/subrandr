use std::{cell::Cell, convert::Infallible};

use super::*;
use crate::csssyn::tokenizer::Tokenizer;

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
pub(super) use impl_spanned;

#[derive(Debug)]
pub struct LineColumn {
    pub line: u32,
    pub column: u32,
}

pub struct ParseStream<'a> {
    cursor: Cell<Cursor<'a>>,
}

impl<'a> ParseStream<'a> {
    fn new(buffer: &'a TokenBuffer<'a>) -> Result<Self, ParseError> {
        Ok(Self {
            cursor: Cell::new(buffer.start().skip_whitespace()),
        })
    }

    fn cursor(&self) -> Cursor<'a> {
        self.cursor.get()
    }

    fn ensure_end(&self) -> Result<(), ParseError> {
        let cursor = self.cursor();
        if !cursor.eof() {
            Err(ParseError::unexpected(cursor, &["<eof>"]))
        } else {
            Ok(())
        }
    }

    pub fn parse<T: Parse<'a>>(&self) -> Result<T, ParseError> {
        T::parse(self)
    }

    pub fn peek<T: Peek>(&self, peek: T) -> bool {
        peek.peek(self.cursor())
    }

    pub fn skip(&self) {
        _ = self.parse::<TokenTree<'a>>();
    }

    pub fn lookahead1(&self) -> Lookahead<'a> {
        Lookahead {
            cursor: self.cursor(),
            tried: Vec::new(),
            reveal_ident: false,
        }
    }
}

pub trait Parse<'a>: Sized {
    fn parse(stream: &ParseStream<'a>) -> Result<Self, ParseError>;
}

pub trait Peek: Sized {
    #[doc(hidden)]
    fn name(&self) -> &'static str;

    #[doc(hidden)]
    fn is_keyword() -> bool {
        false
    }

    #[doc(hidden)]
    fn peek(&self, cursor: Cursor) -> bool;
}

impl Peek for &'static str {
    fn name(&self) -> &'static str {
        self
    }

    fn is_keyword() -> bool {
        true
    }

    fn peek(&self, cursor: Cursor) -> bool {
        cursor.token_tree().is_some_and(|(tree, _)| match tree {
            TokenTree::Ident(ident) => ident.value() == *self,
            _ => false,
        })
    }
}

impl<F: FnOnce(Infallible) -> T, T: token::Token> Peek for F {
    fn name(&self) -> &'static str {
        T::name()
    }

    fn peek(&self, cursor: Cursor) -> bool {
        T::peek(cursor)
    }
}

macro_rules! impl_token {
    (for<$lt: lifetime> $name: ident $(<$ltarg: lifetime>)?, $kind: ident, $err_name: literal
    ) => {
        impl<$lt> Parse<$lt> for $name $(<$ltarg>)? {
            fn parse(stream: &ParseStream<'a>) -> Result<Self, ParseError> {
                let cursor = stream.cursor();
                if let Some((TokenTree::$kind(token), next)) = cursor.token_tree() {
                    stream.cursor.set(next);
                    Ok(token)
                } else {
                    Err(ParseError::unexpected(cursor, &[<$name>::name()]))
                }
            }
        }

        impl<$lt> token::Token for $name $(<$ltarg>)? {
            fn name() -> &'static str {
                $err_name
            }

            fn peek(cursor: Cursor) -> bool {
                matches!(cursor.token_tree(), Some((TokenTree::$kind(..), _)))
            }
        }

        #[doc(hidden)]
        #[allow(non_snake_case)]
        pub fn $name<$lt>(marker: Infallible) -> $name $(<$ltarg>)? {
            match marker {}
        }
    };
}

impl<'a> Parse<'a> for TokenTree<'a> {
    fn parse(stream: &ParseStream<'a>) -> Result<Self, ParseError> {
        let cursor = stream.cursor();
        let (tree, next) = cursor.token_tree().ok_or_else(|| {
            ParseError::new(cursor, format_args!("{cursor} is not valid in values"))
        })?;
        stream.cursor.set(next);
        Ok(tree)
    }
}

impl_token!(for<'a> Ident<'a>, Ident, "<ident>");
impl_token!(for<'a> LitString<'a>, String, "<ident>");
impl_token!(for<'a> Number<'a>, Number, "<number>");
impl_token!(for<'a> Percentage<'a>, Percentage, "<dimension>");
impl_token!(for<'a> Dimension<'a>, Dimension, "<dimension>");

pub mod token {
    #[doc(hidden)]
    pub trait Token: Sized {
        #[doc(hidden)]
        fn name() -> &'static str;

        #[doc(hidden)]
        fn peek(cursor: super::Cursor) -> bool;
    }

    #[doc(hidden)]
    pub struct Zero;

    impl super::Peek for Zero {
        fn name(&self) -> &'static str {
            "0"
        }

        fn peek(&self, cursor: super::Cursor) -> bool {
            matches!(
                cursor.token_tree(),
                Some((
                    super::TokenTree::Number(super::Number {
                        span: _,
                        value: super::NumericTokenValue {
                            value: "0",
                            integer: true
                        }
                    }),
                    _
                ))
            )
        }
    }

    macro_rules! impl_peeks {
        ($($name: ident, $value: literal, $value_token: tt)*;) => {
            $(#[doc(hidden)]
            pub struct $name;

            impl super::Peek for $name {
                fn name(&self) -> &'static str {
                    stringify!($value_token)
                }

                fn peek(&self, cursor: super::Cursor) -> bool {
                    matches!(cursor.token_tree(), Some((super::TokenTree::Punct(super::Punct {
                        span: _,
                        value: $value
                    }), _)))
                }
            })*

            macro_rules! TokenMacro {
                (0) => { $crate::csssyn::value::token::Zero };
                $(($value_token) => { $crate::csssyn::value::token::$name };)*
            }
            pub(crate) use TokenMacro as Token;
        };
    }

    impl_peeks!(
        Comma, ',', ,;
    );
}

pub(crate) use token::Token;

#[derive(Debug, Clone, Copy)]
pub struct LitInt<'a> {
    span: Span,
    value: &'a str,
}

impl_spanned!(LitInt<'_>);

impl LitInt<'_> {
    pub fn to_u32(self) -> Option<u32> {
        dbg!(self.value).parse().ok()
    }
}

impl Token for LitInt<'_> {
    fn name() -> &'static str {
        "<integer>"
    }

    fn peek(cursor: Cursor) -> bool {
        matches!(
            cursor.token_tree(),
            Some((
                TokenTree::Number(Number {
                    span: _,
                    value: NumericTokenValue {
                        value: _,
                        integer: true
                    }
                }),
                _
            ))
        )
    }
}

impl<'a> Parse<'a> for LitInt<'a> {
    fn parse(stream: &ParseStream<'a>) -> Result<Self, ParseError> {
        let cursor = stream.cursor();
        if let Some((
            TokenTree::Number(Number {
                span,
                value:
                    NumericTokenValue {
                        value,
                        integer: true,
                    },
            }),
            next,
        )) = cursor.token_tree()
        {
            stream.cursor.set(next);
            Ok(LitInt { span, value })
        } else {
            Err(ParseError::unexpected(cursor, &[Self::name()]))
        }
    }
}

#[doc(hidden)]
#[allow(non_snake_case)]
pub fn LitInt(marker: Infallible) -> LitInt<'static> {
    match marker {}
}

pub struct End;

impl Peek for End {
    fn name(&self) -> &'static str {
        "<eof>"
    }

    fn peek(&self, cursor: Cursor) -> bool {
        cursor.eof()
    }
}

pub struct Lookahead<'a> {
    cursor: Cursor<'a>,
    tried: Vec<&'static str>,
    reveal_ident: bool,
}

impl Lookahead<'_> {
    pub fn peek<T: Peek>(&mut self, peek: T) -> bool {
        self.tried.push(peek.name());
        if T::is_keyword() {
            self.reveal_ident = true;
        }
        peek.peek(self.cursor)
    }

    pub fn error(self) -> ParseError {
        ParseError::unexpected(self.cursor, &self.tried)
    }
}

impl ParseError {
    fn unexpected(cursor: Cursor, expected: &[&'static str]) -> Self {
        match expected {
            [] => unreachable!("`Lookahead::error()` called before any `peek()`s"),
            [one] => Self::new(cursor, format_args!("expected `{one}` found `{cursor}`",)),
            [one, two] => Self::new(
                cursor,
                format_args!("expected `{one}` or `{two}`, found `{cursor}`",),
            ),
            [first, ref middle @ .., last] => Self::new(
                cursor,
                util::fmt_from_fn(move |f| {
                    write!(f, "expected one of {first}")?;
                    for &name in middle {
                        write!(f, ", {name}")?;
                    }
                    write!(f, ", or `{last}`, found `{cursor}`")
                }),
            ),
        }
    }
}

pub fn parse_str<T: for<'a> Parse<'a>>(source: &str) -> Result<T, ParseError> {
    let buffer = TokenBuffer::from_tokenizer(Tokenizer::new(source))?;
    let stream = ParseStream::new(&buffer)?;
    let result = T::parse(&stream)?;
    stream.ensure_end()?;
    Ok(result)
}
