use std::convert::Infallible;

use crate::csssyn::{
    buffer::{Cursor, TokenView},
    token::{End, FunctionalNotation, Token, TokenParse, Whitespace},
    tokenizer::{Escaped, TokenKind},
    ParseError, Peek,
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

        let mut name = peek.name();
        // HACK: Not very pretty
        if name == "<eof>" {
            if let Some(delimiter) = self.cursor.outer_delimiter() {
                name = delimiter.closing_str();
            }
        }

        self.tried.push(name);
        result
    }

    pub fn peek_skip<T: LookaheadPeek>(&mut self, peek: T) -> bool {
        if self.peek(peek) {
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

pub fn parse_cursor_with<'a, T>(
    cursor: Cursor<'a>,
    fun: impl FnOnce(&mut ParseStream<'a>) -> Result<T, ParseError>,
) -> Result<T, ParseError> {
    let mut stream = ParseStream::new(cursor);
    let result = fun(&mut stream)?;
    stream.ensure_end()?;
    Ok(result)
}

pub fn parse_cursor<'a, T: Parse<'a>>(cursor: Cursor<'a>) -> Result<T, ParseError> {
    parse_cursor_with(cursor, T::parse)
}
