use super::{impl_spanned, Cursor, Span, Spanned};
use crate::csssyn::tokenizer::Escaped;

#[derive(Debug, Clone, Copy)]
pub struct Ident<'a> {
    pub(super) span: Span,
    pub(super) value: Escaped<'a>,
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
    pub(super) span: Span,
    pub(super) value: Escaped<'a>,
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
    pub(super) content: Cursor<'a>,
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
