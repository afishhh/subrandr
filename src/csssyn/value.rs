mod error;
mod parse;
mod token_buffer;
mod token_tree;
pub use error::ParseError;
pub use parse::*;
use token_buffer::*;
pub use token_tree::*;

use crate::csssyn::Tokenizer;

// https://drafts.csswg.org/css-syntax-3/#consume-a-component-value
//
// `Cursor`'s group skipping effectively implements component values.
fn skip_a_component_value<'a>(cursor: Cursor<'a>) -> Cursor<'a> {
    cursor.next().unwrap_or(cursor)
}

// https://drafts.csswg.org/css-syntax-3/#consume-list-of-components
fn consume_a_list_of_component_values<'a>(
    mut cursor: Cursor<'a>,
    nested: bool,
    stop_token: impl LookaheadPeek<'a> + Copy,
) -> (Cursor<'a>, Cursor<'a>) {
    let start = cursor;
    loop {
        if cursor.eof() || cursor.is(stop_token) {
            return (start.limited(cursor), cursor);
        } else if cursor.is(RightBrace) && nested {
            return (start.limited(cursor), cursor);
        } else {
            cursor = skip_a_component_value(cursor);
        }
    }
}

// https://drafts.csswg.org/css-syntax-3/#consume-the-remnants-of-a-bad-declaration
fn consume_the_remnants_of_a_bad_declaration<'a>(
    mut cursor: Cursor<'a>,
    nested: bool,
) -> Cursor<'a> {
    loop {
        if let Some(next) = cursor.skip(Token![;]).or(cursor.skip(End)) {
            return next;
        } else if let Some(next) = cursor.skip(RightBrace) {
            if nested {
                return cursor;
            } else {
                cursor = next;
            }
        } else {
            return skip_a_component_value(cursor);
        }
    }
}

#[derive(Debug, Clone)]
pub struct Declaration<'a> {
    pub name: Ident<'a>,
    pub value: Cursor<'a>,
    pub important: Option<Ident<'a>>,
}

fn consume_a_declaration<'a>(
    mut cursor: Cursor<'a>,
    nested: bool,
) -> (Option<Declaration<'a>>, Cursor<'a>) {
    // If the next token is an <ident-token>, consume a token from input and set decl's name to the token’s value.
    let Some((name, next)) = cursor.ident() else {
        // Otherwise, consume the remnants of a bad declaration from input, with nested, and return nothing.
        return (
            None,
            consume_the_remnants_of_a_bad_declaration(cursor, nested),
        );
    };
    cursor = next;

    // Discard whitespace from input.
    cursor = cursor.skip_whitespace();

    // If the next token is a <colon-token>, discard a token from input.
    let Some(next) = cursor.skip(Token![:]) else {
        // Otherwise, consume the remnants of a bad declaration from input, with nested, and return nothing.
        return (
            None,
            consume_the_remnants_of_a_bad_declaration(cursor, nested),
        );
    };
    cursor = next;

    // Discard whitespace from input.
    cursor = cursor.skip_whitespace();

    // Consume a list of component values from input, with nested, and with <semicolon-token> as the stop token, and set decl’s value to the result.
    let (mut value, next) = consume_a_list_of_component_values(cursor, nested, Token![;]);
    cursor = next;

    let mut important = None;
    // If the last two non-<whitespace-token>s in decl’s value are a <delim-token> with the value "!" followed by an <ident-token> with a value that is an ASCII case-insensitive match for "important", remove them from decl’s value and set decl’s important flag.
    if let Some((ident, new_value)) = value.take_important_from_end() {
        important = Some(ident);
        value = new_value;
    }

    // > While the last item in decl’s value is a <whitespace-token>, remove that token.
    // Parsing of declaration content implicitly ignores whitespace so the above is not necessary.
    // In fact, it would be problematic with how `Cursor::skip_whitespace` works.

    // TODO: Otherwise, if decl’s value contains a top-level simple block with an associated token of <{-token>, and also contains any other non-<whitespace-token> value, return nothing. (That is, a top-level {}-block is only allowed as the entire value of a non-custom property.)

    // TODO: Otherwise, if decl’s name is an ASCII case-insensitive match for "unicode-range", consume the value of a unicode-range descriptor from the segment of the original source text string corresponding to the tokens returned by the consume a list of component values call, and replace decl’s value with the result.

    (
        Some(Declaration {
            name,
            value,
            important,
        }),
        cursor,
    )
}

// https://drafts.csswg.org/css-syntax-3/#consume-block-contents but qualified rules are illegal.
pub fn parse_declaration_list<'a>(mut cursor: Cursor<'a>) -> impl Iterator<Item = Declaration<'a>> {
    std::iter::from_fn(move || loop {
        if cursor.eof() {
            return None;
        }

        cursor = cursor.skip_whitespace();

        if let Some(next) = cursor.skip(Token![;]) {
            cursor = next;
            continue;
        }

        // TODO: at-rule

        let decl;
        (decl, cursor) = consume_a_declaration(cursor, false);

        match decl {
            Some(decl) => return Some(decl),
            None => continue,
        }
    })
}

#[test]
fn abcd() {
    let buffer =
        TokenBuffer::from_tokenizer(Tokenizer::new("hello: world !important ; w: a")).unwrap();
    panic!(
        "{:?}",
        parse_declaration_list(buffer.start()).collect::<Vec<_>>()
    );
}
