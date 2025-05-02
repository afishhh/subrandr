use std::rc::Rc;

use super::parse::tokenizer::{InputStream, Token, TokenKind};

pub struct TokenParser<'a> {
    input: InputStream<'a>,
    reconsumed: Option<Token<'a>>,
    temporary_buffer: Vec<ComponentValue<'a>>,
}

impl<'a> TokenParser<'a> {
    pub fn new(text: &'a str) -> Self {
        Self {
            input: InputStream::new(text),
            reconsumed: None,
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

    fn consume_qualified_rule(&mut self) -> Option<Rule<'a>> {
        let start = self.temporary_buffer.len();

        loop {
            match self.consume_token() {
                Some(token) => match token.kind {
                    TokenKind::LBrace => {
                        return Some(Rule {
                            prelude: self.take_component_buffer(start),
                            kind: RuleKind::Qualified(self.consume_simple_block(BlockToken::Brace)),
                        })
                    }
                    _ => {
                        self.reconsume(token);
                        let component = self.consume_component_value();
                        self.temporary_buffer.push(component);
                    }
                },
                None => return None,
            }
        }
    }

    fn consume_at_rule(&mut self, name: Box<str>) -> Rule<'a> {
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
                    TokenKind::LBrace => {
                        prelude = self.take_component_buffer(start);
                        block = Some(self.consume_simple_block(BlockToken::Brace));
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
    pub block: Block<'a>,
}

#[derive(Debug, Clone)]
pub struct AtRule<'a> {
    pub name: Box<str>,
    pub block: Option<Block<'a>>,
}

#[derive(Debug, Clone)]
pub enum RuleKind<'a> {
    Qualified(Block<'a>),
    AtRule(AtRule<'a>),
}

#[derive(Debug, Clone)]
pub struct Rule<'a> {
    pub prelude: ComponentStream<'a>,
    pub kind: RuleKind<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentStream<'a> {
    pub(super) components: Rc<[ComponentValue<'a>]>,
}
