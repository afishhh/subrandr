use crate::css::{
    tokenizer::{LineColumn, SourceMap, Span, Spanned},
    value::{Cursor, ValueTokenTree},
};

#[derive(Debug)]
pub struct ParseError {
    messages: Vec<ErrorMessage>,
}

#[derive(Debug)]
struct ErrorMessage {
    span: Span,
    message: String,
}

impl ParseError {
    pub fn new(span: impl Spanned, message: impl std::fmt::Display) -> Self {
        Self {
            messages: vec![ErrorMessage {
                span: span.span(),
                message: message.to_string(),
            }],
        }
    }

    pub(super) fn unexpected(cursor: Cursor, expected: &[&'static str]) -> Self {
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

#[derive(Debug)]
pub struct ResolvedParseError {
    messages: Vec<ResolvedErrorMessage>,
}

#[derive(Debug)]
struct ResolvedErrorMessage {
    linecol: LineColumn,
    message: String,
}

impl ParseError {
    pub fn resolve(self, source_map: &SourceMap) -> ResolvedParseError {
        ResolvedParseError {
            messages: self
                .messages
                .into_iter()
                .map(|msg| ResolvedErrorMessage {
                    linecol: source_map.byte_line_column(msg.span.start),
                    message: msg.message,
                })
                .collect(),
        }
    }
}

impl std::fmt::Display for ResolvedParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = &self.messages[0];
        write!(
            f,
            "<unnamed>:{}:{}: {}",
            msg.linecol.line + 1,
            msg.linecol.column + 1,
            msg.message
        )
    }
}

impl std::error::Error for ResolvedParseError {}
