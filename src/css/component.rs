use std::{ops::Range, rc::Rc};

use super::{
    parse::{
        parse_whole, parse_whole_with,
        tokenizer::{InputStream, Token, TokenKind},
        ParseStream,
    },
    properties::AnyPropertyValue,
    selector::CompoundSelectorList,
};

/// Implements parsing algorithms defined in <https://drafts.csswg.org/css-syntax-3/#parsing>.
pub struct TokenParser<'a> {
    input: InputStream<'a>,
    reconsumed: Option<Token<'a>>,
    temporary_buffer: Vec<ComponentValue<'a>>,
}

enum RuleParseResult<R> {
    Rule(R),
    Nothing,
    InvalidRuleError,
}

// TODO: imagine if we could use ParseStream here :(
impl<'a> TokenParser<'a> {
    pub fn new(text: &'a str) -> Self {
        Self {
            input: InputStream::new(text),
            reconsumed: None,
            temporary_buffer: Vec::new(),
        }
    }

    // PERF: I don't think forking tokenizers is a good idea since CSS tokenization seems
    //       pretty expensive. Maybe instead we should truly have a lookback list when marked
    //       content exists.
    fn fork(&self) -> Self {
        Self {
            input: self.input.fork(),
            reconsumed: self.reconsumed.clone(),
            temporary_buffer: Vec::new(),
        }
    }

    fn take_component_buffer(&mut self, start: usize) -> ComponentStream<'a> {
        assert!(start <= self.temporary_buffer.len());

        let mut out =
            Rc::<[ComponentValue<'a>]>::new_uninit_slice(self.temporary_buffer.len() - start);

        unsafe {
            std::ptr::copy_nonoverlapping(
                self.temporary_buffer.as_ptr().add(start),
                Rc::get_mut(&mut out).unwrap_unchecked().as_mut_ptr().cast(),
                self.temporary_buffer.len() - start,
            );
            self.temporary_buffer.set_len(start);

            ComponentStream {
                range: 0..out.len(),
                components: Rc::<[_]>::assume_init(out),
            }
        }
    }

    fn reconsume(&mut self, token: Token<'a>) {
        self.reconsumed = Some(token);
    }

    fn consume_token(&mut self) -> Option<Token<'a>> {
        if let Some(token) = self.reconsumed.take() {
            return Some(token);
        }

        self.input.consume_token()
    }

    fn consume_simple_block(&mut self, block_token: BlockToken) -> Block<'a> {
        let start = self.temporary_buffer.len();

        loop {
            match self.consume_token() {
                Some(token) if block_token.is_ending_token(&token.kind) => break,
                Some(token) => {
                    self.reconsume(token);
                    let component = self.consume_component_value();
                    self.temporary_buffer.push(component);
                }
                None => break,
            }
        }

        Block {
            associated_token: block_token,
            value: self.take_component_buffer(start),
        }
    }

    // https://drafts.csswg.org/css-syntax/#consume-a-block
    fn consume_a_block(&mut self) -> BlockContents<'a> {
        self.consume_a_blocks_contents()
    }

    fn consume_function(&mut self, name: Box<str>) -> Function<'a> {
        let start = self.temporary_buffer.len();

        loop {
            match self.consume_token() {
                Some(token) => match token.kind {
                    TokenKind::RParen => break,
                    _ => {
                        self.reconsume(token);
                        let component = self.consume_component_value();
                        self.temporary_buffer.push(component);
                    }
                },
                None => break,
            }
        }

        Function {
            name,
            value: self.take_component_buffer(start),
        }
    }

    fn consume_component_value(&mut self) -> ComponentValue<'a> {
        match self.try_consume_component_value() {
            Some(value) => value,
            None => unreachable!("consume_component_value called on empty parser"),
        }
    }

    fn try_consume_component_value(&mut self) -> Option<ComponentValue<'a>> {
        match self.consume_token() {
            Some(token) => Some(match token.kind {
                TokenKind::LParen => {
                    ComponentValue::Block(self.consume_simple_block(BlockToken::Paren))
                }
                TokenKind::LBracket => {
                    ComponentValue::Block(self.consume_simple_block(BlockToken::Bracket))
                }
                TokenKind::LBrace => {
                    ComponentValue::Block(self.consume_simple_block(BlockToken::Brace))
                }
                TokenKind::Function(name) => ComponentValue::Function(self.consume_function(name)),
                _ => ComponentValue::PreservedToken(token),
            }),
            None => None,
        }
    }

    pub fn parse_component_stream(&mut self) -> ComponentStream<'a> {
        while let Some(value) = self.try_consume_component_value() {
            self.temporary_buffer.push(value);
        }

        self.take_component_buffer(0)
    }

    fn consume_qualified_rule(
        &mut self,
        nested: bool,
        stop_token: Option<TokenKind>,
    ) -> RuleParseResult<StyleRule> {
        let start = self.temporary_buffer.len();

        loop {
            match self.consume_token() {
                Some(token) => match token.kind {
                    TokenKind::LBrace => {
                        let mut it = self.temporary_buffer[start..].iter().filter(|x| {
                            !matches!(
                                x,
                                ComponentValue::PreservedToken(Token {
                                    kind: TokenKind::Whitespace,
                                    ..
                                })
                            )
                        });

                        if matches!(it.next(), Some(ComponentValue::PreservedToken(Token { kind:TokenKind::Ident(value), .. })) if value.starts_with("--"))
                            && matches!(
                                it.next(),
                                Some(ComponentValue::PreservedToken(Token {
                                    kind: TokenKind::Colon,
                                    ..
                                }))
                            )
                        {
                            if nested {
                                self.consume_remnants_of_a_bad_declaration(true);
                                return RuleParseResult::Nothing;
                            } else {
                                self.temporary_buffer.truncate(start);
                                self.consume_a_block();
                                return RuleParseResult::Nothing;
                            }
                        }

                        let prelude = self.take_component_buffer(start);
                        let qual = {
                            let mut block = self.consume_a_block();
                            QualifiedRule {
                                declarations: {
                                    if matches!(
                                        block.0.first(),
                                        Some(RuleOrListOfDeclarations::Declarations(..))
                                    ) {
                                        match block.0.remove(0) {
                                            RuleOrListOfDeclarations::Rule(..) => {
                                                unreachable!()
                                            }
                                            RuleOrListOfDeclarations::Declarations(decls) => decls,
                                        }
                                    } else {
                                        Vec::new()
                                    }
                                },
                                child_rules: block
                                    .0
                                    .into_iter()
                                    .map(|x| match x {
                                        RuleOrListOfDeclarations::Rule(rule) => rule,
                                        RuleOrListOfDeclarations::Declarations(decls) => {
                                            Rule::NestedDeclarations(decls)
                                        }
                                    })
                                    .collect(),
                            }
                        };

                        return RuleParseResult::Rule(StyleRule {
                            selector: match parse_whole(ParseStream::new(prelude)) {
                                Ok(selector) => selector,
                                Err(..) => return RuleParseResult::InvalidRuleError,
                            },
                            properties: {
                                if !qual.child_rules.is_empty() {
                                    // TODO: nesting not supported :)
                                    return RuleParseResult::InvalidRuleError;
                                }

                                qual.declarations
                            },
                        });
                    }
                    TokenKind::RParen if nested => {
                        return RuleParseResult::Nothing;
                    }
                    _ => {
                        let stop = stop_token.as_ref().is_some_and(|s| &token.kind == s);
                        self.reconsume(token);
                        if stop {
                            return RuleParseResult::Nothing;
                        }
                        let component = self.consume_component_value();
                        self.temporary_buffer.push(component);
                    }
                },
                None => return RuleParseResult::Nothing,
            }
        }
    }

    fn consume_at_rule(&mut self, name: Box<str>, nested: bool) -> Option<AtRule<'a>> {
        let start = self.temporary_buffer.len();
        let prelude;
        let mut block = None;

        loop {
            match self.consume_token() {
                Some(token) => match token.kind {
                    TokenKind::Semicolon => {
                        prelude = self.take_component_buffer(start);
                        break;
                    }
                    TokenKind::RBrace if nested => {
                        prelude = self.take_component_buffer(start);
                        break;
                    }
                    TokenKind::LBrace => {
                        prelude = self.take_component_buffer(start);
                        block = Some(self.consume_a_block());
                        break;
                    }
                    _ => {
                        self.reconsume(token);
                        let component = self.consume_component_value();
                        self.temporary_buffer.push(component);
                    }
                },
                None => {
                    prelude = self.take_component_buffer(start);
                    break;
                }
            }
        }

        Some(AtRule {
            prelude,
            name,
            block,
        })
    }

    // https://drafts.csswg.org/css-syntax-3/#consume-a-stylesheets-contents
    fn consume_a_stylesheets_contents(&mut self) -> Vec<Rule<'a>> {
        let mut rules = Vec::new();

        loop {
            match self.consume_token() {
                Some(token) => match token.kind {
                    TokenKind::Whitespace => (),
                    TokenKind::Cdo | TokenKind::Cdc => {}
                    TokenKind::AtKeyword(name) => {
                        if let Some(rule) = self.consume_at_rule(name, false) {
                            rules.push(Rule::AtRule(rule));
                        }
                    }
                    _ => {
                        self.reconsume(token);
                        if let RuleParseResult::Rule(rule) =
                            self.consume_qualified_rule(false, None)
                        {
                            rules.push(Rule::Style(rule));
                        }
                    }
                },
                None => break,
            }
        }

        rules
    }

    pub fn consume_a_blocks_contents(&mut self) -> BlockContents<'a> {
        let mut rules = Vec::new();
        let mut decls = Vec::new();

        loop {
            match self.consume_token() {
                Some(Token {
                    kind: TokenKind::Whitespace | TokenKind::Semicolon,
                    ..
                }) => (),
                None
                | Some(Token {
                    kind: TokenKind::RBrace,
                    ..
                }) => {
                    // FIXME: the spec forgets to specify this?
                    if !decls.is_empty() {
                        rules.push(RuleOrListOfDeclarations::Declarations(std::mem::take(
                            &mut decls,
                        )))
                    }

                    break;
                }
                Some(Token {
                    kind: TokenKind::AtKeyword(name),
                    ..
                }) => {
                    if !decls.is_empty() {
                        rules.push(RuleOrListOfDeclarations::Declarations(std::mem::take(
                            &mut decls,
                        )))
                    }

                    if let Some(rule) = self.consume_at_rule(name, true) {
                        rules.push(RuleOrListOfDeclarations::Rule(Rule::AtRule(rule)))
                    }
                }
                v => {
                    if let Some(token) = v {
                        self.reconsume(token);
                    }

                    let mut fork = self.fork();
                    if let Some(declaration) = fork.consume_a_declaration(true) {
                        decls.push(declaration);
                        *self = fork;
                    } else {
                        match self.consume_qualified_rule(true, Some(TokenKind::Semicolon)) {
                            RuleParseResult::Nothing => (),
                            RuleParseResult::InvalidRuleError => {
                                if !decls.is_empty() {
                                    rules.push(RuleOrListOfDeclarations::Declarations(
                                        std::mem::take(&mut decls),
                                    ))
                                }
                            }
                            RuleParseResult::Rule(rule) => {
                                if !decls.is_empty() {
                                    rules.push(RuleOrListOfDeclarations::Declarations(
                                        std::mem::take(&mut decls),
                                    ))
                                }
                                rules.push(RuleOrListOfDeclarations::Rule(Rule::Style(rule)))
                            }
                        }
                    }
                }
            }
        }

        BlockContents(rules)
    }

    // https://drafts.csswg.org/css-syntax/#consume-a-declaration
    fn consume_a_declaration(&mut self, nested: bool) -> Option<PropertyDeclaration> {
        let mut name = match self.consume_token() {
            Some(Token {
                kind: TokenKind::Ident(ident),
                ..
            }) => ident,
            Some(token) => {
                self.reconsume(token);
                self.consume_remnants_of_a_bad_declaration(nested);
                return None;
            }
            _ => {
                self.consume_remnants_of_a_bad_declaration(nested);
                return None;
            }
        };

        loop {
            match self.consume_token() {
                Some(Token {
                    kind: TokenKind::Whitespace,
                    ..
                }) => continue,
                Some(Token {
                    kind: TokenKind::Colon,
                    ..
                }) => break,
                Some(token) => {
                    self.reconsume(token);
                    self.consume_remnants_of_a_bad_declaration(nested);
                    return None;
                }
                _ => {
                    self.consume_remnants_of_a_bad_declaration(nested);
                    return None;
                }
            }
        }

        loop {
            match self.consume_token() {
                Some(Token {
                    kind: TokenKind::Whitespace,
                    ..
                }) => continue,
                Some(token) => {
                    self.reconsume(token);
                    break;
                }
                None => break,
            }
        }

        let mut important = false;

        let start = self.temporary_buffer.len();
        self.consume_a_list_of_component_values_into_temporary_buffer(nested, TokenKind::Semicolon);
        let value_tokens = &self.temporary_buffer[start..];
        if value_tokens.len() >= 2
            && matches!(
                value_tokens[value_tokens.len() - 2],
                ComponentValue::PreservedToken(Token {
                    kind: TokenKind::Delim('!'),
                    ..
                })
            )
            && matches!(&value_tokens[value_tokens.len() - 1], ComponentValue::PreservedToken(Token { kind: TokenKind::Ident(value), .. }) if value.eq_ignore_ascii_case("important"))
        {
            self.temporary_buffer
                .truncate(self.temporary_buffer.len() - 2);
            important = true;
        }

        while matches!(
            self.temporary_buffer[start..].last(),
            Some(ComponentValue::PreservedToken(Token {
                kind: TokenKind::Whitespace,
                ..
            }))
        ) {
            self.temporary_buffer.pop();
        }

        let value = self.take_component_buffer(start);

        if name.starts_with("--") {
            // TODO: If decl’s name is a custom property name string, then set decl’s original text to the segment of the original source text string corresponding to the tokens of decl’s value.
            // ^^^ preserve span information in tokenizer instead of representation...
            return None;
        } else if value.components.len() != 1
            && value.components.iter().any(|value| {
                matches!(
                    value,
                    ComponentValue::Block(Block {
                        associated_token: BlockToken::Brace,
                        ..
                    })
                )
            })
        {
            return None;
        } else {
            name.make_ascii_lowercase();

            if &*name == "unicode-range" {
                // TODO: Otherwise, if decl’s name is an ASCII case-insensitive match for "unicode-range", consume the value of a unicode-range descriptor from the segment of the original source text string corresponding to the tokens returned by the consume a list of component values call, and replace decl’s value with the result.
                return None;
            }
        }

        match super::properties::property_to_parser(&*name) {
            Some(parser) => Some(PropertyDeclaration {
                value: parse_whole_with(ParseStream::new(value), parser).ok()?,
                important,
            }),
            None => None,
        }
    }

    // https://drafts.csswg.org/css-syntax/#consume-the-remnants-of-a-bad-declaration
    fn consume_remnants_of_a_bad_declaration(&mut self, nested: bool) {
        loop {
            match self.consume_token() {
                None
                | Some(Token {
                    kind: TokenKind::Semicolon,
                    ..
                }) => return,
                Some(
                    token @ Token {
                        kind: TokenKind::RBrace,
                        ..
                    },
                ) => {
                    if nested {
                        self.reconsume(token);
                        return;
                    }
                }
                Some(token) => {
                    self.reconsume(token);
                    self.consume_component_value();
                }
            }
        }
    }

    fn consume_a_list_of_component_values_into_temporary_buffer(
        &mut self,
        nested: bool,
        stop_token: TokenKind,
    ) {
        loop {
            match self.consume_token() {
                None => break,
                Some(token) if token.kind == stop_token => {
                    self.reconsume(token);
                    break;
                }
                Some(
                    token @ Token {
                        kind: TokenKind::RBrace,
                        ..
                    },
                ) => {
                    if nested {
                        self.reconsume(token);
                        return;
                    } else {
                        self.temporary_buffer
                            .push(ComponentValue::PreservedToken(token));
                    }
                }
                Some(token) => {
                    self.reconsume(token);
                    let component = self.consume_component_value();
                    self.temporary_buffer.push(component);
                }
            }
        }
    }

    pub fn parse_a_stylesheet(mut self) -> Vec<Rule<'a>> {
        self.consume_a_stylesheets_contents()
    }
}

#[derive(Debug, Clone)]
pub enum RuleOrListOfDeclarations<'a> {
    Rule(Rule<'a>),
    Declarations(Vec<PropertyDeclaration>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockToken {
    Paren,
    Bracket,
    Brace,
}

impl BlockToken {
    fn is_ending_token(&self, token: &TokenKind) -> bool {
        matches!(
            (self, token),
            (Self::Paren, TokenKind::RParen)
                | (Self::Bracket, TokenKind::RBracket)
                | (Self::Brace, TokenKind::RBrace)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block<'a> {
    pub associated_token: BlockToken,
    pub value: ComponentStream<'a>,
}

#[derive(Debug, Clone)]
pub struct BlockContents<'a>(pub Vec<RuleOrListOfDeclarations<'a>>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyDeclaration {
    pub value: AnyPropertyValue,
    pub important: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function<'a> {
    pub name: Box<str>,
    pub value: ComponentStream<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentValue<'a> {
    PreservedToken(Token<'a>),
    Function(Function<'a>),
    Block(Block<'a>),
}

#[derive(Debug, Clone)]
pub struct QualifiedRule<'a> {
    pub declarations: Vec<PropertyDeclaration>,
    pub child_rules: Vec<Rule<'a>>,
}

#[derive(Debug, Clone)]
pub struct StyleRule {
    pub selector: CompoundSelectorList,
    pub properties: Vec<PropertyDeclaration>,
    // TODO: nesting not supported :)
}

#[derive(Debug, Clone)]
pub struct AtRule<'a> {
    pub prelude: ComponentStream<'a>,
    pub name: Box<str>,
    // truly do not care about at-rules for now, just pass the block
    pub block: Option<BlockContents<'a>>,
}

#[derive(Debug, Clone)]
pub enum Rule<'a> {
    Style(StyleRule),
    AtRule(AtRule<'a>),
    // https://drafts.csswg.org/css-nesting-1/#nested-declarations-rule
    NestedDeclarations(Vec<PropertyDeclaration>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentStream<'a> {
    components: Rc<[ComponentValue<'a>]>,
    range: Range<usize>,
}

impl<'a> ComponentStream<'a> {
    pub fn empty() -> Self {
        Self {
            components: Rc::new([]),
            range: 0..0,
        }
    }

    pub(super) fn new(components: Rc<[ComponentValue<'a>]>) -> Self {
        Self {
            range: 0..components.len(),
            components,
        }
    }

    pub(super) fn components(&self) -> &[ComponentValue<'a>] {
        &self.components[self.range.clone()]
    }

    pub(super) fn substream(&self, range: Range<usize>) -> Self {
        let new_range = self.range.start + range.start..self.range.start + range.end;
        assert!(new_range.end <= self.range.end);
        assert!(new_range.start <= self.range.end);
        Self {
            components: self.components.clone(),
            range: new_range,
        }
    }

    pub fn len(&self) -> usize {
        self.range.end - self.range.start
    }
}

#[cfg(test)]
mod test {
    use super::TokenParser;

    #[test]
    fn does_not_crash() {
        dbg!(TokenParser::new(
            r#"
::cue(:lang(en-US, brazil\!\!\!)) {
    /* color: blue !important; */
    ruby-position: alternate over;
    ruby-position: alternate;
    ruby-position: under alternate;
    ruby-position: under !important;
    ruby-position: inter-character;
    font-style: normal;
    font-style: italic;
    font-style: oblique;
}
"#,
        )
        .parse_a_stylesheet());
    }
}
