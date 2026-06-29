use log::{warn, LogContext};

use super::selectors::{self, ComplexSelectorUnit};
use crate::csssyn::{
    self,
    algorithms::{Declaration, QualifiedRule, Rule},
    buffer::{BufferTokenizationError, Cursor},
    token::Whitespace,
    Spanned, TokenBuffer,
};

#[derive(Debug)]
pub(super) struct StyleRule<'a> {
    selector: ComplexSelectorUnit,
    declarations: Vec<Declaration<'a>>,
}

impl<'a> StyleRule<'a> {
    fn parse(log: &LogContext, rule: QualifiedRule<'a>) -> Option<Self> {
        let selector = match selectors::parse_complex_selector_unit(rule.prelude) {
            Ok(s) => s,
            Err(err) => {
                warn!(
                    log,
                    "Failed to parse selector `{}`: {err}",
                    rule.prelude
                        .skip(Whitespace)
                        .skip_back(Whitespace)
                        .scope_source()
                );
                return None;
            }
        };

        let mut declarations = Vec::new();
        for item in rule.content.parse() {
            match item {
                csssyn::algorithms::BlockItem::Rule(Rule::Qualified(nested)) => {
                    warn!(
                        log,
                        "Encountered unsupported nested qualified rule at byte {}",
                        nested.prelude.span().start,
                    );
                }
                csssyn::algorithms::BlockItem::Declaration(declaration) => {
                    declarations.push(declaration);
                }
            }
        }

        Some(Self {
            selector,
            declarations,
        })
    }

    pub(super) fn selector(&self) -> &ComplexSelectorUnit {
        &self.selector
    }

    pub(super) fn declarations(&self) -> &[Declaration<'a>] {
        &self.declarations
    }
}

impl<'a> StylesheetInner<'a> {
    fn parse(log: &LogContext, cursor: Cursor<'a>) -> Self {
        let mut result = Self { rules: Vec::new() };

        for Rule::Qualified(rule) in csssyn::algorithms::consume_a_stylesheets_contents(cursor) {
            result.rules.extend(StyleRule::parse(log, rule));
        }

        result
    }
}

#[derive(Debug)]
struct StylesheetInner<'a> {
    rules: Vec<StyleRule<'a>>,
}

// self-referential
pub(super) struct Stylesheet {
    inner: StylesheetInner<'static>,
    _tokens: TokenBuffer<'static>,
    _source: Box<str>,
}

impl std::fmt::Debug for Stylesheet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.inner, f)
    }
}

impl Stylesheet {
    pub(super) fn parse(
        log: &LogContext,
        source: Box<str>,
    ) -> Result<Self, BufferTokenizationError> {
        let tokens = TokenBuffer::from_source(&source)?;

        Ok(Stylesheet {
            inner: unsafe { std::mem::transmute(StylesheetInner::parse(log, tokens.start())) },
            _tokens: unsafe { std::mem::transmute(tokens) },
            _source: source,
        })
    }

    pub(super) fn rules(&self) -> &[StyleRule<'_>] {
        &self.inner.rules
    }
}
