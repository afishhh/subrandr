use std::{cell::Cell, convert::Infallible, fmt::Display};

use super::*;
use crate::csssyn::{
    tokenizer::{Escaped, TokenKind, Tokenizer},
    value::token::TokenParse,
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
    pub fn new(cursor: Cursor<'a>) -> Self {
        Self {
            cursor: Cell::new(cursor.skip(Whitespace)),
        }
    }

    fn cursor(&self) -> Cursor<'a> {
        self.cursor.get()
    }

    fn advance_to(&self, cursor: Cursor<'a>) {
        self.cursor.set(cursor.skip(Whitespace))
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
        let cursor = self.cursor();
        self.advance_to(cursor.next().unwrap_or(cursor));
    }

    pub fn lookahead1(&self) -> Lookahead<'a> {
        Lookahead {
            cursor: self.cursor(),
            tried: Vec::new(),
        }
    }
}

pub trait Parse<'a>: Sized {
    fn parse(stream: &ParseStream<'a>) -> Result<Self, ParseError>;
}

impl<'a, T: TokenParse<'a>> Parse<'a> for T {
    fn parse(stream: &super::ParseStream<'a>) -> Result<Self, crate::csssyn::value::ParseError> {
        let cursor = stream.cursor();
        match T::take(cursor) {
            Some((value, next)) => {
                stream.advance_to(next);
                Ok(value)
            }
            None => Err(ParseError::unexpected(cursor, &[T::name()])),
        }
    }
}

pub trait Peek: Sized {
    #[doc(hidden)]
    fn peek(&self, cursor: Cursor) -> bool;
}

impl Peek for &'static str {
    fn peek(&self, cursor: Cursor) -> bool {
        cursor.token().is_some_and(|(token, _)| match token.kind {
            TokenKind::Ident => Escaped::new(token.source) == *self,
            _ => false,
        })
    }
}

pub trait LookaheadPeek: Peek + Sized {
    #[doc(hidden)]
    fn name(&self) -> &'static str;
}

impl LookaheadPeek for &'static str {
    fn name(&self) -> &'static str {
        self
    }
}

impl<F: FnOnce(Infallible) -> T, T: token::Token> Peek for F {
    fn peek(&self, cursor: Cursor) -> bool {
        T::peek(cursor)
    }
}

impl<'a, F: FnOnce(Infallible) -> T, T: token::Token> LookaheadPeek for F {
    fn name(&self) -> &'static str {
        T::name()
    }
}

macro_rules! impl_token {
    (
        for <$lt: lifetime> $name: ident $(<$ltarg: lifetime>)?;

        name = $err_name: literal;
        matches TokenView { $($pattern_body: tt)* };
        parse $body: expr;
    ) => {
        impl<$lt> token::Token for $name$(<$ltarg>)? {
            fn name() -> &'static str {
                $err_name
            }

            #[allow(unused_variables)] // for pattern variables that are only used in `take` below
            fn peek(cursor: Cursor) -> bool {
                matches!(cursor.token(), Some((TokenView { $($pattern_body)* }, _)))
            }
        }

        impl<$lt> token::TokenParse<$lt> for $name$(<$ltarg>)? {
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

impl_token!(
    for<'a> Ident<'a>;

    name = "<ident>";
    matches TokenView { span, source, kind: TokenKind::Ident };
    parse Ident { span, value: Escaped::new(source) };
);

impl_token!(
    for<'a> LitString<'a>;

    name = "<string>";
    matches TokenView { span, source, kind: TokenKind::String };
    parse LitString { span, value: Escaped::new(&source[1..source.len()-1]) };
);

impl_token!(
    for<'a> Number<'a>;

    name = "<number>";
    matches TokenView { span, source, kind: TokenKind::Number { integer } };
    parse Number { span, value: NumericTokenValue { value: source, integer } };
);

impl_token!(
    for<'a> Percentage<'a>;

    name = "<percentage>";
    matches TokenView { span, source, kind: TokenKind::Percentage { integer } };
    parse Percentage { span, value: NumericTokenValue { value: &source[..source.len()-1], integer } };
);

impl_token!(
    for<'a> Dimension<'a>;

    name = "<dimension>";
    matches TokenView { span, source, kind: TokenKind::Dimension { integer, unit_offset } };
    parse Dimension { span, text: source, integer, unit_offset };
);

impl_token!(
    for<'a> UnquotedUrl<'a>;

    name = "<unquoted-url>";
    matches TokenView { span, source, kind: TokenKind::Url { value_offset, trailing_len } };
    parse UnquotedUrl {
        span,
        value: Escaped::new(
            &source[usize::from(value_offset)..source.len() - usize::from(trailing_len)]
        )
    };
);

impl_token!(
    for<'a> Punct;

    name = "<punct>";
    matches TokenView { span, source: _, kind: TokenKind::Punct(c) };
    parse Punct { span, value: c };
);

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

impl<'a> Parse<'a> for FunctionalNotation<'a> {
    fn parse(stream: &ParseStream<'a>) -> Result<Self, ParseError> {
        let cursor = stream.cursor();
        let Some((
            TokenView {
                span,
                source,
                kind: TokenKind::Function,
            },
            next,
        )) = cursor.token()
        else {
            return Err(ParseError::unexpected(cursor, &[Self::name()]));
        };

        let Some(group_end) = cursor.group_end() else {
            return Err(ParseError::new(cursor, "unclosed functional notation"));
        };

        let inner = next.limited(group_end);
        stream.advance_to(group_end.next().unwrap());

        Ok(FunctionalNotation {
            span,
            function: Escaped::new(&source[..source.len() - 1]),
            content: inner,
        })
    }
}

#[doc(hidden)]
#[allow(non_snake_case)]
pub fn FunctionalNotation<'a>(marker: Infallible) -> FunctionalNotation<'a> {
    match marker {}
}

pub mod token {
    use super::{Cursor, LookaheadPeek, Peek};
    use crate::csssyn::{tokenizer::TokenKind, value::token_buffer::TokenView};

    #[doc(hidden)]
    pub trait Token: Sized {
        #[doc(hidden)]
        fn name() -> &'static str;

        #[doc(hidden)]
        fn peek(cursor: Cursor) -> bool;
    }

    #[doc(hidden)]
    pub trait TokenParse<'a>: Token + Sized {
        #[doc(hidden)]
        fn take(cursor: Cursor<'a>) -> Option<(Self, Cursor<'a>)>;
    }

    #[doc(hidden)]
    pub struct Zero;

    impl Peek for Zero {
        fn peek(&self, cursor: Cursor) -> bool {
            matches!(
                cursor.token(),
                Some((
                    TokenView {
                        span: _,
                        source: "0",
                        kind: TokenKind::Number { integer: true },
                    },
                    _
                ))
            )
        }
    }

    impl LookaheadPeek for Zero {
        fn name(&self) -> &'static str {
            "0"
        }
    }

    macro_rules! impl_peeks {
        ($($name: ident, $value: literal, $value_token: tt;)*) => {
            $(#[doc(hidden)]
            #[derive(Clone, Copy)]
            pub struct $name;

            impl Peek for $name {
                fn peek(&self, cursor: super::Cursor) -> bool {
                    matches!(cursor.token(), Some((
                        TokenView {
                            span: _,
                            source: _,
                            kind: TokenKind::Punct($value)
                        },
                        _
                    )))
                }
            }

            impl LookaheadPeek for $name {
                fn name(&self) -> &'static str {
                    stringify!($value_token)
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
        Colon, ':', :;
        Semicolon, ';', ;;
        ExclamationMark, '!', !;
    );
}

pub(crate) use token::Token;

#[derive(Debug, Clone, Copy)]
pub struct LitInt<'a> {
    span: Span,
    value: &'a str,
}

impl_spanned!(LitInt<'_>);

impl_token! {
    for<'a> LitInt<'a>;

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
pub struct AtKeyword<'a> {
    span: Span,
    value: Escaped<'a>,
}

impl_spanned!(AtKeyword<'_>);

impl_token! {
    for<'a> AtKeyword<'a>;

    name = "<at-keyword>";
    matches TokenView { span, source, kind: TokenKind::AtKeyword };
    parse AtKeyword { span, value: Escaped::new(&source[1..]) };
}

impl<'a> AtKeyword<'a> {
    pub fn value(&self) -> Escaped<'a> {
        self.value
    }
}

pub struct Whitespace;

impl Peek for Whitespace {
    fn peek(&self, cursor: Cursor) -> bool {
        cursor
            .token()
            .is_some_and(|(view, _)| matches!(view.kind, TokenKind::Whitespace))
    }
}

impl LookaheadPeek for Whitespace {
    fn name(&self) -> &'static str {
        "}"
    }
}

pub struct RightBrace;

impl Peek for RightBrace {
    fn peek(&self, cursor: Cursor) -> bool {
        cursor
            .token()
            .is_some_and(|(view, _)| matches!(view.kind, TokenKind::RBrace))
    }
}

impl LookaheadPeek for RightBrace {
    fn name(&self) -> &'static str {
        "}"
    }
}

pub struct End;

impl Peek for End {
    fn peek(&self, cursor: Cursor) -> bool {
        cursor.eof()
    }
}

impl LookaheadPeek for End {
    fn name(&self) -> &'static str {
        "<eof>"
    }
}

pub struct Lookahead<'a> {
    cursor: Cursor<'a>,
    tried: Vec<&'static str>,
}

impl<'a> Lookahead<'a> {
    pub fn peek<T: LookaheadPeek>(&mut self, peek: T) -> bool {
        self.tried.push(peek.name());
        peek.peek(self.cursor)
    }

    pub fn error(self) -> ParseError {
        ParseError::unexpected(self.cursor, &self.tried)
    }
}

impl ParseError {
    fn display_cursor_name(cursor: Cursor<'_>) -> impl Display + use<'_> {
        util::fmt_from_fn(move |f| {
            let Some((token, _)) = cursor.token() else {
                // TODO: if we're in a block this should be block terminator instead
                return f.write_str("<eof>");
            };

            f.write_str(match token.kind {
                TokenKind::LParen => "(",
                TokenKind::LBracket => "[",
                TokenKind::LBrace => "{",
                TokenKind::Function => "<function>",
                TokenKind::Ident => {
                    return Display::fmt(&Escaped::new(token.source), f);
                }
                TokenKind::AtKeyword => "<at-keyword>",
                TokenKind::Hash { .. } => "<hash>",
                TokenKind::Number { .. } => "<number>",
                TokenKind::Percentage { .. } => "<percentage>",
                TokenKind::Dimension { unit_offset, .. } => {
                    return write!(
                        f,
                        "<dimension-{}>",
                        Escaped::new(&token.source[unit_offset as usize..])
                    )
                }
                TokenKind::Url { .. } => "<unquoted-url>",
                TokenKind::String => "<string>",
                TokenKind::RParen => ")",
                TokenKind::RBracket => "]",
                TokenKind::RBrace => "}",
                TokenKind::Punct(chr) => return Display::fmt(&chr, f),
                TokenKind::Cdc => "-->",
                TokenKind::Cdo => "<!--",
                TokenKind::Whitespace => "<whitespace>",
                TokenKind::BadString => "<bad-string>",
                TokenKind::BadUrl => "<bad-url>",
            })
        })
    }

    fn unexpected(cursor: Cursor, expected: &[&'static str]) -> Self {
        let found = Self::display_cursor_name(cursor);
        match expected {
            [] => unreachable!("`Lookahead::error()` called before any `peek()`s"),
            [one] => Self::new(cursor, format_args!("expected `{one}` found `{found}`",)),
            [one, two] => Self::new(
                cursor,
                format_args!("expected `{one}` or `{two}`, found `{found}`",),
            ),
            [first, ref middle @ .., last] => Self::new(
                cursor,
                util::fmt_from_fn(move |f| {
                    write!(f, "expected one of {first}")?;
                    for &name in middle {
                        write!(f, ", {name}")?;
                    }
                    write!(f, ", or `{last}`, found `{found}`")
                }),
            ),
        }
    }
}

pub fn parse_str<T: for<'a> Parse<'a>>(source: &str) -> Result<T, ParseError> {
    let buffer = TokenBuffer::from_tokenizer(Tokenizer::new(source))?;
    let stream = ParseStream::new(buffer.start());
    let result = T::parse(&stream)?;
    stream.ensure_end()?;
    Ok(result)
}
