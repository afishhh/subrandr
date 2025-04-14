pub enum CSSValue {
    Initial,
    Inherit,
    Unset,
    Ident(Box<str>),
    String(Box<str>),
    FunctionalNotation(FunctionalNotation),
}

pub struct FunctionalNotation {
    function: Box<str>,
    // content:
}

pub enum UrlValue {
    Functional(Box<str>),
}
