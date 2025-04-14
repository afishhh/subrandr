use std::{cell::Cell, convert::Infallible, fmt::Display};

use crate::csssyn::{
    buffer::{Cursor, TokenView},
    peek::Whitespace,
    token::{FunctionalNotation, Token, TokenParse},
    tokenizer::{Escaped, TokenKind, Tokenizer},
    ParseError, Peek, TokenBuffer,
};

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
    fn parse(stream: &super::ParseStream<'a>) -> Result<Self, ParseError> {
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

impl<F: FnOnce(Infallible) -> T, T: Token> Peek for F {
    fn peek(&self, cursor: Cursor) -> bool {
        T::peek(cursor)
    }
}

impl<F: FnOnce(Infallible) -> T, T: Token> LookaheadPeek for F {
    fn name(&self) -> &'static str {
        T::name()
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

    pub fn peek_skip<T: LookaheadPeek>(&mut self, peek: T, stream: &ParseStream) -> bool {
        self.tried.push(peek.name());
        if peek.peek(self.cursor) {
            stream.skip();
            true
        } else {
            false
        }
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

pub fn parse_cursor<'a, T: Parse<'a>>(cursor: Cursor<'a>) -> Result<T, ParseError> {
    let stream = ParseStream::new(cursor);
    let result = T::parse(&stream)?;
    stream.ensure_end()?;
    Ok(result)
}

pub fn parse_str<T: for<'a> Parse<'a>>(source: &str) -> Result<T, ParseError> {
    let buffer = TokenBuffer::from_source(source)?;
    parse_cursor(buffer.start())
}
