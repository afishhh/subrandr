//! Implementation of select parser algorithms from https://drafts.csswg.org/css-syntax-3/.

use crate::csssyn::{
    buffer::Cursor,
    token::{End, Ident, LeftBrace, RightBrace, Token, Whitespace},
    Peek,
};

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

    // If decl’s name is a custom property name string, then set decl’s original text to the segment of the original source text string corresponding to the tokens of decl’s value.
    // NOTE: We ignore the part about saving the original source text because our token
    //       representation already allows us to get original source text from any Cursor.
    let is_custom_property = name.value().unescape_iter().take(2).eq(['-', '-']);
    // Otherwise, if decl’s value contains a top-level simple block with an associated token of <{-token>, and also contains any other non-<whitespace-token> value, return nothing. (That is, a top-level {}-block is only allowed as the entire value of a non-custom property.)
    if !is_custom_property {
        let mut current = value;
        let mut contains_top_level_block = false;
        let mut contains_other_token = false;
        while let Some(next) = current.next_tree() {
            if current.is(LeftBrace) {
                contains_top_level_block = true;
            } else {
                contains_other_token = true;
            }

            if contains_top_level_block && contains_other_token {
                return (None, cursor);
            }

            current = next;
        }
    }

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

        // TODO: at-rules are currently not supported but not parsing them should be
        //       forwards compatible

        let decl;
        (decl, cursor) = consume_a_declaration(cursor, false);

        match decl {
            Some(decl) => return Some(decl),
            None => continue,
        }
    })
}

pub enum BlockItem<'a> {
    Rule(Rule<'a>),
    Declaration(Declaration<'a>),
}

pub enum Rule<'a> {
    Qualified(QualifiedRule<'a>),
}

pub struct QualifiedRule<'a> {
    pub prelude: Cursor<'a>,
    pub content: BlockContent<'a>,
}

// https://drafts.csswg.org/css-syntax-3/#consume-block-contents but this is ran on a limited cursor.
// And returns items one-by-one instead of in chunks.
fn parse_a_blocks_contents<'a>(mut cursor: Cursor<'a>) -> impl Iterator<Item = BlockItem<'a>> {
    std::iter::from_fn(move || loop {
        if cursor.eof() {
            return None;
        }

        cursor = cursor.skip(Whitespace);

        if let Some(next) = cursor.next_if(Token![;]) {
            cursor = next;
            continue;
        }

        // Consume a declaration from input, with nested set to true.
        let (decl, next) = consume_a_declaration(cursor, true);

        // If a declaration was returned, append it to decls, and discard a mark from input.
        if let Some(decl) = decl {
            cursor = next;
            return Some(BlockItem::Declaration(decl));
        }

        // Otherwise, restore a mark from input, then consume a qualified rule from input, with nested set to true, and <semicolon-token> as the stop token.
        let rule;
        (rule, cursor) = consume_a_qualified_rule(cursor, true, true);

        if let Some(rule) = rule {
            return Some(BlockItem::Rule(Rule::Qualified(rule)));
        }
    })
}

pub struct BlockContent<'a>(Cursor<'a>);

impl<'a> BlockContent<'a> {
    pub fn parse(&self) -> impl Iterator<Item = BlockItem<'a>> {
        parse_a_blocks_contents(self.0)
    }
}

// https://drafts.csswg.org/css-syntax-3/#consume-a-block
// This deviates a bit from the spec by consuming the whole group at once to
// make things simpler but should result in the same behavior.
fn consume_a_block<'a>(mut cursor: Cursor<'a>) -> (BlockContent<'a>, Cursor<'a>) {
    // Assert: The next token is a <{-token>.
    let content_start = cursor
        .next_if(LeftBrace)
        .expect("The next token is a <{-token>.");
    let block_end = cursor.group_end().unwrap();

    // Consume a block’s contents from input and let rules be the result.
    let rules = BlockContent(content_start.limited(block_end));
    cursor = block_end;

    // Discard a token from input.
    cursor = cursor.next().unwrap_or(cursor);

    (rules, cursor)
}

// https://drafts.csswg.org/css-syntax-3/#consume-a-qualified-rule
fn consume_a_qualified_rule<'a>(
    mut cursor: Cursor<'a>,
    nested: bool,
    stop_on_semicolon: bool,
) -> (Option<QualifiedRule<'a>>, Cursor<'a>) {
    let prelude_start = cursor;
    loop {
        if cursor.eof() || (stop_on_semicolon && cursor.is(Token![;])) {
            return (None, cursor);
        }

        if let Some(next) = cursor.next_if(RightBrace) {
            if nested {
                return (None, cursor);
            } else {
                cursor = next;
            }
        } else if let Some(next) = cursor.next_if(LeftBrace) {
            let prelude = prelude_start.limited(cursor);
            // If the first two non-<whitespace-token> values of rule’s prelude are an <ident-token> whose value starts with "--" followed by a <colon-token>, then:
            if prelude
                .skip(Whitespace)
                .take::<Ident>()
                .is_some_and(|(ident, next)| {
                    ident.value().starts_with("--") && next.skip(Whitespace).is(Token![:])
                })
            {
                if nested {
                    // If nested is true, consume the remnants of a bad declaration from input, with nested set to true, and return nothing.
                    cursor = consume_the_remnants_of_a_bad_declaration(cursor, true);
                    return (None, cursor);
                } else {
                    // If nested is false, consume a block from input, and return nothing.
                    (_, cursor) = consume_a_block(next);
                    return (None, cursor);
                }
            } else {
                let content;
                (content, cursor) = consume_a_block(cursor);

                return (Some(QualifiedRule { prelude, content }), cursor);
            }
        } else {
            // Consume a component value from input and append the result to rule’s prelude.
            cursor = skip_a_component_value(cursor);
        }
    }
}

// https://drafts.csswg.org/css-syntax-3/#consume-a-stylesheets-contents
pub fn consume_a_stylesheets_contents<'a>(
    mut cursor: Cursor<'a>,
) -> impl Iterator<Item = Rule<'a>> {
    std::iter::from_fn(move || loop {
        if let Some(next) = cursor
            .next_if(Whitespace)
            .or(cursor.next_if(Token![<!--]))
            .or(cursor.next_if(Token![-->]))
        {
            cursor = next;
        } else if cursor.eof() {
            return None;
        } else {
            let (rule, next) = consume_a_qualified_rule(cursor, false, false);
            cursor = next;

            if let Some(rule) = rule {
                return Some(Rule::Qualified(rule));
            }
        }
    })
}

#[cfg(test)]
mod test {
    use crate::csssyn::TokenBuffer;

    fn check_declaration_list_parse(source: &str, expected: &[(&str, &str, bool)]) {
        let buffer = TokenBuffer::from_source(source).unwrap();

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
                "font-style: italic ;\n",
            ),
            &[
                ("font-family", "'Ahem'", false),
                ("font-size", "20pt", true),
                ("font-style", "italic", false),
            ],
        );

        check_declaration_list_parse(
            concat!(
                "font-family: {};\n",
                "font-size: {} b;\n",
                "font-style: {} !important;\n",
                "color: a {} ;\n",
                "--custom: a {} b;\n",
            ),
            &[
                ("font-family", "{}", false),
                ("font-style", "{}", true),
                ("--custom", "a {} b", false),
            ],
        );
    }
}
