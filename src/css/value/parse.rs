use std::cell::Cell;

use super::*;
use crate::css::tokenizer::{
    DimensionToken, Escaped, NumberToken, PercentageToken, Span, Spanned, Token, TokenKind,
    TokenStream,
};

pub enum ValueTokenTree<'a> {
    Ident(Ident<'a>),
    String(StringLit<'a>),
    FunctionalNotation(FunctionalNotation<'a>),
    UnquotedUrl(Escaped<'a>),
    Number(Number),
    Percentage(PercentageToken),
    Dimension(Dimension<'a>),
    Comma(Comma),
}

impl<'a> Spanned for ValueTokenTree<'a> {
    fn span(&self) -> Span {
        match self {
            ValueTokenTree::Ident(ident) => ident.span(),
            ValueTokenTree::String(string) => string.span(),
            ValueTokenTree::FunctionalNotation(functional_notation) => todo!(),
            ValueTokenTree::UnquotedUrl(escaped) => todo!(),
            ValueTokenTree::Number(number) => todo!(),
            ValueTokenTree::Percentage(percentage_token) => todo!(),
            ValueTokenTree::Dimension(dimension) => todo!(),
            ValueTokenTree::Comma(comma) => todo!(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Ident<'a> {
    span: Span,
    value: Escaped<'a>,
}

impl Spanned for Ident<'_> {
    fn span(&self) -> Span {
        self.span
    }
}

impl<'a> Ident<'a> {
    pub fn value(&self) -> Escaped<'a> {
        self.value
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StringLit<'a> {
    span: Span,
    value: Escaped<'a>,
}

impl Spanned for StringLit<'_> {
    fn span(&self) -> Span {
        self.span
    }
}

impl<'a> StringLit<'a> {
    pub fn value(&self) -> Escaped<'a> {
        self.value
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Number {
    token: NumberToken,
}

impl Number {
    pub fn is_integer(&self) -> bool {
        self.token.integer
    }

    pub fn value(&self) -> f64 {
        self.token.value
    }
}

pub struct FunctionalNotation<'a> {
    function: Escaped<'a>,
    content: Vec<Token<'a>>,
}

#[derive(Debug, Clone, Copy)]
pub struct Dimension<'a> {
    token: DimensionToken<'a>,
}

impl<'a> Dimension<'a> {
    pub fn is_integer(&self) -> bool {
        self.token.integer
    }

    pub fn value(&self) -> f64 {
        self.token.value
    }

    pub fn unit(&self) -> Escaped<'a> {
        self.token.unit
    }
}

impl<'a> ValueTokenTree<'a> {
    fn try_next_in(stream: &mut TokenStream<'a>) -> Result<Option<ValueTokenTree<'a>>, ParseError> {
        loop {
            let Some(token) = stream.consume_token() else {
                return Ok(None);
            };

            let span = token.span;
            return Ok(Some(match token.kind {
                TokenKind::Whitespace => continue,
                TokenKind::Ident(value) => ValueTokenTree::Ident(Ident { span, value }),
                TokenKind::String(value) => ValueTokenTree::String(StringLit { span, value }),
                TokenKind::Function(function) => {
                    let mut content = Vec::new();
                    loop {
                        let Some(token) = stream.consume_token() else {
                            return Err(ParseError::new(
                                token.span,
                                "unterminated functional notation",
                            ));
                        };

                        if token.kind == TokenKind::RParen {
                            break ValueTokenTree::FunctionalNotation(FunctionalNotation {
                                function,
                                content,
                            });
                        }

                        content.push(token);
                    }
                }
                TokenKind::Url(url) => ValueTokenTree::UnquotedUrl(url),
                TokenKind::Number(token) => ValueTokenTree::Number(Number { token }),
                TokenKind::Percentage(percentage) => ValueTokenTree::Percentage(percentage),
                TokenKind::Dimension(token) => ValueTokenTree::Dimension(Dimension { token }),
                TokenKind::Comma => ValueTokenTree::Comma(Comma(())),
                kind => {
                    return Err(ParseError::new(
                        span,
                        format_args!("`{}` not legal in values", kind.name()),
                    ))
                }
            }));
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            ValueTokenTree::Ident(_) => "<ident>",
            ValueTokenTree::String(_) => "<string>",
            ValueTokenTree::FunctionalNotation(_) => "<function>",
            ValueTokenTree::UnquotedUrl(_) => "<unquoted-url>",
            &ValueTokenTree::Number(Number {
                token: NumberToken { integer, .. },
            }) => match integer {
                true => "<integer>",
                false => "<number>",
            },
            ValueTokenTree::Percentage(_) => "<percentage>",
            ValueTokenTree::Dimension(_) => "<dimension>",
            ValueTokenTree::Comma(_) => ",",
        }
    }
}

pub struct ValueParseStream<'a> {
    source: &'a str,
    tts: Vec<ValueTokenTree<'a>>,
    position: Cell<usize>,
}

impl<'a> ValueParseStream<'a> {
    pub(super) fn new(source: &'a str) -> Result<Self, ParseError> {
        let mut stream = TokenStream::new(source);
        let tts = std::iter::from_fn(move || ValueTokenTree::try_next_in(&mut stream).transpose())
            .collect::<Result<Vec<ValueTokenTree>, ParseError>>()?;
        Ok(Self {
            source,
            tts,
            position: Cell::new(0),
        })
    }

    fn advance_by(&self, count: usize) {
        self.position.set(self.position.get() + count);
    }

    fn cursor(&self, offset: usize) -> Cursor {
        Cursor {
            stream: self,
            index: self.position.get() + offset,
        }
    }

    fn ensure_end(&self) -> Result<(), ParseError> {
        let cursor = self.cursor(0);
        if !cursor.eof() {
            Err(ParseError::unexpected(cursor, &["<eof>"]))
        } else {
            Ok(())
        }
    }

    fn end_span(&self) -> Span {
        Span {
            start: self.source.len() as u32,
            end: self.source.len() as u32,
        }
    }

    pub fn parse<T: ValueParse<'a>>(&'a self) -> Result<T, ParseError> {
        T::parse(self)
    }

    pub fn peek<T: ValuePeek>(&self) -> bool {
        T::peek(&self.cursor(0))
    }

    pub fn skip(&self) {
        self.advance_by(1);
    }

    pub fn lookahead1(&self) -> Lookahead {
        Lookahead {
            cursor: self.cursor(0),
            tried: Vec::new(),
        }
    }
}

struct Cursor<'a> {
    stream: &'a ValueParseStream<'a>,
    index: usize,
}

impl<'a> Cursor<'a> {
    fn tree(&self) -> Option<&'a ValueTokenTree<'a>> {
        self.stream.tts.get(self.index)
    }

    fn eof(&self) -> bool {
        self.tree().is_none()
    }
}

impl<'a> Spanned for Cursor<'a> {
    fn span(&self) -> Span {
        if let Some(tree) = self.tree() {
            tree.span()
        } else {
            self.stream.end_span()
        }
    }
}

impl ParseError {
    fn unexpected(cursor: Cursor, expected: &[&'static str]) -> Self {
        let found = cursor.tree().map_or("<eof>", ValueTokenTree::name);
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
    fn parse(stream: &'a ValueParseStream<'a>) -> Result<Self, ParseError>;
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
            fn parse(stream: &'a ValueParseStream<'a>) -> Result<Self, ParseError> {
                let next = stream.cursor(0);
                if let Some(ValueTokenTree::$kind($v)) = next.tree() {
                    stream.advance_by(1);
                    Ok($body)
                } else {
                    Err(ParseError::unexpected(next, &[<$peektype>::name()]))
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

macro_rules! def_simple_token {
    ($name: ident, $err_name: literal) => {
        #[derive(Debug, Clone, Copy)]
        pub struct $name(());

        impl_token!($name, for<'a> $name, $name, ",", |v| *v);
    };
}

impl_token!(Ident<'_>, for<'a> &'a Ident<'a>, Ident, "<ident>", |ident| ident);
impl_token!(StringLit<'_>, for<'a> &'a StringLit<'a>, String, "<ident>", |string| string);
impl_token!(Number, for<'a> Number, Number, "<number>", |number| *number);
impl_token!(
    Dimension<'_>,
    for<'a> Dimension<'a>,
    Dimension,
    "<dimension>",
    |dimension| *dimension
);
def_simple_token!(Comma, ",");

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
}

impl Lookahead<'_> {
    pub fn peek<T: ValuePeek>(&mut self) -> bool {
        self.tried.push(T::name());
        T::peek(&self.cursor)
    }

    pub fn peek_keyword(&mut self, keyword: &'static str) -> bool {
        self.tried.push(keyword);
        if let Some(ValueTokenTree::Ident(ident)) = self.cursor.tree() {
            ident.value().eq_ignore_ascii_case(keyword)
        } else {
            false
        }
    }

    pub fn error(self) -> ParseError {
        ParseError::unexpected(self.cursor, &self.tried)
    }
}

pub fn parse_str<T: for<'a> ValueParse<'a>>(source: &str) -> Result<T, ParseError> {
    let stream = ValueParseStream::new(source)?;
    let result = T::parse(&stream)?;
    stream.ensure_end()?;
    Ok(result)
}
