use std::convert::Infallible;

use crate::csssyn::{
    buffer::Cursor,
    parse_stream::LookaheadPeek,
    token::Token,
    tokenizer::{Escaped, TokenKind},
};

pub trait Peek: Sized {
    #[doc(hidden)]
    fn peek(&self, cursor: Cursor) -> bool;
}

impl<F: FnOnce(Infallible) -> T, T: Token> Peek for F {
    fn peek(&self, cursor: Cursor) -> bool {
        T::peek(cursor)
    }
}

impl Peek for &'static str {
    fn peek(&self, cursor: Cursor) -> bool {
        cursor.token().is_some_and(|(token, _)| {
            matches!(token.kind, TokenKind::Ident | TokenKind::Function)
                && Escaped::new(token.source).eq_ignore_ascii_case(self)
        })
    }
}

impl LookaheadPeek for &'static str {
    fn name(&self) -> &'static str {
        self
    }
}
