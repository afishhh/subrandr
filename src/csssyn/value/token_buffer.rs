use std::{fmt::Display, marker::PhantomData, num::NonZero, ptr::NonNull};

use super::{
    Dimension, FunctionalNotation, Ident, LitString, Number, NumericTokenValue, ParseError,
    Percentage, Punct, Span, Spanned, TokenTree, UnquotedUrl,
};
use crate::csssyn::{
    tokenizer::{Escaped, HashTypeFlag, TokenKind, Tokenizer},
    value::{Parse, ParseStream, Peek, Token},
};

// Really could be anything <2^32-1 so that `Span` can use 32-bit integers.
const SOURCE_LEN_LIMIT: usize = 1 << 20;

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
    EndOfFile,

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
}

impl EntryTokenKind {
    fn as_group_start(&self) -> Option<(Delimiter, &GroupStart)> {
        Some(match self {
            EntryTokenKind::OpenParenthesis(group_start)
            | EntryTokenKind::Function(group_start) => (Delimiter::Parenthesis, group_start),
            EntryTokenKind::OpenBracket(group_start) => (Delimiter::Bracket, group_start),
            EntryTokenKind::OpenBrace(group_start) => (Delimiter::Brace, group_start),
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
struct GroupStart {
    /// Offset of the entry for the closing delimiter of this group.
    /// `None` if the group was left unclosed.
    end_offset: Option<NonZero<u32>>,
}

pub(super) struct TokenBuffer<'a> {
    source: &'a str,
    entries: Vec<Entry>,
}

impl<'a> TokenBuffer<'a> {
    pub(super) fn from_tokenizer(mut tokenizer: Tokenizer<'a>) -> Result<Self, ParseError> {
        let source = tokenizer.source();
        if source.len() > SOURCE_LEN_LIMIT {
            return Err(ParseError::new(
                Span { start: 0, end: 0 },
                format_args!("source longer than {SOURCE_LEN_LIMIT} bytes is unsupported"),
            ));
        }

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
                start: source.len() as u32,
                end: source.len() as u32,
            },
            kind: EntryTokenKind::EndOfFile,
        });

        dbg!(&entries);
        // TODO:
        // let mut unclosed_groups = group_stack.iter();
        // if let Some(first) = unclosed_groups.next() {
        //     let end_span = Span {
        //         start: source.len() as u32,
        //         end: source.len() as u32,
        //     };
        //     let error = |entry: &GroupEntry| match entry {
        //         GroupEntry::Function(_) => {
        //             ParseError::new(end_span, "unclosed functional notation")
        //         }
        //     };

        //     let mut result = error(&first);
        //     for group in unclosed_groups {
        //         result.append(error(group));
        //     }
        //     return Err(result);
        // }

        Ok(Self { source, entries })
    }

    pub(super) fn end_span(&self) -> Span {
        Span {
            start: self.source.len() as u32,
            end: self.source.len() as u32,
        }
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

    unsafe fn group_cursors(self, group_start: &GroupStart) -> Option<(Cursor<'a>, Cursor<'a>)> {
        let end_offset = group_start.end_offset?;
        let end_ptr = self.entry.wrapping_add(end_offset.get() as usize);
        // This could happen if `self` has been `limited` to an entry inside this group.
        if end_ptr >= self.end {
            return None;
        }

        let inner_cursor = Cursor {
            source_base: self.source_base,
            entry: self.entry.wrapping_add(1),
            end: end_ptr,
            phantom: PhantomData,
        };
        let outer_cursor = Cursor {
            source_base: self.source_base,
            entry: self.entry.wrapping_add(end_offset.get() as usize + 1),
            end: self.end,
            phantom: PhantomData,
        };
        Some((inner_cursor, outer_cursor))
    }

    pub fn is_whitespace(self) -> bool {
        matches!(self.entry().kind, EntryTokenKind::Whitespace)
    }

    pub fn skip_whitespace(mut self) -> Cursor<'a> {
        if matches!(self.entry().kind, EntryTokenKind::Whitespace) {
            self.entry = unsafe { self.entry.add(1) };
        }

        self
    }

    pub fn limited(mut self, other: Cursor<'a>) -> Cursor<'a> {
        assert!(other.entry <= self.end);
        // We assume that `Whitespace` cannot be an end entry above.
        assert!(!matches!(other.entry().kind, EntryTokenKind::Whitespace));

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

    pub fn is<T: Peek>(self, peek: T) -> bool {
        peek.peek(self)
    }

    pub fn skip<T: Peek>(mut self, peek: T) -> Option<Cursor<'a>> {
        self.is(peek).then(|| {
            // TODO: make this part of `<T::peek>`
            if !self.eof() {
                self.entry = unsafe { self.entry.add(1) };
            }
            self
        })
    }

    pub fn token_tree(self) -> Option<(TokenTree<'a>, Cursor<'a>)> {
        let entry = self.entry();
        let span = entry.span;
        let entry_source = |leading: usize, trailing: usize| unsafe {
            let start = entry.span.start as usize + leading;
            let len = entry.span.end as usize - start - trailing;
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                self.source_base.as_ptr().add(start),
                len,
            ))
        };

        let mut next = Cursor {
            entry: self.entry.wrapping_add(1),
            ..self
        };
        let tree = match &entry.kind {
            EntryTokenKind::OpenParenthesis(_)
            | EntryTokenKind::OpenBracket(_)
            | EntryTokenKind::OpenBrace(_)
            | EntryTokenKind::CloseParenthesis
            | EntryTokenKind::CloseBracket
            | EntryTokenKind::CloseBrace
            | EntryTokenKind::EndOfFile
            | EntryTokenKind::Cdc
            | EntryTokenKind::Cdo
            | EntryTokenKind::Whitespace
            | EntryTokenKind::AtKeyword
            | EntryTokenKind::Hash { .. }
            | EntryTokenKind::BadUrl
            | EntryTokenKind::BadString => return None,
            EntryTokenKind::Function(group_start) => {
                let inner;
                (inner, next) = unsafe { self.group_cursors(group_start) }?;
                let end = unsafe { (*inner.end).span.end };

                TokenTree::FunctionalNotation(FunctionalNotation {
                    span: Span {
                        start: span.start,
                        end,
                    },
                    function: Escaped::new(entry_source(0, 1)),
                    content: inner,
                })
            }
            &EntryTokenKind::Punct(value) => TokenTree::Punct(Punct { span, value }),
            EntryTokenKind::Ident => TokenTree::Ident(Ident {
                span,
                value: Escaped::new(entry_source(0, 0)),
            }),
            EntryTokenKind::String => TokenTree::String(LitString {
                span,
                value: Escaped::new(entry_source(1, 1)),
            }),
            &EntryTokenKind::Url {
                value_offset,
                trailing_len,
            } => TokenTree::UnquotedUrl(UnquotedUrl {
                span,
                value: Escaped::new(entry_source(
                    usize::from(value_offset),
                    usize::from(trailing_len),
                )),
            }),
            &EntryTokenKind::Number { integer } => TokenTree::Number(Number {
                span,
                value: NumericTokenValue {
                    value: entry_source(0, 0),
                    integer,
                },
            }),
            &EntryTokenKind::Percentage { integer } => TokenTree::Percentage(Percentage {
                span,
                value: NumericTokenValue {
                    value: entry_source(0, 1),
                    integer,
                },
            }),
            &EntryTokenKind::Dimension {
                integer,
                unit_offset,
            } => TokenTree::Dimension(Dimension {
                span,
                text: entry_source(0, 0),
                integer,
                unit_offset,
            }),
        };

        Some((tree, next.skip_whitespace()))
    }

    pub fn ident(self) -> Option<(Ident<'a>, Cursor<'a>)> {
        self.token_tree().and_then(|(tt, next)| match tt {
            TokenTree::Ident(ident) => Some((ident, next)),
            _ => None,
        })
    }

    pub fn right_brace(self) -> Option<(Punct, Cursor<'a>)> {
        let entry = self.entry();
        let EntryTokenKind::Punct(chr) = entry.kind else {
            return None;
        };

        Some((
            Punct {
                span: entry.span,
                value: chr,
            },
            self.next()?,
        ))
    }

    pub fn next(mut self) -> Option<Cursor<'a>> {
        if self.eof() {
            return None;
        }

        match self.entry().kind.as_group_start() {
            Some((_, group_start)) => {
                if let Some(end_offset) = group_start.end_offset {
                    self.entry = unsafe { self.entry.add(end_offset.get() as usize) }.min(self.end);
                } else {
                    self.entry = self.end;
                }
            }
            None => self.entry = unsafe { self.entry.add(1) },
        }

        Some(self)
    }

    pub fn take_important_from_end(mut self) -> Option<(Ident<'a>, Cursor<'a>)> {
        if unsafe { self.end.offset_from_unsigned(self.entry) } < 2 {
            return None;
        }

        let last_entry = unsafe { &*self.end.sub(1) };
        let second_to_last_entry = unsafe { &*self.end.sub(2) };
        if !matches!(second_to_last_entry.kind, EntryTokenKind::Punct('!'))
            || !matches!(last_entry.kind, EntryTokenKind::Ident)
        {
            return None;
        }

        let last_value = Escaped::new(unsafe { self.span_source(last_entry.span) });
        if !last_value.eq_ignore_ascii_case("important") {
            return None;
        }

        self.end = unsafe { self.end.sub(2) };
        return Some((
            Ident {
                span: last_entry.span,
                value: last_value,
            },
            self,
        ));
    }
}

impl<'a> Spanned for Cursor<'a> {
    fn span(&self) -> Span {
        self.entry().span
    }
}

impl Display for Cursor<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let entry = self.entry();
        f.write_str(match entry.kind {
            EntryTokenKind::OpenParenthesis(_) => "(",
            EntryTokenKind::OpenBracket(_) => "[",
            EntryTokenKind::OpenBrace(_) => "{",
            EntryTokenKind::Function(_)
            | EntryTokenKind::Ident
            | EntryTokenKind::AtKeyword
            | EntryTokenKind::Hash { .. }
            | EntryTokenKind::String
            | EntryTokenKind::Url { .. }
            | EntryTokenKind::Number { .. }
            | EntryTokenKind::Percentage { .. }
            | EntryTokenKind::Dimension { .. } => {
                return Display::fmt(&Escaped::new(unsafe { self.span_source(entry.span) }), f);
            }
            EntryTokenKind::CloseParenthesis => ")",
            EntryTokenKind::CloseBracket => "]",
            EntryTokenKind::CloseBrace => "}",
            EntryTokenKind::EndOfFile => "<eof>",
            EntryTokenKind::Punct(chr) => return Display::fmt(&chr, f),
            EntryTokenKind::Cdc => "-->",
            EntryTokenKind::Cdo => "<!--",
            EntryTokenKind::Whitespace => unsafe { self.span_source(entry.span) },
            EntryTokenKind::BadString => "<bad-string>",
            EntryTokenKind::BadUrl => "<bad-url>",
        })
    }
}
