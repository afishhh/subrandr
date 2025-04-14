use super::parse::{
    parse_whole, parse_whole_with, tokenizer::is_whitespace, BlockLikeToken, IdHash, Ident, Parse,
    ParseError, ParseStream, Punctuated, StringLiteral, Token,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompoundSelector {
    type_selector: Option<TypeSelector>,
    subclass_selectors: Vec<SubclassSelector>,
    pseudo_element: Option<PseudoElementSelector>,
}

impl Parse<'_> for CompoundSelector {
    fn parse(stream: &mut ParseStream<'_>) -> Result<Self, ParseError> {
        let mut result = CompoundSelector {
            type_selector: None,
            subclass_selectors: Vec::new(),
            pseudo_element: None,
        };

        if stream.peek::<Ident>() {
            result.type_selector = Some(stream.parse()?);
        }

        loop {
            if stream.peek::<Token![:]>() && stream.peek2::<Token![:]>() {
                result.pseudo_element = Some(stream.parse()?);
                break;
            } else if SubclassSelector::peek_in(stream) {
                result.subclass_selectors.push(dbg!(stream.parse())?);
            } else {
                break;
            }
        }

        Ok(result)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubclassSelector {
    Id(IdSelector),
    Class(ClassSelector),
    Attribute(AttributeSelector),
    PseudoClass(PseudoClassSelector),
}

impl SubclassSelector {
    fn peek_in(stream: &ParseStream) -> bool {
        // TODO: bracket
        stream.peek::<IdHash>() || stream.peek::<Token![.]>() || stream.peek::<Token![:]>()
    }
}

impl Parse<'_> for SubclassSelector {
    fn parse(stream: &mut ParseStream<'_>) -> Result<Self, ParseError> {
        let mut lk = stream.lookahead1();
        if lk.peek::<IdHash>() {
            Ok(Self::Id(stream.parse()?))
        } else if lk.peek::<Token![.]>() {
            Ok(Self::Class(stream.parse()?))
            // TODO: attribute selector
        } else if lk.peek::<Token![:]>() {
            Ok(Self::PseudoClass(stream.parse()?))
        } else {
            Err(lk.error())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeSelector(pub Ident);

impl Parse<'_> for TypeSelector {
    fn parse(stream: &mut ParseStream) -> Result<Self, ParseError> {
        Ok(Self(stream.parse()?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdSelector(pub IdHash);

impl Parse<'_> for IdSelector {
    fn parse(stream: &mut ParseStream) -> Result<Self, ParseError> {
        Ok(Self(stream.parse()?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassSelector {
    pub dot: Token![.],
    pub name: Ident,
}

impl Parse<'_> for ClassSelector {
    fn parse(stream: &mut ParseStream) -> Result<Self, ParseError> {
        Ok(Self {
            dot: stream.parse()?,
            name: stream.parse()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeSelector {
    attribute: Box<str>,
    value: AttributeSelectorOperator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PseudoClassSelector {
    pub colon: Token![:],
    pub kind: PseudoClassSelectorKind,
}

impl Parse<'_> for PseudoClassSelector {
    fn parse(stream: &mut ParseStream<'_>) -> Result<Self, ParseError> {
        Ok(Self {
            colon: stream.parse()?,
            kind: stream.parse()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PseudoClassSelectorKind {
    Past(Token![past]),
    Future(Token![future]),
    Lang(LangPseudoClassSelector),
}

impl Parse<'_> for PseudoClassSelectorKind {
    fn parse(stream: &mut ParseStream) -> Result<Self, ParseError> {
        let mut lk = stream.lookahead1();
        if lk.peek::<Token![past]>() {
            Ok(Self::Past(stream.parse()?))
        } else if lk.peek::<Token![future]>() {
            Ok(Self::Future(stream.parse()?))
        } else if lk.peek::<Token![lang(..)]>() {
            Ok(Self::Lang(stream.parse()?))
        } else {
            Err(lk.error())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LangPseudoClassSelector(pub Punctuated<LanguageRange, Token![,]>);

impl Parse<'_> for LangPseudoClassSelector {
    fn parse(stream: &mut ParseStream<'_>) -> Result<Self, ParseError> {
        let langf = stream.parse::<Token![lang(..)]>()?;

        Ok(Self(parse_whole_with(
            langf.parse_content(),
            Punctuated::parse_separated_skip_whitespace,
        )?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LanguageRange {
    Ident(Ident),
    String(StringLiteral),
}

impl Parse<'_> for LanguageRange {
    fn parse(stream: &mut ParseStream<'_>) -> Result<Self, ParseError> {
        let mut lk = stream.lookahead1();
        if lk.peek::<Ident>() {
            Ok(Self::Ident(stream.parse()?))
        } else if lk.peek::<StringLiteral>() {
            Ok(Self::String(stream.parse()?))
        } else {
            Err(lk.error())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PseudoElementSelector {
    pub colon1: Token![:],
    pub colon2: Token![:],
    pub kind: PseudoElementSelectorKind,
}

impl Parse<'_> for PseudoElementSelector {
    fn parse(stream: &mut ParseStream<'_>) -> Result<Self, ParseError> {
        Ok(Self {
            colon1: stream.parse()?,
            colon2: stream.parse()?,
            kind: stream.parse()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PseudoElementSelectorKind {
    Cue(CuePsuedoElement),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CuePsuedoElement {
    pub selector: Option<Box<CompoundSelector>>,
}

impl Parse<'_> for PseudoElementSelectorKind {
    fn parse(stream: &mut ParseStream<'_>) -> Result<Self, ParseError> {
        let mut lk = stream.lookahead1();
        if lk.peek::<Token![cue(..)]>() {
            let func = stream.parse::<Token![cue(..)]>()?;
            let selector = parse_whole::<CompoundSelector>(func.parse_content())?;
            Ok(PseudoElementSelectorKind::Cue(CuePsuedoElement {
                selector: Some(Box::new(selector)),
            }))
        } else if lk.peek::<Token![cue]>() {
            stream.parse::<Token![cue]>()?;
            Ok(PseudoElementSelectorKind::Cue(CuePsuedoElement {
                selector: None,
            }))
        } else {
            Err(lk.error())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompoundSelectorList(Punctuated<CompoundSelector, Token![,]>);

impl Parse<'_> for CompoundSelectorList {
    fn parse(stream: &mut ParseStream<'_>) -> Result<Self, ParseError> {
        Punctuated::parse_separated_skip_whitespace(stream).map(Self)
    }
}

// TODO: Log unsupported stuff
// TODO: Qualified names exist

#[cfg(test)]
mod test {
    use super::*;
    use crate::css::component::TokenParser;

    fn tokenize_and_parse<T: for<'a> Parse<'a>>(text: &str) -> Result<T, ParseError> {
        parse_whole(ParseStream::new(
            TokenParser::new(text).parse_component_stream(),
        ))
    }

    #[test]
    fn parse_compound_selectors() {
        assert_eq!(
            tokenize_and_parse::<CompoundSelector>("div.class").unwrap(),
            CompoundSelector {
                type_selector: Some(TypeSelector(Ident::new_unspanned("div"))),
                subclass_selectors: vec![SubclassSelector::Class(ClassSelector {
                    dot: <Token![.]>::new_unspanned(),
                    name: Ident::new_unspanned("class")
                })],
                pseudo_element: None
            }
        );

        assert_eq!(
            tokenize_and_parse::<CompoundSelector>(
                "div.class#id.class2:past::cue(.class-in-cue:future)",
            )
            .unwrap(),
            CompoundSelector {
                type_selector: Some(TypeSelector(Ident::new_unspanned("div"))),
                subclass_selectors: vec![
                    SubclassSelector::Class(ClassSelector {
                        dot: <Token![.]>::new_unspanned(),
                        name: Ident::new_unspanned("class"),
                    }),
                    SubclassSelector::Id(IdSelector(IdHash::new_unspanned("id"))),
                    SubclassSelector::Class(ClassSelector {
                        dot: <Token![.]>::new_unspanned(),
                        name: Ident::new_unspanned("class2"),
                    }),
                    SubclassSelector::PseudoClass(PseudoClassSelector {
                        colon: <Token![:]>::new_unspanned(),
                        kind: PseudoClassSelectorKind::Past(<Token![past]>::new_unspanned()),
                    }),
                ],
                pseudo_element: Some(PseudoElementSelector {
                    colon1: <Token![:]>::new_unspanned(),
                    colon2: <Token![:]>::new_unspanned(),
                    kind: PseudoElementSelectorKind::Cue(CuePsuedoElement {
                        selector: Some(Box::new(CompoundSelector {
                            type_selector: None,
                            subclass_selectors: vec![
                                SubclassSelector::Class(ClassSelector {
                                    dot: <Token![.]>::new_unspanned(),
                                    name: Ident::new_unspanned("class-in-cue"),
                                }),
                                SubclassSelector::PseudoClass(PseudoClassSelector {
                                    colon: <Token![:]>::new_unspanned(),
                                    kind: PseudoClassSelectorKind::Future(
                                        <Token![future]>::new_unspanned()
                                    ),
                                }),
                            ],
                            pseudo_element: None,
                        }))
                    })
                })
            }
        );
    }
    #[test]
    fn parse_compound_selector_list() {
        assert_eq!(
            tokenize_and_parse::<CompoundSelectorList>("div.class").unwrap(),
            CompoundSelectorList(Punctuated(vec![(
                CompoundSelector {
                    type_selector: Some(TypeSelector(Ident::new_unspanned("div"))),
                    subclass_selectors: vec![SubclassSelector::Class(ClassSelector {
                        dot: <Token![.]>::new_unspanned(),
                        name: Ident::new_unspanned("class")
                    })],
                    pseudo_element: None
                },
                None
            )]))
        );

        assert_eq!(
            tokenize_and_parse::<CompoundSelectorList>(
                r#"div.class:lang(en-US, "*-JP"), span#id ,::cue"#,
            )
            .unwrap(),
            CompoundSelectorList(Punctuated(vec![
                (
                    CompoundSelector {
                        type_selector: Some(TypeSelector(Ident::new_unspanned("div"))),
                        subclass_selectors: vec![
                            SubclassSelector::Class(ClassSelector {
                                dot: <Token![.]>::new_unspanned(),
                                name: Ident::new_unspanned("class")
                            }),
                            SubclassSelector::PseudoClass(PseudoClassSelector {
                                colon: <Token![:]>::new_unspanned(),
                                kind: PseudoClassSelectorKind::Lang(LangPseudoClassSelector(
                                    Punctuated(vec![
                                        (
                                            LanguageRange::Ident(Ident::new_unspanned("en-US")),
                                            Some(<Token![,]>::new_unspanned())
                                        ),
                                        (
                                            LanguageRange::String(StringLiteral::new_unspanned(
                                                "*-JP"
                                            )),
                                            None
                                        )
                                    ])
                                ))
                            })
                        ],
                        pseudo_element: None
                    },
                    Some(<Token![,]>::new_unspanned())
                ),
                (
                    CompoundSelector {
                        type_selector: Some(TypeSelector(Ident::new_unspanned("span"))),
                        subclass_selectors: vec![SubclassSelector::Id(IdSelector(
                            IdHash::new_unspanned("id")
                        ))],
                        pseudo_element: None
                    },
                    Some(<Token![,]>::new_unspanned())
                ),
                (
                    CompoundSelector {
                        type_selector: None,
                        subclass_selectors: vec![],
                        pseudo_element: Some(PseudoElementSelector {
                            colon1: <Token![:]>::new_unspanned(),
                            colon2: <Token![:]>::new_unspanned(),
                            kind: PseudoElementSelectorKind::Cue(CuePsuedoElement {
                                selector: None
                            })
                        })
                    },
                    None
                ),
            ]))
        );
    }
}
