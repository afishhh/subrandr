use std::cell::Cell;

use crate::css::tokenizer::{Escaped, Tokenizer};

mod error;
mod token_buffer;
mod token_tree;

pub use error::ParseError;
use token_buffer::*;
pub use token_tree::*;

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
            cursor: Cell::new(buffer.start()),
        })
    }

    fn advance1(&self) {
        self.cursor.set(self.cursor2());
    }

    fn cursor1(&self) -> Cursor<'a> {
        self.cursor.get()
    }

    fn cursor2(&self) -> Cursor<'a> {
        self.cursor1().next()
    }

    fn ensure_end(&self) -> Result<(), ParseError> {
        let cursor = self.cursor1();
        if !cursor.eof() {
            Err(ParseError::unexpected(cursor, &["<eof>"], false))
        } else {
            Ok(())
        }
    }

    pub fn parse<T: ValueParse<'a>>(&'a self) -> Result<T, ParseError> {
        T::parse(self)
    }

    pub fn peek<T: ValuePeek>(&self) -> bool {
        T::peek(&self.cursor1())
    }

    pub fn skip(&self) {
        self.advance1();
    }

    pub fn lookahead1(&self) -> Lookahead {
        Lookahead {
            cursor: self.cursor1(),
            tried: Vec::new(),
            reveal_ident: false,
        }
    }
}

impl ParseError {
    fn unexpected(cursor: Cursor, expected: &[&'static str], reveal_ident: bool) -> Self {
        let found = util::fmt_from_fn(|f| match cursor.tree() {
            Some(tt) => std::fmt::Display::fmt(&tt.display_for_error(reveal_ident), f),
            None => f.write_str("<eof>"),
        });
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

pub trait ValueParse<'a>: Sized {
    fn parse(stream: &'a ParseStream<'a>) -> Result<Self, ParseError>;
}

pub trait ValuePeek: Sized {
    #[doc(hidden)]
    fn peek(tt: &Cursor) -> bool;

    #[doc(hidden)]
    fn name() -> &'static str;
}

macro_rules! impl_token {
    ($peektype: ty, for<$lt: lifetime> $ftype: ty, $kind: ident, $err_name: literal,
        |$v: ident| $body: expr
    ) => {
        impl<$lt> ValueParse<$lt> for $ftype {
            fn parse(stream: &'a ParseStream<'a>) -> Result<Self, ParseError> {
                let next = stream.cursor1();
                if let Some(ValueTokenTree::$kind($v)) = next.tree() {
                    stream.advance1();
                    Ok($body)
                } else {
                    Err(ParseError::unexpected(next, &[<$peektype>::name()], false))
                }
            }
        }

        impl ValuePeek for $peektype {
            fn name() -> &'static str {
                $err_name
            }

            fn peek(cursor: &Cursor) -> bool {
                matches!(cursor.tree(), Some(ValueTokenTree::$kind(_)))
            }
        }
    };
}

impl_token!(Ident<'_>, for<'a> &'a Ident<'a>, Ident, "<ident>", |ident| ident);
impl_token!(StringLit<'_>, for<'a> &'a StringLit<'a>, String, "<ident>", |string| string);
impl_token!(
    Number<'_>,
    for<'a> Number<'a>,
    Number,
    "<number>",
    |number| *number
);
impl_token!(
    Percentage<'_>,
    for<'a> Percentage<'a>,
    Percentage,
    "<dimension>",
    |dimension| *dimension
);
impl_token!(
    Dimension<'_>,
    for<'a> Dimension<'a>,
    Dimension,
    "<dimension>",
    |dimension| *dimension
);

pub struct End(());

impl ValuePeek for End {
    fn name() -> &'static str {
        "<eof>"
    }

    fn peek(tt: &Cursor) -> bool {
        tt.eof()
    }
}

pub struct Lookahead<'a> {
    cursor: Cursor<'a>,
    tried: Vec<&'static str>,
    reveal_ident: bool,
}

impl Lookahead<'_> {
    pub fn peek<T: ValuePeek>(&mut self) -> bool {
        self.tried.push(T::name());
        T::peek(&self.cursor)
    }

    pub fn peek_keyword(&mut self, keyword: &'static str) -> bool {
        self.tried.push(keyword);
        self.reveal_ident = true;
        if let Some(ValueTokenTree::Ident(ident)) = self.cursor.tree() {
            ident.value().eq_ignore_ascii_case(keyword)
        } else {
            false
        }
    }

    pub fn error(self) -> ParseError {
        ParseError::unexpected(self.cursor, &self.tried, self.reveal_ident)
    }
}

pub fn parse_str<T: for<'a> ValueParse<'a>>(source: &str) -> Result<T, ParseError> {
    let buffer = TokenBuffer::from_tokenizer(Tokenizer::new(source))?;
    let stream = ParseStream::new(&buffer)?;
    let result = T::parse(&stream)?;
    stream.ensure_end()?;
    Ok(result)
}
