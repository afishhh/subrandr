use super::{
    is_whitespace,
    tokenizer::{HashTypeFlag, InputStream, Token, TokenKind},
};

pub struct TokenParser<'a> {
    input: InputStream<'a>,
    reconsumed: Option<Token<'a>>,
}

impl<'a> TokenParser<'a> {
    pub fn new(text: &'a str) -> Self {
        Self {
            input: InputStream::new(text),
            reconsumed: None,
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
        let mut result = Block {
            associated_token: block_token,
            value: Vec::new(),
        };

        loop {
            match self.consume_token() {
                Some(token) if block_token.is_ending_token(&token.kind) => break,
                Some(token) => {
                    self.reconsume(token);
                    result.value.push(self.consume_component_value());
                }
                None => break,
            }
        }

        result
    }

    fn consume_function(&mut self, name: Box<str>) -> Function<'a> {
        let mut value = Vec::new();

        loop {
            match self.consume_token() {
                Some(token) => match token.kind {
                    TokenKind::RParen => break,
                    _ => {
                        self.reconsume(token);
                        value.push(self.consume_component_value());
                    }
                },
                None => break,
            }
        }

        Function { name, value }
    }

    fn consume_component_value(&mut self) -> ComponentValue<'a> {
        match self.consume_token() {
            Some(token) => match token.kind {
                TokenKind::LParen => {
                    return ComponentValue::Block(self.consume_simple_block(BlockToken::Paren))
                }
                TokenKind::LBracket => {
                    return ComponentValue::Block(self.consume_simple_block(BlockToken::Bracket))
                }
                TokenKind::LBrace => {
                    return ComponentValue::Block(self.consume_simple_block(BlockToken::Brace))
                }
                TokenKind::Function(name) => {
                    return ComponentValue::Function(self.consume_function(name))
                }
                _ => return ComponentValue::PreservedToken(token),
            },
            None => unreachable!("consume_component_value called on empty parser"),
        }
    }

    fn consume_qualified_rule(&mut self) -> Option<Rule<'a>> {
        let mut prelude = Vec::new();

        loop {
            match self.consume_token() {
                Some(token) => match token.kind {
                    TokenKind::LBrace => {
                        return Some(Rule {
                            prelude,
                            kind: RuleKind::Qualified(self.consume_simple_block(BlockToken::Brace)),
                        })
                    }
                    _ => {
                        self.reconsume(token);
                        prelude.push(self.consume_component_value());
                    }
                },
                None => return None,
            }
        }
    }

    fn consume_at_rule(&mut self, name: Box<str>) -> Rule<'a> {
        let mut prelude = Vec::new();
        let mut block = None;

        loop {
            match self.consume_token() {
                Some(token) => match token.kind {
                    TokenKind::Semicolon => break,
                    TokenKind::LBrace => {
                        block = Some(self.consume_simple_block(BlockToken::Brace));
                        break;
                    }
                    _ => {
                        self.reconsume(token);
                        prelude.push(self.consume_component_value());
                    }
                },
                None => break,
            }
        }

        Rule {
            prelude,
            kind: RuleKind::AtRule(AtRule { name, block }),
        }
    }

    fn consume_list_of_rules(&mut self, top_level: bool, output: &mut Vec<Rule<'a>>) {
        match self.consume_token() {
            Some(token) => match token.kind {
                TokenKind::Whitespace => (),
                TokenKind::Cdo | TokenKind::Cdc => {
                    if !top_level {
                        self.reconsume(token);
                        if let Some(rule) = self.consume_qualified_rule() {
                            output.push(rule);
                        }
                    }
                }
                TokenKind::AtKeyword(name) => {
                    output.push(self.consume_at_rule(name));
                }
                _ => {
                    self.reconsume(token);
                    if let Some(rule) = self.consume_qualified_rule() {
                        output.push(rule);
                    }
                }
            },
            None => return,
        }
    }

    pub fn parse_stylesheet(mut self) -> Vec<Rule<'a>> {
        let mut result = Vec::new();
        self.consume_list_of_rules(true, &mut result);
        result
    }
}

#[derive(Debug, Clone)]
pub struct QualifiedRule<'a> {
    block: Block<'a>,
}

#[derive(Debug, Clone)]
pub struct AtRule<'a> {
    name: Box<str>,
    block: Option<Block<'a>>,
}

#[derive(Debug, Clone)]
pub enum RuleKind<'a> {
    Qualified(Block<'a>),
    AtRule(AtRule<'a>),
}

#[derive(Debug, Clone)]
pub struct Rule<'a> {
    prelude: Vec<ComponentValue<'a>>,
    kind: RuleKind<'a>,
}

#[derive(Debug, Clone, Copy)]
pub enum BlockToken {
    Paren,
    Bracket,
    Brace,
}

impl BlockToken {
    fn is_ending_token(&self, token: &TokenKind) -> bool {
        match (self, token) {
            (Self::Paren, TokenKind::RParen) => true,
            (Self::Bracket, TokenKind::RBracket) => true,
            (Self::Brace, TokenKind::RBrace) => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Block<'a> {
    associated_token: BlockToken,
    value: Vec<ComponentValue<'a>>,
}

#[derive(Debug, Clone)]
pub struct Function<'a> {
    name: Box<str>,
    value: Vec<ComponentValue<'a>>,
}

#[derive(Debug, Clone)]
pub enum ComponentValue<'a> {
    PreservedToken(Token<'a>),
    Function(Function<'a>),
    Block(Block<'a>),
}

pub struct ComponentParser<'a> {
    input: std::vec::IntoIter<ComponentValue<'a>>,
    reconsumed: Option<ComponentValue<'a>>,
}

impl<'a> ComponentParser<'a> {
    pub fn new(values: std::vec::IntoIter<ComponentValue<'a>>) -> Self {
        Self {
            input: values,
            reconsumed: None,
        }
    }

    fn reconsume(&mut self, value: ComponentValue<'a>) {
        self.reconsumed = Some(value);
    }

    fn consume_token(&mut self) -> Option<ComponentValue<'a>> {
        if let Some(value) = self.reconsumed.take() {
            return Some(value);
        }

        self.input.next()
    }
}

#[derive(Debug, Clone)]
pub struct SelectorList(Vec<CompoundSelector>);

#[derive(Debug, Clone)]
pub struct CompoundSelector {
    type_selector: Option<TypeSelector>,
    subclass_selectors: Vec<SubclassSelector>,
    pseudo_element: Option<PseudoElementSelector>,
}

#[derive(Debug, Clone)]
pub enum SubclassSelector {
    Id(IdSelector),
    Class(ClassSelector),
    // Attribute(AttributeSelector),
    PseudoClass(PseudoClassSelector),
}

#[derive(Debug, Clone, Copy)]
pub enum Combinator {
    Descendant,
    Child,
    NextSibling,
    SubsequentSibling,
}

#[derive(Debug, Clone)]
pub struct TypeSelector(Box<str>);

#[derive(Debug, Clone)]
pub struct IdSelector(Box<str>);

#[derive(Debug, Clone)]
pub struct ClassSelector(Box<str>);

#[derive(Debug, Clone)]
pub struct AttributeSelector {
    attribute: Box<str>,
    value: AttributeSelectorOperator,
}

#[derive(Debug, Clone)]
pub enum AttributeSelectorOperator {
    Exists,
    Equal(Box<str>),
    ListContains(Box<str>),
    EqualOrStartsWithDash(Box<str>),
    StartsWith(Box<str>),
    EndsWith(Box<str>),
    Contains(Box<str>),
}

impl AttributeSelectorOperator {
    pub fn match_value(&self, value: &str) -> bool {
        match self {
            AttributeSelectorOperator::Exists => true,
            AttributeSelectorOperator::Equal(other) => &**other == value,
            AttributeSelectorOperator::ListContains(other) => {
                value.split(is_whitespace).any(|value| &**other == value)
            }
            AttributeSelectorOperator::EqualOrStartsWithDash(other) => {
                if value.starts_with(&**other) {
                    let byte = value.as_bytes().get(other.len());
                    matches!(byte, Some(b'-') | None)
                } else {
                    false
                }
            }
            AttributeSelectorOperator::StartsWith(other) => value.starts_with(&**other),
            AttributeSelectorOperator::EndsWith(other) => value.ends_with(&**other),
            AttributeSelectorOperator::Contains(other) => value.contains(&**other),
        }
    }
}

#[derive(Debug, Clone)]
pub enum PseudoClassSelector {
    Past,
    Future,
    Lang(Box<str>),
}

#[derive(Debug, Clone)]
pub struct PseudoElementSelector(Box<str>);

fn starts_subclass_selector(token: &TokenKind) -> bool {
    match token {
        TokenKind::Hash(hash) => hash.type_flag == HashTypeFlag::Id,
        TokenKind::Colon => true,
        TokenKind::LBracket => true,
        TokenKind::Delim('.') => true,
        _ => false,
    }
}

impl ComponentParser<'_> {
    fn consume_subclass_selector(&mut self) -> Option<SubclassSelector> {
        match self.consume_token()? {
            ComponentValue::PreservedToken(token) => match token.kind {
                TokenKind::LBracket => todo!(),
                _ => todo!(),
            },
            _ => return None,
        }
    }

    fn consume_compound_selector(&mut self) -> Option<CompoundSelector> {
        let _result = CompoundSelector {
            type_selector: None,
            subclass_selectors: Vec::new(),
            pseudo_element: None,
        };

        todo!()
    }

    pub fn parse_selector_list(self) {
        todo!()
    }
}
