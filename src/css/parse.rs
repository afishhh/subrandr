use tokenizer::{HashToken, HashTypeFlag, Token, TokenKind};

pub mod tokenizer;

// TODO: Implement parse errers
#[derive(Debug)]
pub struct ParseError {}

#[derive(Debug, Clone)]
pub struct ParseStream<'a> {
    stream: ComponentStream<'a>,
    position: usize,
}

impl<'a> ParseStream<'a> {
    pub fn new(tokens: ComponentStream<'a>) -> Self {
        Self {
            stream: tokens,
            position: 0,
        }
    }

    fn next_token(&mut self) -> Option<&ComponentValue<'a>> {
        if let Some(component) = self.stream.components.get(self.position) {
            self.position += 1;
            Some(component)
        } else {
            None
        }
    }

    fn peek_token(&mut self) -> Option<&ComponentValue<'a>> {
        self.stream.components.get(self.position)
    }

    pub fn parse<T: Parse<'a>>(&mut self) -> Result<T, ParseError> {
        T::parse(self)
    }

    pub fn skip_whitespace(&mut self) -> bool {
        if self.peek::<Whitespace>() {
            self.advance_by(1);
            true
        } else {
            false
        }
    }

    pub fn peek<T: AtomicParse<'a>>(&self) -> bool {
        Self::peek_n::<0, T>(self).is_some()
    }

    pub fn peek2<T: AtomicParse<'a>>(&self) -> bool {
        Self::peek_n::<1, T>(self).is_some()
    }

    pub fn peek3<T: AtomicParse<'a>>(&self) -> bool {
        Self::peek_n::<2, T>(self).is_some()
    }

    fn advance_by(&mut self, count: usize) {
        self.position += count;
    }

    fn peek_n<const OFF: usize, T: AtomicParse<'a>>(&self) -> Option<T> {
        T::matches(self.stream.components.get(self.position + OFF))
    }

    pub fn lookahead1(&self) -> Lookahead1<'a, '_> {
        Lookahead1 { stream: self }
    }

    pub fn is_empty(&self) -> bool {
        self.stream.components.len() == self.position
    }

    pub fn fork(&self) -> Self {
        Self {
            stream: self.stream.clone(),
            position: self.position,
        }
    }
}

pub struct Lookahead1<'a, 'p> {
    stream: &'p ParseStream<'a>,
}

impl<'a, 'p> Lookahead1<'a, 'p> {
    pub fn peek<T: AtomicParse<'a>>(&mut self) -> bool {
        self.stream.peek::<T>()
    }

    pub fn error(self) -> ParseError {
        ParseError {}
    }
}

pub trait Parse<'a>: Sized {
    fn parse(stream: &mut ParseStream<'a>) -> Result<Self, ParseError>;
}

pub fn parse_whole_with<'a, R>(
    mut stream: ParseStream<'a>,
    parser: impl FnOnce(&mut ParseStream<'a>) -> Result<R, ParseError>,
) -> Result<R, ParseError> {
    let result = parser(&mut stream)?;
    if stream.is_empty() {
        Ok(result)
    } else {
        Err(ParseError {})
    }
}

pub fn parse_whole<'a, T: Parse<'a>>(stream: ParseStream<'a>) -> Result<T, ParseError> {
    parse_whole_with(stream, T::parse)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Punctuated<T, P>(pub Vec<(T, Option<P>)>);

impl<'a, T: Parse<'a>, P: AtomicParse<'a>> Punctuated<T, P> {
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    pub fn push(&mut self, value: T) {
        assert!(self.0.last().is_none_or(|x| x.1.is_some()));
        self.0.push((value, None));
    }

    pub fn push_punct(&mut self, value: P) {
        assert!(self.0.last().is_some_and(|x| x.1.is_none()));
        self.0.last_mut().unwrap().1 = Some(value);
    }

    pub fn parse_separated_skip_whitespace(
        stream: &mut ParseStream<'a>,
    ) -> Result<Self, ParseError> {
        let mut result = Self(Vec::new());

        loop {
            if stream.is_empty() {
                break;
            }

            result.push(stream.parse()?);

            stream.skip_whitespace();
            if stream.peek::<P>() {
                result.push_punct(stream.parse()?);
                stream.skip_whitespace();
            } else {
                break;
            }
        }

        Ok(result)
    }
}

pub trait AtomicParse<'a>: Sized {
    fn matches(token: Option<&ComponentValue<'a>>) -> Option<Self>;
}

impl<'a, T: AtomicParse<'a>> Parse<'a> for T {
    fn parse(stream: &mut ParseStream<'a>) -> Result<Self, ParseError> {
        if let Some(parsed) = T::matches(stream.peek_token()) {
            stream.advance_by(1);
            Ok(parsed)
        } else {
            Err(ParseError {})
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdHash {
    pub value: Box<str>,
}

impl IdHash {
    #[cfg(test)]
    pub fn new_unspanned(value: impl Into<Box<str>>) -> Self {
        Self {
            value: value.into(),
        }
    }
}

impl AtomicParse<'_> for IdHash {
    fn matches(token: Option<&ComponentValue>) -> Option<Self> {
        match token {
            Some(ComponentValue::PreservedToken(Token {
                kind:
                    TokenKind::Hash(HashToken {
                        value,
                        type_flag: HashTypeFlag::Id,
                    }),
                representation: _,
            })) => Some(Self {
                value: value.clone(),
            }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident {
    pub value: Box<str>,
}

impl Ident {
    #[cfg(test)]
    pub fn new_unspanned(value: impl Into<Box<str>>) -> Self {
        Self {
            value: value.into(),
        }
    }
}

impl AtomicParse<'_> for Ident {
    fn matches(token: Option<&ComponentValue>) -> Option<Self> {
        match token {
            Some(ComponentValue::PreservedToken(Token {
                kind: TokenKind::Ident(value),
                representation: _,
            })) => Some(Self {
                value: value.clone(),
            }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringLiteral {
    pub value: Box<str>,
}

impl StringLiteral {
    #[cfg(test)]
    pub fn new_unspanned(value: impl Into<Box<str>>) -> Self {
        Self {
            value: value.into(),
        }
    }
}

impl AtomicParse<'_> for StringLiteral {
    fn matches(token: Option<&ComponentValue>) -> Option<Self> {
        match token {
            Some(ComponentValue::PreservedToken(Token {
                kind: TokenKind::String(value),
                representation: _,
            })) => Some(Self {
                value: value.clone(),
            }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Whitespace;

impl AtomicParse<'_> for Whitespace {
    fn matches(token: Option<&ComponentValue<'_>>) -> Option<Self> {
        match token {
            Some(ComponentValue::PreservedToken(Token {
                kind: TokenKind::Whitespace,
                representation: _,
            })) => Some(Self),
            _ => None,
        }
    }
}

pub trait BlockLikeToken<'a> {
    fn parse_content(&self) -> ParseStream<'a>;
}

#[doc(hidden)]
pub mod tokens {
    use super::super::component::*;
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Colon {}

    impl Colon {
        #[cfg(test)]
        pub fn new_unspanned() -> Self {
            Self {}
        }
    }

    impl AtomicParse<'_> for Colon {
        fn matches(token: Option<&ComponentValue>) -> Option<Self> {
            match token {
                Some(ComponentValue::PreservedToken(Token {
                    kind: TokenKind::Colon,
                    representation: _,
                })) => Some(Self {}),
                _ => None,
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Comma {}

    impl Comma {
        #[cfg(test)]
        pub fn new_unspanned() -> Self {
            Self {}
        }
    }

    impl AtomicParse<'_> for Comma {
        fn matches(token: Option<&ComponentValue>) -> Option<Self> {
            match token {
                Some(ComponentValue::PreservedToken(Token {
                    kind: TokenKind::Comma,
                    representation: _,
                })) => Some(Self {}),
                _ => None,
            }
        }
    }

    macro_rules! delimiter_atomic {
        ($name: ident, $chr: literal, $display_str: literal) => {
            #[derive(Debug, Clone, PartialEq, Eq)]
            pub struct $name {}

            #[automatically_derived]
            impl $name {
                #[cfg(test)]
                pub fn new_unspanned() -> Self {
                    Self {}
                }
            }

            #[automatically_derived]
            impl AtomicParse<'_> for $name {
                fn matches(token: Option<&ComponentValue>) -> Option<Self> {
                    match token {
                        Some(ComponentValue::PreservedToken(Token {
                            kind: TokenKind::Delim($chr),
                            representation: _,
                        })) => Some(Self {}),
                        _ => None,
                    }
                }
            }
        };
    }

    delimiter_atomic!(Dot, '.', ".");

    macro_rules! make_keyword {
        ($name: ident, $value: literal) => {
            #[derive(Debug, Clone, PartialEq, Eq)]
            pub struct $name {}

            #[automatically_derived]
            impl $name {
                #[cfg(test)]
                pub fn new_unspanned() -> Self {
                    Self {}
                }
            }

            #[automatically_derived]
            impl AtomicParse<'_> for $name {
                fn matches(token: Option<&ComponentValue>) -> Option<Self> {
                    match token {
                        Some(ComponentValue::PreservedToken(Token {
                            kind: TokenKind::Ident(value),
                            representation: _,
                        })) if &**value == $value => Some(Self {}),
                        _ => None,
                    }
                }
            }
        };
    }

    make_keyword!(Past, "past");
    make_keyword!(Future, "future");
    make_keyword!(Cue, "cue");

    macro_rules! make_function_keyword {
        ($name: ident, $fname: literal) => {
            #[derive(Debug, Clone, PartialEq, Eq)]
            pub struct $name<'a> {
                pub value: ComponentStream<'a>,
            }

            #[automatically_derived]
            impl<'a> $name<'a> {
                #[cfg(test)]
                pub fn new_unspanned(value: ComponentStream<'a>) -> Self {
                    Self { value }
                }
            }

            #[automatically_derived]
            impl<'a> AtomicParse<'a> for $name<'a> {
                fn matches(token: Option<&ComponentValue<'a>>) -> Option<Self> {
                    match token {
                        Some(ComponentValue::Function(Function { name, value }))
                            if &**name == $fname =>
                        {
                            Some(Self {
                                value: value.clone(),
                            })
                        }
                        _ => None,
                    }
                }
            }

            #[automatically_derived]
            impl<'a> BlockLikeToken<'a> for $name<'a> {
                fn parse_content(&self) -> ParseStream<'a> {
                    ParseStream::new(self.value.clone())
                }
            }
        };
    }

    make_function_keyword!(LangFunction, "lang");
    make_function_keyword!(CueFunction, "cue");
}

#[rustfmt::skip]
macro_rules! token_macro {
    (:) => { $crate::css::parse::tokens::Colon };
    (,) => { $crate::css::parse::tokens::Comma };
    (.) => { $crate::css::parse::tokens::Dot };
    (past) => { $crate::css::parse::tokens::Past };
    (future) => { $crate::css::parse::tokens::Future };
    (lang(..)) => { $crate::css::parse::tokens::LangFunction };
    (cue) => { $crate::css::parse::tokens::Cue };
    (cue(..)) => { $crate::css::parse::tokens::CueFunction };
}

pub(crate) use token_macro as Token;

use super::component::{ComponentStream, ComponentValue};
