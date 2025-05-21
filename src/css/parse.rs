use tokenizer::{HashToken, HashTypeFlag, Token, TokenKind};

pub mod tokenizer;

// TODO: Implement parse errers
#[derive(Debug)]
pub struct ParseError {}

#[derive(Debug, Clone)]
pub struct ParseStream<'a> {
    // TODO: Support TokenStream as a backend for ComponentStream
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
        if let Some(component) = self.stream.components().get(self.position) {
            self.position += 1;
            Some(component)
        } else {
            None
        }
    }

    fn peek_token(&mut self) -> Option<&ComponentValue<'a>> {
        self.stream.components().get(self.position)
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
        T::matches(self.stream.components().get(self.position + OFF))
    }

    pub fn lookahead1(&self) -> Lookahead1<'a, '_> {
        Lookahead1 { stream: self }
    }

    pub fn is_empty(&self) -> bool {
        self.stream.len() == self.position
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

impl<'a, T: AtomicParse<'a>> Parse<'a> for Option<T> {
    fn parse(stream: &mut ParseStream<'a>) -> Result<Self, ParseError> {
        if let Some(parsed) = T::matches(stream.peek_token()) {
            stream.advance_by(1);
            Ok(Some(parsed))
        } else {
            Ok(None)
        }
    }
}

impl<'a> Parse<'a> for ComponentStream<'a> {
    fn parse(stream: &mut ParseStream<'a>) -> Result<Self, ParseError> {
        Ok({
            let result = stream
                .stream
                .substream(stream.position..stream.stream.len());
            stream.position = stream.stream.len();
            result
        })
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

    macro_rules! make_simple {
        ($name: ident, $token: ident, $display_str: literal) => {
            #[derive(Debug, Clone, PartialEq, Eq)]
            pub struct $name {}

            #[automatically_derived]
            #[allow(dead_code)]
            impl $name {
                #[cfg(test)]
                pub fn new_unspanned() -> Self {
                    Self {}
                }
            }

            #[automatically_derived]
            #[allow(dead_code)]
            impl AtomicParse<'_> for $name {
                fn matches(token: Option<&ComponentValue>) -> Option<Self> {
                    match token {
                        Some(ComponentValue::PreservedToken(Token {
                            kind: TokenKind::$token,
                            representation: _,
                        })) => Some(Self {}),
                        _ => None,
                    }
                }
            }
        };
    }

    macro_rules! make_delim {
        ($name: ident, $chr: literal, $display_str: literal) => {
            #[derive(Debug, Clone, PartialEq, Eq)]
            pub struct $name {}

            #[automatically_derived]
            #[allow(dead_code)]
            impl $name {
                #[cfg(test)]
                pub fn new_unspanned() -> Self {
                    Self {}
                }
            }

            #[automatically_derived]
            #[allow(dead_code)]
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

    macro_rules! make_keyword {
        ($name: ident, $value: literal) => {
            #[derive(Debug, Clone, PartialEq, Eq)]
            pub struct $name {}

            #[automatically_derived]
            #[allow(dead_code)]
            impl $name {
                #[cfg(test)]
                pub fn new_unspanned() -> Self {
                    Self {}
                }
            }

            #[automatically_derived]
            #[allow(dead_code)]
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

    macro_rules! make_function_keyword {
        ($name: ident, $fname: literal) => {
            #[derive(Debug, Clone, PartialEq, Eq)]
            pub struct $name<'a> {
                pub value: ComponentStream<'a>,
            }

            #[automatically_derived]
            #[allow(dead_code)]
            impl<'a> $name<'a> {
                #[cfg(test)]
                pub fn new_unspanned(value: ComponentStream<'a>) -> Self {
                    Self { value }
                }
            }

            #[automatically_derived]
            #[allow(dead_code)]
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
            #[allow(dead_code)]
            impl<'a> BlockLikeToken<'a> for $name<'a> {
                fn parse_content(&self) -> ParseStream<'a> {
                    ParseStream::new(self.value.clone())
                }
            }
        };
    }

    macro_rules! make_all {
        (
            $dolar: tt,
            $($what: ident $name: ident [$($token_name: tt)*] ($($params: tt)*);)*
        ) => {
            macro_rules! Token {
                $(($($token_name)*) => { $dolar crate::css::parse::tokens::$name };)*
            }

            $(make_all!(@mktype $what $name $($params)*);)*
        };
        (@mktype simple $name: ident $($params: tt)*) => {
            make_simple!($name, $($params)*);
        };
        (@mktype delim $name: ident $($params: tt)*) => {
            make_delim!($name, $($params)*);
        };
        (@mktype keyword $name: ident $($params: tt)*) => {
            make_keyword!($name, $($params)*);
        };
        (@mktype function_keyword $name: ident $($params: tt)*) => {
            make_function_keyword!($name, $($params)*);
        };
    }

    make_simple!(LBrace, LBrace, "{");
    make_simple!(RBrace, RBrace, "}");
    make_simple!(LBracket, LBracket, "[");
    make_simple!(RBracket, RBracket, "]");

    make_all! {
        $,
        simple Colon [:] (Colon, ":");
        simple Comma [,] (Comma, ",");
        delim Dot [.] ('.', ".");

        keyword Past        [past] ("past");
        keyword Future      [future] ("future");
        keyword Cue         [cue] ("cue");
        keyword Auto        [auto] ("auto");
        keyword Inherit     [inherit] ("inherit");
        keyword Initial     [initial] ("initial");
        keyword Unset       [unset] ("unset");
        keyword Static      [static] ("static");
        keyword Relative    [relative] ("relative");
        keyword Absolute    [absolute] ("absolute");
        keyword Sticky      [sticky] ("sticky");
        keyword Fixed       [fixed] ("fixed");

        keyword Normal      [normal] ("normal");

        keyword Pre         [pre] ("pre");
        keyword Nowrap      [nowrap] ("nowrap");
        keyword PreWrap     [pre-wrap] ("pre-wrap");
        keyword BreakSpaces [break-spaces] ("break-spaces");
        keyword PreLine     [pre-line] ("pre-line");

        keyword Italic      [italic] ("italic");
        keyword Oblique     [oblique] ("oblique");

        keyword Alternate   [alternate] ("alternate");
        keyword Over        [over]      ("over");
        keyword Under       [under]     ("under");
        keyword InterCharacter [inter-character] ("inter-character");

        function_keyword LangFunction [lang(..)] ("lang");
        function_keyword CueFunction [cue(..)] ("cue");
    }

    pub(crate) use Token;
}

pub(crate) use tokens::Token;

use super::component::{ComponentStream, ComponentValue};
