mod error;
mod parse;
mod token_buffer;
mod token_tree;
pub use error::ParseError;
pub use parse::*;
use token_buffer::*;
pub use token_tree::*;

// https://drafts.csswg.org/css-syntax-3/#consume-a-component-value
fn skip_a_component_value<'a>(cursor: Cursor<'a>) -> Cursor<'a> {
    // `Cursor::next_tree`'s group skipping effectively implements component values.
    cursor.next_tree().unwrap_or(cursor)
}

// https://drafts.csswg.org/css-syntax-3/#consume-list-of-components
fn consume_a_list_of_component_values<'a>(
    mut cursor: Cursor<'a>,
    nested: bool,
    stop_token: impl Peek + Copy,
) -> (Cursor<'a>, Cursor<'a>) {
    let start = cursor;
    loop {
        if cursor.eof() || cursor.is(stop_token) {
            // Return values.
            return (start.limited(cursor), cursor);
        } else if cursor.is(RightBrace) {
            // If nested is true, return values.
            if nested {
                return (start.limited(cursor), cursor);
            } else {
                // Otherwise, this is a parse error. Consume a token from input and append the result to values.
                cursor = skip_a_component_value(cursor);
            }
        } else {
            // Consume a component value from input, and append the result to values.
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
        if let Some(next) = cursor.next_if(Token![;]).or(cursor.next_if(End)) {
            return next;
        } else if let Some(next) = cursor.next_if(RightBrace) {
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
    pub important: bool,
}

fn consume_a_declaration<'a>(
    mut cursor: Cursor<'a>,
    nested: bool,
) -> (Option<Declaration<'a>>, Cursor<'a>) {
    // If the next token is an <ident-token>, consume a token from input and set decl's name to the token’s value.
    let Some((name, next)) = cursor.take::<Ident>() else {
        // Otherwise, consume the remnants of a bad declaration from input, with nested, and return nothing.
        return (
            None,
            consume_the_remnants_of_a_bad_declaration(cursor, nested),
        );
    };
    cursor = next;

    // Discard whitespace from input.
    cursor = cursor.skip(Whitespace);

    // If the next token is a <colon-token>, discard a token from input.
    let Some(next) = cursor.next_if(Token![:]) else {
        // Otherwise, consume the remnants of a bad declaration from input, with nested, and return nothing.
        return (
            None,
            consume_the_remnants_of_a_bad_declaration(cursor, nested),
        );
    };
    cursor = next;

    // Discard whitespace from input.
    cursor = cursor.skip(Whitespace);

    // Consume a list of component values from input, with nested, and with <semicolon-token> as the stop token, and set decl’s value to the result.
    let (mut value, next) = consume_a_list_of_component_values(cursor, nested, Token![;]);
    cursor = next;

    // If the last two non-<whitespace-token>s in decl’s value are a <delim-token> with the value "!" followed by an <ident-token> with a value that is an ASCII case-insensitive match for "important", remove them from decl’s value and set decl’s important flag.
    value = value.skip_back(Whitespace);
    let important = 'important: {
        let last = value.next_back();
        let Some(important) = last.filter(|x| x.is("important")) else {
            break 'important false;
        };

        let second_to_last = value.limited(important).next_back();
        let Some(exclamation_mark) = second_to_last.filter(|x| x.is(Token![!])) else {
            break 'important false;
        };

        value = value.limited(exclamation_mark);
        true
    };

    // While the last item in decl’s value is a <whitespace-token>, remove that token.
    value = value.skip_back(Whitespace);

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

        cursor = cursor.skip(Whitespace);

        if let Some(next) = cursor.next_if(Token![;]) {
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

#[cfg(test)]
mod test {
    use crate::csssyn::{value::TokenBuffer, Tokenizer};

    fn check_declaration_list_parse(source: &str, expected: &[(&str, &str, bool)]) {
        let buffer = TokenBuffer::from_tokenizer(Tokenizer::new(source)).unwrap();

        let left = super::parse_declaration_list(buffer.start())
            .map(|decl| {
                (
                    decl.name.value().to_string(),
                    decl.value.scope_source(),
                    decl.important,
                )
            })
            .collect::<Vec<_>>();
        let left_str = left
            .iter()
            .map(|&(ref a, b, c)| (a.as_str(), b, c))
            .collect::<Vec<_>>();

        assert_eq!(left_str, expected);
    }

    #[test]
    fn declaration_list() {
        check_declaration_list_parse(
            "hello: world !important ; w: a",
            &[("hello", "world", true), ("w", "a", false)],
        );

        check_declaration_list_parse(
            concat!(
                "font-family: 'Ahem';\n",
                "font-size: 20pt!important;\n",
                "some junk ;\n",
                "font-style: italic ;\n"
            ),
            &[
                ("font-family", "'Ahem'", false),
                ("font-size", "20pt", true),
                ("font-style", "italic", false),
            ],
        );
    }
}
