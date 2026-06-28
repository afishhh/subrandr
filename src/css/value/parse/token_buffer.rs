use super::{
    Dimension, FunctionalNotation, Ident, Number, NumericTokenValue, ParseError, Percentage, Punct,
    Span, Spanned, StringLit, UnquotedUrl, ValueTokenTree,
};
use crate::css::tokenizer::{Escaped, TokenKind, Tokenizer};

// Really could be anything <2^32-1 so that `Span` can use 32-bit integers.
const SOURCE_LEN_LIMIT: usize = 1 << 20;

pub(super) struct TokenBuffer<'a> {
    source: &'a str,
    tokens: Vec<ValueTokenTree<'a>>,
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

        enum GroupEntry<'a> {
            Function(FunctionalNotation<'a>),
        }

        let mut toplevel = Vec::new();
        let mut group_stack = Vec::new();
        let mut last = 0;
        while let Some(token) = tokenizer.consume_token() {
            let span = Span {
                start: last,
                end: last + token.len,
            };
            let token_source = &source[span.start as usize..span.end as usize];
            let token = match token.kind {
                TokenKind::Whitespace => {
                    last = span.end;
                    continue;
                }
                TokenKind::Ident => {
                    ValueTokenTree::Ident(Ident::new(span, Escaped::new(token_source)))
                }
                TokenKind::String => ValueTokenTree::String(StringLit::new(
                    span,
                    Escaped::new(&token_source[1..token_source.len() - 1]),
                )),
                TokenKind::Function => {
                    group_stack.push(GroupEntry::Function(FunctionalNotation {
                        span,
                        function: Escaped::new(&token_source[..token_source.len() - 1]),
                        content: Vec::new(),
                    }));
                    continue;
                }
                TokenKind::RParen
                    if matches!(group_stack.last(), Some(GroupEntry::Function(_))) =>
                {
                    let Some(GroupEntry::Function(entry)) = group_stack.pop() else {
                        unreachable!()
                    };

                    ValueTokenTree::FunctionalNotation(entry)
                }
                TokenKind::Url {
                    value_offset,
                    trailing_len,
                } => ValueTokenTree::UnquotedUrl(UnquotedUrl {
                    span,
                    value: Escaped::new(
                        &token_source[usize::from(value_offset)
                            ..token_source.len() - usize::from(trailing_len)],
                    ),
                }),
                TokenKind::Number { integer } => ValueTokenTree::Number(Number {
                    span,
                    value: NumericTokenValue {
                        value: token_source,
                        integer,
                    },
                }),
                TokenKind::Percentage { integer } => ValueTokenTree::Percentage(Percentage {
                    span,
                    value: NumericTokenValue {
                        value: token_source,
                        integer,
                    },
                }),
                TokenKind::Dimension {
                    integer,
                    unit_offset,
                } => ValueTokenTree::Dimension(Dimension {
                    span,
                    text: token_source,
                    integer,
                    unit_offset,
                }),
                TokenKind::Comma => ValueTokenTree::Punct(Punct { span, value: ',' }),
                kind => {
                    return Err(ParseError::new(
                        span,
                        format_args!("`{}` not legal in values", kind.name()),
                    ))
                }
            };

            if let Some(group) = group_stack.last_mut() {
                match group {
                    GroupEntry::Function(functional) => {
                        functional.span.end = span.end;
                        functional.content.push(token);
                    }
                }
            } else {
                toplevel.push(token);
            }
        }

        let mut unclosed_groups = group_stack.iter();
        if let Some(first) = unclosed_groups.next() {
            let end_span = Span {
                start: source.len() as u32,
                end: source.len() as u32,
            };
            let error = |entry: &GroupEntry| match entry {
                GroupEntry::Function(_) => {
                    ParseError::new(end_span, "unclosed functional notation")
                }
            };

            let mut result = error(&first);
            for group in unclosed_groups {
                result.append(error(group));
            }
            return Err(result);
        }

        Ok(Self {
            source,
            tokens: toplevel,
        })
    }

    pub(super) fn tokens(&self) -> &[ValueTokenTree<'a>] {
        &self.tokens
    }

    pub(super) fn end_span(&self) -> Span {
        Span {
            start: self.source.len() as u32,
            end: self.source.len() as u32,
        }
    }

    pub(super) fn start(&self) -> Cursor {
        Cursor {
            buffer: self,
            index: todo!(),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Cursor<'a> {
    buffer: &'a TokenBuffer<'a>,
    index: usize,
}

impl<'a> Cursor<'a> {
    pub(super) fn tree(&self) -> Option<&'a ValueTokenTree<'a>> {
        self.buffer.tokens().get(self.index)
    }

    pub(super) fn eof(&self) -> bool {
        self.tree().is_none()
    }

    pub(super) fn next(&self) -> Cursor<'a> {
        Cursor {
            buffer: self.buffer,
            index: self.index + 1,
        }
    }
}

impl<'a> Spanned for Cursor<'a> {
    fn span(&self) -> Span {
        if let Some(tree) = self.tree() {
            tree.span()
        } else {
            self.buffer.end_span()
        }
    }
}
