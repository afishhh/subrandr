use std::{convert::Infallible, fmt::Display};

use crate::csssyn::{
    buffer::{Cursor, TokenView},
    token::{End, FunctionalNotation, Token, TokenParse, Whitespace},
    tokenizer::{Escaped, TokenKind},
    ParseError, Peek, TokenBuffer,
};

pub struct ParseStream<'a> {
    cursor: Cursor<'a>,
    tried: Vec<&'static str>,
}

impl<'a> ParseStream<'a> {
    pub fn new(cursor: Cursor<'a>) -> Self {
        Self {
            cursor: cursor.skip(Whitespace),
            tried: Vec::new(),
        }
    }

    pub fn cursor(&self) -> Cursor<'a> {
        self.cursor
    }

    pub fn advance_to(&mut self, cursor: Cursor<'a>) {
        self.cursor = cursor.skip(Whitespace);
        self.tried.clear();
    }

    fn ensure_end(&mut self) -> Result<(), ParseError> {
        if !self.peek(End) {
            Err(self.lookahead_error())
        } else {
            Ok(())
        }
    }

    pub fn parse<T: Parse<'a>>(&mut self) -> Result<T, ParseError> {
        T::parse(self)
    }

    pub fn peek<T: LookaheadPeek>(&mut self, peek: T) -> bool {
        let result = peek.peek(self.cursor);
        self.tried.push(peek.name());
        result
    }

    pub fn peek_skip<T: Peek>(&mut self, peek: T) -> bool {
        if peek.peek(self.cursor) {
            self.skip();
            true
        } else {
            false
        }
    }

    pub fn extend_attempted(&mut self, attempted: impl IntoIterator<Item = &'static str>) {
        self.tried.extend(attempted);
    }

    pub fn lookahead_error(&mut self) -> ParseError {
        self.tried.sort_unstable();
        self.tried.dedup();
        ParseError::unexpected(self.cursor, &self.tried)
    }

    pub fn skip(&mut self) {
        self.advance_to(self.cursor.next().unwrap_or(self.cursor));
    }
}

pub trait Parse<'a>: Sized {
    fn parse(stream: &mut ParseStream<'a>) -> Result<Self, ParseError>;
}

impl<'a, T: TokenParse<'a>> Parse<'a> for T {
    fn parse(stream: &mut ParseStream<'a>) -> Result<Self, ParseError> {
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
    fn parse(stream: &mut ParseStream<'a>) -> Result<Self, ParseError> {
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

pub trait LookaheadPeek: Peek + Sized {
    #[doc(hidden)]
    fn name(&self) -> &'static str;
}

impl<F: FnOnce(Infallible) -> T, T: Token> LookaheadPeek for F {
    fn name(&self) -> &'static str {
        T::name()
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
    let mut stream = ParseStream::new(cursor);
    let result = T::parse(&mut stream)?;
    stream.ensure_end()?;
    Ok(result)
}

#[cfg_attr(not(test), expect(dead_code))]
pub fn parse_str<T: for<'a> Parse<'a>>(source: &str) -> Result<T, ParseError> {
    let buffer = TokenBuffer::from_source(source)?;
    parse_cursor(buffer.start())
}
