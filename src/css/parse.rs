mod token_tree;
mod tokenizer;

pub use token_tree::*;
pub use tokenizer::Span;

pub struct TokenStream<'a> {
    tokens: Vec<TokenTree<'a>>,
}

pub struct TokenError {
    span: Span,
    kind: TokenErrorKind,
}

enum TokenErrorKind {
    UnclosedDelimiter,
}

impl<'a> TokenStream<'a> {
    pub fn try_from_str(
        &self,
        source: &str,
        allow_unclosed_groups: bool,
    ) -> Result<TokenTree<'a>, TokenError> {
        let mut tokenizer = tokenizer::Tokenizer::new(source);
        while let Some(token) = tokenizer.next() {
            match token.kind {
                tokenizer::TokenKind::Comma => TokenTree::Punct(Punct {
                    span_start: token.span.start,
                    character: ',',
                }),
                tokenizer::TokenKind::Cdc => TokenTree::Cdc(Cdc {
                    span_start: token.span.start,
                }),
                tokenizer::TokenKind::Cdo => TokenTree::Cdo(Cdo {
                    span_start: token.span.start,
                }),
                tokenizer::TokenKind::Colon => TokenTree::Punct(Punct {
                    span_start: token.span.start,
                    character: ':',
                }),
                tokenizer::TokenKind::Semicolon => TokenTree::Punct(Punct {
                    span_start: token.span.start,
                    character: ';',
                }),
                tokenizer::TokenKind::Whitespace => todo!(),
                tokenizer::TokenKind::LParen => todo!(),
                tokenizer::TokenKind::RParen => todo!(),
                tokenizer::TokenKind::LBracket => todo!(),
                tokenizer::TokenKind::RBracket => todo!(),
                tokenizer::TokenKind::LBrace => todo!(),
                tokenizer::TokenKind::RBrace => todo!(),
                tokenizer::TokenKind::Ident(escaped) => todo!(),
                tokenizer::TokenKind::Function(escaped) => todo!(),
                tokenizer::TokenKind::AtKeyword(escaped) => todo!(),
                tokenizer::TokenKind::Hash { value, type_flag } => todo!(),
                tokenizer::TokenKind::String(escaped) => todo!(),
                tokenizer::TokenKind::BadString => todo!(),
                tokenizer::TokenKind::Url(escaped) => todo!(),
                tokenizer::TokenKind::BadUrl => todo!(),
                tokenizer::TokenKind::Delim(_) => todo!(),
                tokenizer::TokenKind::Number { integer } => todo!(),
                tokenizer::TokenKind::Percentage { value } => todo!(),
                tokenizer::TokenKind::Dimension {
                    value,
                    integer,
                    unit,
                } => todo!(),
            }
        }
    }
}
