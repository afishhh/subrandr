use crate::csssyn::{
    buffer::{Cursor, TokenView},
    tokenizer::{Escaped, TokenKind},
    value::LookaheadPeek,
};

pub trait Peek: Sized {
    #[doc(hidden)]
    fn peek(&self, cursor: Cursor) -> bool;
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

#[doc(hidden)]
pub struct Zero;

impl Peek for Zero {
    fn peek(&self, cursor: Cursor) -> bool {
        matches!(
            cursor.token(),
            Some((
                TokenView {
                    span: _,
                    source: "0",
                    kind: TokenKind::Number { integer: true },
                },
                _
            ))
        )
    }
}

impl LookaheadPeek for Zero {
    fn name(&self) -> &'static str {
        "0"
    }
}

macro_rules! impl_peeks {
        ($($name: ident, $value: literal, $value_token: tt;)*) => {
            $(#[doc(hidden)]
            #[derive(Clone, Copy)]
            pub struct $name;

            impl Peek for $name {
                fn peek(&self, cursor: Cursor) -> bool {
                    matches!(cursor.token(), Some((
                        TokenView {
                            span: _,
                            source: _,
                            kind: TokenKind::Punct($value)
                        },
                        _
                    )))
                }
            }

            impl LookaheadPeek for $name {
                fn name(&self) -> &'static str {
                    stringify!($value_token)
                }
            })*

            macro_rules! TokenMacro {
                (0) => { $crate::csssyn::peek::Zero };
                $(($value_token) => { $crate::csssyn::peek::$name };)*
            }
            pub(crate) use TokenMacro as Token;
        };
    }

impl_peeks!(
    Comma, ',', ,;
    Colon, ':', :;
    Semicolon, ';', ;;
    ExclamationMark, '!', !;
    Slash, '/', /;
);

pub struct Whitespace;

impl Peek for Whitespace {
    fn peek(&self, cursor: Cursor) -> bool {
        cursor
            .token()
            .is_some_and(|(view, _)| matches!(view.kind, TokenKind::Whitespace))
    }
}

impl LookaheadPeek for Whitespace {
    fn name(&self) -> &'static str {
        "}"
    }
}

pub struct RightBrace;

impl Peek for RightBrace {
    fn peek(&self, cursor: Cursor) -> bool {
        cursor
            .token()
            .is_some_and(|(view, _)| matches!(view.kind, TokenKind::RBrace))
    }
}

impl LookaheadPeek for RightBrace {
    fn name(&self) -> &'static str {
        "}"
    }
}

pub struct End;

impl Peek for End {
    fn peek(&self, cursor: Cursor) -> bool {
        cursor.eof()
    }
}

impl LookaheadPeek for End {
    fn name(&self) -> &'static str {
        "<eof>"
    }
}
