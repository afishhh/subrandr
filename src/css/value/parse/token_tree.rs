use std::fmt::Write;

use super::{Escaped, Span, Spanned};

macro_rules! impl_spanned {
    ($name: ty) => {
        impl Spanned for $name {
            fn span(&self) -> Span {
                self.span
            }
        }
    };
}

pub enum ValueTokenTree<'a> {
    Ident(Ident<'a>),
    String(LitString<'a>),
    Number(Number<'a>),
    Percentage(Percentage<'a>),
    Dimension(Dimension<'a>),
    FunctionalNotation(FunctionalNotation<'a>),
    UnquotedUrl(UnquotedUrl<'a>),
    Punct(Punct),
}

impl<'a> Spanned for ValueTokenTree<'a> {
    fn span(&self) -> Span {
        match self {
            ValueTokenTree::Ident(ident) => ident.span(),
            ValueTokenTree::String(string) => string.span(),
            ValueTokenTree::FunctionalNotation(functional_notation) => functional_notation.span(),
            ValueTokenTree::UnquotedUrl(unquoted_url) => unquoted_url.span(),
            ValueTokenTree::Number(number) => number.span(),
            ValueTokenTree::Percentage(percentage) => percentage.span(),
            ValueTokenTree::Dimension(dimension) => dimension.span(),
            ValueTokenTree::Punct(punct) => punct.span(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Ident<'a> {
    span: Span,
    value: Escaped<'a>,
}

impl_spanned!(Ident<'_>);

impl<'a> Ident<'a> {
    pub(super) fn new(span: Span, value: Escaped<'a>) -> Self {
        Self { span, value }
    }

    pub fn value(&self) -> Escaped<'a> {
        self.value
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LitString<'a> {
    span: Span,
    value: Escaped<'a>,
}

impl_spanned!(LitString<'_>);

impl<'a> LitString<'a> {
    pub(super) fn new(span: Span, value: Escaped<'a>) -> Self {
        Self { span, value }
    }

    pub fn value(&self) -> Escaped<'a> {
        self.value
    }
}

#[derive(Debug, Clone, Copy)]
pub struct NumericTokenValue<'a> {
    pub(super) value: &'a str,
    pub(super) integer: bool,
}

impl<'a> NumericTokenValue<'a> {
    pub fn to_f64(&self) -> f64 {
        self.value.parse().unwrap()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Number<'a> {
    pub(super) span: Span,
    pub(super) value: NumericTokenValue<'a>,
}

impl_spanned!(Number<'_>);

impl<'a> Number<'a> {
    pub fn value(&self) -> NumericTokenValue<'a> {
        self.value
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Percentage<'a> {
    pub(super) span: Span,
    pub(super) value: NumericTokenValue<'a>,
}

impl_spanned!(Percentage<'_>);

impl<'a> Percentage<'a> {
    pub fn value(&self) -> NumericTokenValue<'a> {
        self.value
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Dimension<'a> {
    pub(super) span: Span,
    pub(super) text: &'a str,
    pub(super) integer: bool,
    pub(super) unit_offset: u32,
}

impl_spanned!(Dimension<'_>);

impl<'a> Dimension<'a> {
    pub fn value(&self) -> NumericTokenValue<'a> {
        NumericTokenValue {
            value: &self.text[..self.unit_offset as usize],
            integer: self.integer,
        }
    }

    pub fn unit(&self) -> Escaped<'a> {
        Escaped::new(&self.text[self.unit_offset as usize..])
    }
}

pub struct FunctionalNotation<'a> {
    pub(super) span: Span,
    pub(super) function: Escaped<'a>,
    pub(super) content: Vec<ValueTokenTree<'a>>,
}

impl_spanned!(FunctionalNotation<'_>);

pub struct UnquotedUrl<'a> {
    pub(super) span: Span,
    pub(super) value: Escaped<'a>,
}

impl_spanned!(UnquotedUrl<'_>);

#[derive(Debug, Clone)]
pub struct Punct {
    pub(super) span: Span,
    pub(super) value: char,
}

impl_spanned!(Punct);

impl Punct {
    pub fn value(&self) -> char {
        self.value
    }
}

impl<'a> ValueTokenTree<'a> {
    pub(super) fn display_for_error(&self, reveal_ident: bool) -> impl std::fmt::Display + '_ {
        ValueTokenTreeErrorDisplay::Static(match self {
            Self::Ident(ident) => {
                if reveal_ident {
                    return ValueTokenTreeErrorDisplay::Ident(ident.value());
                } else {
                    "<ident>"
                }
            }
            Self::String(_) => "<string>",
            Self::FunctionalNotation(_) => "<function>",
            Self::UnquotedUrl(_) => "<unquoted-url>",
            &Self::Number(Number {
                value: NumericTokenValue { integer, .. },
                ..
            }) => match integer {
                true => "<integer>",
                false => "<number>",
            },
            Self::Percentage(_) => "<percentage>",
            Self::Dimension(_) => "<dimension>",
            &Self::Punct(Punct { value, .. }) => return ValueTokenTreeErrorDisplay::Punct(value),
        })
    }
}

enum ValueTokenTreeErrorDisplay<'a> {
    Ident(Escaped<'a>),
    Static(&'static str),
    Punct(char),
}

impl std::fmt::Display for ValueTokenTreeErrorDisplay<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            ValueTokenTreeErrorDisplay::Ident(i) => write!(f, "{i}"),
            ValueTokenTreeErrorDisplay::Static(s) => f.write_str(s),
            ValueTokenTreeErrorDisplay::Punct(c) => f.write_char(c),
        }
    }
}
