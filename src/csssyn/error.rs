use super::{Span, Spanned};
use crate::csssyn::{
    buffer::{Cursor, Delimiter},
    tokenizer::{Escaped, TokenKind},
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
}

fn display_cursor_name(cursor: Cursor<'_>) -> impl std::fmt::Display + use<'_> {
    util::fmt_from_fn(move |f| {
        let Some((token, _)) = cursor.token() else {
            return f.write_str(
                cursor
                    .outer_delimiter()
                    .map_or("<eof>", Delimiter::closing_str),
            );
        };

        f.write_str(match token.kind {
            TokenKind::LParen => "(",
            TokenKind::LBracket => "[",
            TokenKind::LBrace => "{",
            TokenKind::Function => "<function>",
            TokenKind::Ident => {
                return std::fmt::Display::fmt(&Escaped::new(token.source), f);
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
            TokenKind::Punct(chr) => return std::fmt::Display::fmt(&chr, f),
            TokenKind::Cdc => "-->",
            TokenKind::Cdo => "<!--",
            TokenKind::Whitespace => "<whitespace>",
            TokenKind::BadString => "<bad-string>",
            TokenKind::BadUrl => "<bad-url>",
        })
    })
}

impl ParseError {
    pub(crate) fn unexpected(cursor: Cursor, expected: &[&'static str]) -> Self {
        let found = display_cursor_name(cursor);
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
                    write!(f, "expected one of `{first}`")?;
                    for &name in middle {
                        write!(f, ", `{name}`")?;
                    }
                    write!(f, ", or `{last}`, found `{found}`")
                }),
            ),
        }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = &self.messages[0];
        write!(f, "{}..{}: {}", msg.span.start, msg.span.end, msg.message)?;
        if let Some(remaining_messages) = self.messages.len().checked_sub(1).filter(|&x| x > 0) {
            write!(f, " (and {} others)", remaining_messages)?;
        }
        Ok(())
    }
}

impl std::error::Error for ParseError {}
