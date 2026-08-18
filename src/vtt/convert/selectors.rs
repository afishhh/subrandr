//! Implements parsing and matching of a subset of CSS [Selectors].
//!
//! This implementation is specific to WebVTT and has bad error messages
//! but is reasonably simple.
//!
//! [Selectors]: https://drafts.csswg.org/selectors/
use crate::{
    csssyn::{
        buffer::{Cursor, HashTypeFlag},
        token::{FunctionalNotation, Hash, Ident, Token, Whitespace},
        ParseError,
    },
    vtt::convert::{Element, ElementKind, SpanKind},
};

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ComplexSelectorUnit {
    selector: Option<CompoundSelector>,
    pseudo: Option<PseudoCompoundSelector>,
}

#[derive(Debug, PartialEq, Eq)]
struct CompoundSelector {
    type_: Option<TypeSelector>,
    subclasses: Vec<SubclassSelector>,
}

#[derive(Debug, PartialEq, Eq)]
struct PseudoCompoundSelector {
    element: PseudoElementSelector,
}

#[derive(Debug, PartialEq, Eq)]
enum PseudoElementSelector {
    Cue(Option<Box<ComplexSelectorUnit>>),
}

#[derive(Debug, PartialEq, Eq)]
enum TypeSelector {
    Ident(Box<str>),
    Universal,
}

#[derive(Debug, PartialEq, Eq)]
enum SubclassSelector {
    Id(Box<str>),
    Class(Box<str>),
}

fn consume_compound_selector(
    mut cursor: Cursor,
) -> Result<(Option<CompoundSelector>, Cursor), ParseError> {
    let type_ = if let Some((ident, next)) = cursor.take::<Ident>() {
        cursor = next;
        Some(TypeSelector::Ident(ident.value().into()))
    } else if let Some(next) = cursor.next_if(Token![*]) {
        cursor = next;
        Some(TypeSelector::Universal)
    } else {
        None
    };

    let mut subclasses = Vec::new();
    loop {
        if let Some((hash, next)) = cursor.take::<Hash>() {
            if hash.type_flag() != HashTypeFlag::Id {
                return Err(ParseError::new(
                    hash,
                    "<id-selector>'s <hash> value must be an identifier",
                ));
            }

            cursor = next;
            subclasses.push(SubclassSelector::Id(hash.value().into()));
        } else if let Some(next) = cursor.next_if(Token![.]) {
            let Some((ident, next)) = next.take::<Ident>() else {
                return Err(ParseError::new(cursor, "invalid class selector"));
            };
            cursor = next;
            subclasses.push(SubclassSelector::Class(ident.value().into()));
        } else {
            break;
        }
    }

    if type_.is_none() && subclasses.is_empty() {
        Ok((None, cursor))
    } else {
        Ok((Some(CompoundSelector { type_, subclasses }), cursor))
    }
}

fn consume_pseudo_element_selector(
    cursor: Cursor,
) -> Result<Option<(PseudoElementSelector, Cursor)>, ParseError> {
    let Some(cursor) = cursor.next_if(Token![:]) else {
        return Ok(None);
    };
    let Some(cursor) = cursor.next_if(Token![:]) else {
        return Err(ParseError::unexpected(cursor, &[<Token![:]>::name()]));
    };

    if let Some((ident, cursor)) = cursor.take::<Ident>() {
        if ident.value().eq_ignore_ascii_case("cue") {
            return Ok(Some((PseudoElementSelector::Cue(None), cursor)));
        }
    } else if let Ok((func, cursor)) = FunctionalNotation::take(cursor) {
        if func.function().eq_ignore_ascii_case("cue") {
            return Ok(Some((
                PseudoElementSelector::Cue(Some(Box::new(parse_complex_selector_unit(
                    func.content(),
                )?))),
                cursor,
            )));
        }
    } else {
        return Err(ParseError::unexpected(
            cursor,
            &[Ident::name(), FunctionalNotation::name()],
        ));
    }

    Err(ParseError::new(cursor, "unknown pseudo element"))
}

fn consume_pseudo_compound_selector(
    cursor: Cursor,
) -> Result<Option<(PseudoCompoundSelector, Cursor)>, ParseError> {
    match consume_pseudo_element_selector(cursor) {
        Ok(Some((element, cursor))) => Ok(Some((PseudoCompoundSelector { element }, cursor))),
        Ok(None) => Ok(None),
        Err(err) => Err(err),
    }
}

pub fn parse_complex_selector_unit(mut cursor: Cursor) -> Result<ComplexSelectorUnit, ParseError> {
    cursor = cursor.skip(Whitespace);

    let compound;
    (compound, cursor) = consume_compound_selector(cursor)?;

    let pseudo = match consume_pseudo_compound_selector(cursor)? {
        Some((pseudo, next)) => {
            cursor = next;
            Some(pseudo)
        }
        None => None,
    };

    if compound.is_none() && pseudo.is_none() {
        return Err(ParseError::new(cursor, "invalid complex selector unit"));
    }

    cursor = cursor.skip(Whitespace);

    if !cursor.eof() {
        return Err(ParseError::new(cursor, "unexpected tokens"));
    }

    Ok(ComplexSelectorUnit {
        selector: compound,
        pseudo,
    })
}

impl TypeSelector {
    fn matches_anonymous(&self) -> bool {
        match self {
            TypeSelector::Ident(_) => false,
            TypeSelector::Universal => true,
        }
    }

    fn matches_in_cue(&self, element: &Element) -> bool {
        match self {
            TypeSelector::Ident(type_) => element.type_().is_some_and(|t| t == &**type_),
            TypeSelector::Universal => true,
        }
    }
}

impl SubclassSelector {
    fn matches_anonymous(&self) -> bool {
        match self {
            SubclassSelector::Id(_) => false,
            SubclassSelector::Class(_) => false,
        }
    }

    fn matches_in_cue(&self, element: &Element) -> bool {
        match self {
            SubclassSelector::Id(id) => element.id().is_some_and(|i| i == &**id),
            SubclassSelector::Class(class) => element.classlist().any(|c| c == &**class),
        }
    }
}

impl CompoundSelector {
    fn matches_anonymous(&self) -> bool {
        self.type_.as_ref().is_none_or(|s| s.matches_anonymous())
            && self.subclasses.iter().all(|s| s.matches_anonymous())
    }

    fn matches_in_cue(&self, element: &Element) -> bool {
        self.type_
            .as_ref()
            .is_none_or(|s| s.matches_in_cue(element))
            && self.subclasses.iter().all(|s| s.matches_in_cue(element))
    }
}

impl ComplexSelectorUnit {
    fn matches_in_cue(&self, element: &Element) -> bool {
        self.pseudo.is_none()
            && self
                .selector
                .as_ref()
                .is_none_or(|x| x.matches_in_cue(element))
    }
}

impl ComplexSelectorUnit {
    pub(super) fn matches(&self, cue_element: &Element) -> bool {
        self.selector.as_ref().is_none_or(|s| s.matches_anonymous())
            && match self.pseudo.as_ref() {
                Some(PseudoCompoundSelector {
                    element: PseudoElementSelector::Cue(cue_selector),
                }) => match cue_selector {
                    Some(selector) => selector.matches_in_cue(cue_element),
                    None => matches!(cue_element.kind, ElementKind::Span(SpanKind::Root)),
                },
                None => false,
            }
    }
}

#[derive(Default, Debug, Clone, Copy, PartialOrd, Ord, PartialEq, Eq)]
pub(super) struct Specificity {
    a: u16,
    b: u16,
    c: u16,
}

impl Specificity {
    const ZERO: Self = Self { a: 0, b: 0, c: 0 };
}

impl std::ops::Add for Specificity {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            a: self.a + rhs.a,
            b: self.b + rhs.b,
            c: self.c + rhs.c,
        }
    }
}

impl std::ops::AddAssign for Specificity {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl CompoundSelector {
    // https://drafts.csswg.org/selectors/#specificity-rules
    fn specificity(&self) -> Specificity {
        let mut result = Specificity::ZERO;

        match self.type_ {
            Some(TypeSelector::Ident(_)) => result.c += 1,
            Some(TypeSelector::Universal) | None => (),
        }

        for s in &self.subclasses {
            match s {
                SubclassSelector::Id(_) => result.a += 1,
                SubclassSelector::Class(_) => result.b += 1,
            }
        }
        result
    }
}

impl ComplexSelectorUnit {
    pub(super) fn specificity(&self) -> Specificity {
        let mut result = self
            .selector
            .as_ref()
            // TODO: map_or_default (MSRV: 1.98) (below too)
            .map_or(Specificity::ZERO, CompoundSelector::specificity);

        if let Some(pseudo) = self.pseudo.as_ref() {
            result.c += 1;
            match &pseudo.element {
                PseudoElementSelector::Cue(inner) => {
                    result += inner
                        .as_ref()
                        .map_or(Specificity::ZERO, |s| s.specificity());
                }
            }
        }

        result
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::csssyn::TokenBuffer;

    #[track_caller]
    fn test_parse(text: &str, expected: ComplexSelectorUnit, expected_specificity: Specificity) {
        let buffer = TokenBuffer::from_source(text).unwrap();
        let result = parse_complex_selector_unit(buffer.start()).unwrap();

        assert_eq!(result, expected);
        assert_eq!(result.specificity(), expected_specificity);
    }

    #[test]
    fn parse_compound() {
        test_parse(
            ".abc",
            ComplexSelectorUnit {
                selector: Some(CompoundSelector {
                    type_: None,
                    subclasses: vec![SubclassSelector::Class("abc".into())],
                }),
                pseudo: None,
            },
            Specificity { a: 0, b: 1, c: 0 },
        );

        test_parse(
            "type#id.cl1.cl2",
            ComplexSelectorUnit {
                selector: Some(CompoundSelector {
                    type_: Some(TypeSelector::Ident("type".into())),
                    subclasses: vec![
                        SubclassSelector::Id("id".into()),
                        SubclassSelector::Class("cl1".into()),
                        SubclassSelector::Class("cl2".into()),
                    ],
                }),
                pseudo: None,
            },
            Specificity { a: 1, b: 2, c: 1 },
        );

        test_parse(
            "*.cl1#id",
            ComplexSelectorUnit {
                selector: Some(CompoundSelector {
                    type_: Some(TypeSelector::Universal),
                    subclasses: vec![
                        SubclassSelector::Class("cl1".into()),
                        SubclassSelector::Id("id".into()),
                    ],
                }),
                pseudo: None,
            },
            Specificity { a: 1, b: 1, c: 0 },
        );
    }

    #[test]
    fn parse_cue_pseudo_element() {
        test_parse(
            "::cue",
            ComplexSelectorUnit {
                selector: None,
                pseudo: Some(PseudoCompoundSelector {
                    element: PseudoElementSelector::Cue(None),
                }),
            },
            Specificity { a: 0, b: 0, c: 1 },
        );

        test_parse(
            "type#id.abc::cue",
            ComplexSelectorUnit {
                selector: Some(CompoundSelector {
                    type_: Some(TypeSelector::Ident("type".into())),
                    subclasses: vec![
                        SubclassSelector::Id("id".into()),
                        SubclassSelector::Class("abc".into()),
                    ],
                }),
                pseudo: Some(PseudoCompoundSelector {
                    element: PseudoElementSelector::Cue(None),
                }),
            },
            Specificity { a: 1, b: 1, c: 2 },
        );

        test_parse(
            "::cue(.abc)",
            ComplexSelectorUnit {
                selector: None,
                pseudo: Some(PseudoCompoundSelector {
                    element: PseudoElementSelector::Cue(Some(Box::new(ComplexSelectorUnit {
                        selector: Some(CompoundSelector {
                            type_: None,
                            subclasses: vec![SubclassSelector::Class("abc".into())],
                        }),
                        pseudo: None,
                    }))),
                }),
            },
            Specificity { a: 0, b: 1, c: 1 },
        );

        test_parse(
            "::cue(.abc::cue(def))",
            ComplexSelectorUnit {
                selector: None,
                pseudo: Some(PseudoCompoundSelector {
                    element: PseudoElementSelector::Cue(Some(Box::new(ComplexSelectorUnit {
                        selector: Some(CompoundSelector {
                            type_: None,
                            subclasses: vec![SubclassSelector::Class("abc".into())],
                        }),
                        pseudo: Some(PseudoCompoundSelector {
                            element: PseudoElementSelector::Cue(Some(Box::new(
                                ComplexSelectorUnit {
                                    selector: Some(CompoundSelector {
                                        type_: Some(TypeSelector::Ident("def".into())),
                                        subclasses: Vec::new(),
                                    }),
                                    pseudo: None,
                                },
                            ))),
                        }),
                    }))),
                }),
            },
            Specificity { a: 0, b: 1, c: 3 },
        );
    }
}
