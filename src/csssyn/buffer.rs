use std::{marker::PhantomData, num::NonZero, ptr::NonNull};

use super::{
    token::TokenParse,
    tokenizer::{HashTypeFlag, TokenKind, Tokenizer},
    ParseError, Peek, Span, Spanned,
};

const SOURCE_LEN_LIMIT: u32 = 1 << 20;

#[derive(Debug, PartialEq, Eq)]
pub enum Delimiter {
    Parenthesis,
    Bracket,
    Brace,
}

#[derive(Debug)]
struct Entry {
    span: Span,
    kind: EntryTokenKind,
}

#[derive(Debug)]
enum EntryTokenKind {
    OpenParenthesis(GroupStart),
    OpenBracket(GroupStart),
    OpenBrace(GroupStart),
    Function(GroupStart),
    CloseParenthesis,
    CloseBracket,
    CloseBrace,

    Punct(char),
    Cdc,
    Cdo,
    Whitespace,
    Ident,
    AtKeyword,
    Hash {
        type_flag: HashTypeFlag,
    },
    String,
    BadString,
    Url {
        value_offset: u16,
        trailing_len: u16,
    },
    BadUrl,
    Number {
        integer: bool,
    },
    Percentage {
        integer: bool,
    },
    Dimension {
        integer: bool,
        unit_offset: u32,
    },

    EndOfFile,
}

impl EntryTokenKind {
    fn as_token_kind(&self) -> Option<TokenKind> {
        Some(match self {
            EntryTokenKind::OpenParenthesis(_) => TokenKind::LParen,
            EntryTokenKind::OpenBracket(_) => TokenKind::LBracket,
            EntryTokenKind::OpenBrace(_) => TokenKind::LBrace,
            EntryTokenKind::Function(_) => TokenKind::Function,
            EntryTokenKind::CloseParenthesis => TokenKind::RParen,
            EntryTokenKind::CloseBracket => TokenKind::RBracket,
            EntryTokenKind::CloseBrace => TokenKind::RBrace,
            EntryTokenKind::EndOfFile => return None,
            &EntryTokenKind::Punct(c) => TokenKind::Punct(c),
            EntryTokenKind::Cdc => TokenKind::Cdc,
            EntryTokenKind::Cdo => TokenKind::Cdo,
            EntryTokenKind::Whitespace => TokenKind::Whitespace,
            EntryTokenKind::Ident => TokenKind::Ident,
            EntryTokenKind::AtKeyword => TokenKind::AtKeyword,
            &EntryTokenKind::Hash { type_flag } => TokenKind::Hash { type_flag },
            EntryTokenKind::String => TokenKind::String,
            EntryTokenKind::BadString => TokenKind::BadString,
            &EntryTokenKind::Url {
                value_offset,
                trailing_len,
            } => TokenKind::Url {
                value_offset,
                trailing_len,
            },
            EntryTokenKind::BadUrl => TokenKind::BadUrl,
            &EntryTokenKind::Number { integer } => TokenKind::Number { integer },
            &EntryTokenKind::Percentage { integer } => TokenKind::Percentage { integer },
            &EntryTokenKind::Dimension {
                integer,
                unit_offset,
            } => TokenKind::Dimension {
                integer,
                unit_offset,
            },
        })
    }
}

#[derive(Debug, Clone)]
struct GroupStart {
    /// Offset of the entry for the closing delimiter of this group.
    /// `None` if the group was left unclosed.
    end_offset: Option<NonZero<u32>>,
}

pub struct TokenBuffer<'a> {
    source: &'a str,
    entries: Vec<Entry>,
}

impl<'a> TokenBuffer<'a> {
    pub(super) fn from_tokenizer(mut tokenizer: Tokenizer<'a>) -> Result<Self, ParseError> {
        let source = tokenizer.source();
        let Some(source_end) = u32::try_from(source.len())
            .ok()
            .filter(|&x| x <= SOURCE_LEN_LIMIT)
        else {
            return Err(ParseError::new(
                Span { start: 0, end: 0 },
                format_args!("source longer than {SOURCE_LEN_LIMIT} bytes is unsupported"),
            ));
        };

        let mut entries: Vec<Entry> = Vec::new();
        let mut group_stack: Vec<(Delimiter, usize)> = Vec::new();
        let mut last = 0;
        while let Some(token) = tokenizer.consume_token() {
            let span = Span {
                start: last,
                end: last + token.len,
            };
            let mut close_stack = |expected_delimiter: Delimiter| {
                // TODO: `pop_if` (MSRV 1.85)
                let Some((d, idx)) = group_stack.pop() else {
                    return;
                };
                if d != expected_delimiter {
                    group_stack.push((d, idx));
                    return;
                }

                let end = NonZero::new(u32::try_from(entries.len()).unwrap()).unwrap();
                match (expected_delimiter, &mut entries[idx].kind) {
                    (Delimiter::Parenthesis, EntryTokenKind::OpenParenthesis(group)) => {
                        group.end_offset = Some(end);
                    }
                    (Delimiter::Parenthesis, EntryTokenKind::Function(group)) => {
                        group.end_offset = Some(end);
                    }
                    (Delimiter::Bracket, EntryTokenKind::OpenBracket(group)) => {
                        group.end_offset = Some(end);
                    }
                    (Delimiter::Brace, EntryTokenKind::OpenBrace(group)) => {
                        group.end_offset = Some(end);
                    }
                    (_, _) => unreachable!("group stack points to mismatched entry"),
                }
            };

            let entry_kind = match token.kind {
                TokenKind::Whitespace => EntryTokenKind::Whitespace,
                TokenKind::Ident => EntryTokenKind::Ident,
                TokenKind::String => EntryTokenKind::String,
                TokenKind::Function => EntryTokenKind::Function(GroupStart { end_offset: None }),
                TokenKind::LParen => {
                    group_stack.push((Delimiter::Parenthesis, entries.len()));
                    EntryTokenKind::OpenParenthesis(GroupStart { end_offset: None })
                }
                TokenKind::LBracket => {
                    group_stack.push((Delimiter::Bracket, entries.len()));
                    EntryTokenKind::OpenBracket(GroupStart { end_offset: None })
                }
                TokenKind::LBrace => {
                    group_stack.push((Delimiter::Brace, entries.len()));
                    EntryTokenKind::OpenBrace(GroupStart { end_offset: None })
                }
                TokenKind::RParen => {
                    close_stack(Delimiter::Parenthesis);
                    EntryTokenKind::CloseParenthesis
                }
                TokenKind::RBracket => {
                    close_stack(Delimiter::Bracket);
                    EntryTokenKind::CloseBracket
                }
                TokenKind::RBrace => {
                    close_stack(Delimiter::Brace);
                    EntryTokenKind::CloseBrace
                }

                TokenKind::Url {
                    value_offset,
                    trailing_len,
                } => EntryTokenKind::Url {
                    value_offset,
                    trailing_len,
                },
                TokenKind::Number { integer } => EntryTokenKind::Number { integer },
                TokenKind::Percentage { integer } => EntryTokenKind::Percentage { integer },
                TokenKind::Dimension {
                    integer,
                    unit_offset,
                } => EntryTokenKind::Dimension {
                    integer,
                    unit_offset,
                },
                TokenKind::Cdc => EntryTokenKind::Cdc,
                TokenKind::Cdo => EntryTokenKind::Cdo,
                TokenKind::AtKeyword => EntryTokenKind::AtKeyword,
                TokenKind::Hash { type_flag } => EntryTokenKind::Hash { type_flag },
                TokenKind::BadString => EntryTokenKind::BadString,
                TokenKind::BadUrl => EntryTokenKind::BadUrl,
                TokenKind::Punct(c) => EntryTokenKind::Punct(c),
            };

            entries.push(Entry {
                span,
                kind: entry_kind,
            });
            last = span.end;
        }

        entries.push(Entry {
            span: Span {
                start: source_end,
                end: source_end,
            },
            kind: EntryTokenKind::EndOfFile,
        });

        Ok(Self { source, entries })
    }

    pub fn from_source(source: &'a str) -> Result<Self, ParseError> {
        Self::from_tokenizer(Tokenizer::new(source))
    }

    pub fn start(&self) -> Cursor<'_> {
        unsafe {
            Cursor {
                source_base: NonNull::new_unchecked(self.source.as_ptr().cast_mut()),
                entry: self.entries.as_ptr(),
                end: self.entries.as_ptr().add(self.entries.len() - 1),
                phantom: PhantomData,
            }
        }
    }
}

pub(super) struct TokenView<'a> {
    pub span: Span,
    pub source: &'a str,
    pub kind: TokenKind,
}

#[derive(Clone, Copy)]
pub struct Cursor<'a> {
    source_base: NonNull<u8>,
    entry: *const Entry,
    end: *const Entry,
    phantom: PhantomData<(&'a str, &'a Entry)>,
}

impl<'a> Cursor<'a> {
    fn entry(&self) -> &'a Entry {
        assert!(self.entry <= self.end);
        unsafe { &(*self.entry) }
    }

    pub fn eof(self) -> bool {
        std::ptr::eq(self.entry, self.end)
    }

    pub fn limited(mut self, other: Cursor<'a>) -> Cursor<'a> {
        assert!(other.entry <= self.end);

        self.end = other.entry;
        self
    }

    unsafe fn span_source(self, span: Span) -> &'a str {
        assert!(span.start < span.end);
        unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                self.source_base.as_ptr().add(span.start as usize),
                (span.end - span.start) as usize,
            ))
        }
    }

    pub fn scope_source(self) -> &'a str {
        let start_entry = self.entry();
        let end_entry = self.end().entry();
        unsafe {
            self.span_source(Span {
                start: start_entry.span.start,
                end: end_entry.span.start,
            })
        }
    }

    #[must_use]
    pub fn is<T: Peek>(self, peek: T) -> bool {
        peek.peek(self)
    }

    #[must_use]
    pub fn next_if<T: Peek>(self, peek: T) -> Option<Cursor<'a>> {
        if self.is(peek) {
            self.next()
        } else {
            None
        }
    }

    #[must_use]
    pub fn skip<T: Peek>(self, peek: T) -> Cursor<'a> {
        if self.is(peek) {
            self.next().unwrap_or(self)
        } else {
            self
        }
    }

    #[must_use]
    pub fn take<T: TokenParse<'a>>(self) -> Option<(T, Cursor<'a>)> {
        T::take(self)
    }

    #[inline]
    pub(super) fn token(self) -> Option<(TokenView<'a>, Cursor<'a>)> {
        if self.eof() {
            return None;
        }

        let entry = self.entry();
        let token_source = unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                self.source_base.as_ptr().add(entry.span.start as usize),
                (entry.span.end - entry.span.start) as usize,
            ))
        };
        // SAFETY: `as_token_kind` can only return `None` on `EndOfFile` but we ensured `!self.eof()` already.
        let kind = unsafe { entry.kind.as_token_kind().unwrap_unchecked() };

        Some((
            TokenView {
                span: entry.span,
                source: token_source,
                kind,
            },
            unsafe { self.next().unwrap_unchecked() },
        ))
    }

    pub fn group_end(mut self) -> Option<Cursor<'a>> {
        if self.eof() {
            return None;
        }

        let (EntryTokenKind::OpenParenthesis(group_start)
        | EntryTokenKind::OpenBracket(group_start)
        | EntryTokenKind::OpenBrace(group_start)
        | EntryTokenKind::Function(group_start)) = &self.entry().kind
        else {
            return None;
        };

        let end_offset = group_start.end_offset?.get() as usize;
        let new_entry = self.entry.wrapping_add(end_offset).min(self.end);
        debug_assert!(new_entry >= self.entry);
        self.entry = new_entry;

        Some(self)
    }

    #[inline]
    #[must_use]
    pub fn next(mut self) -> Option<Cursor<'a>> {
        if self.eof() {
            return None;
        }

        self.entry = unsafe { self.entry.add(1) };

        Some(self)
    }

    #[must_use]
    pub fn next_tree(mut self) -> Option<Cursor<'a>> {
        if let Some(group_end) = self.group_end() {
            self = group_end;
            debug_assert!(self.eof(), "group end points at end-of-file token")
        } else if matches!(
            self.entry().kind,
            EntryTokenKind::OpenParenthesis(_)
                | EntryTokenKind::OpenBracket(_)
                | EntryTokenKind::OpenBrace(_)
                | EntryTokenKind::Function(_)
        ) && !self.eof()
        {
            // This is an unclosed group, skip to the end.
            self.entry = self.end;
            return Some(self);
        }

        self.next()
    }

    #[inline]
    #[must_use]
    pub fn next_back(mut self) -> Option<Cursor<'a>> {
        if self.eof() {
            return None;
        }

        self.entry = unsafe { self.end.sub(1) };

        Some(self)
    }

    #[must_use]
    pub fn skip_back<T: Peek>(self, peek: T) -> Cursor<'a> {
        let last = self.next_back();
        if let Some(new_end) = last.filter(|x| x.is(peek)) {
            self.limited(new_end)
        } else {
            self
        }
    }

    #[inline]
    pub fn end(mut self) -> Cursor<'a> {
        self.entry = self.end;
        self
    }
}

impl<'a> Spanned for Cursor<'a> {
    fn span(&self) -> Span {
        self.entry().span
    }
}

impl std::fmt::Debug for Cursor<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Cursor ")?;
        let mut current = *self;
        f.debug_list()
            .entries(std::iter::from_fn(|| {
                if let Some(next) = current.next() {
                    let result = current.entry();
                    current = next;
                    Some((unsafe { self.span_source(result.span) }, result))
                } else {
                    None
                }
            }))
            .finish()
    }
}
