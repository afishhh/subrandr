//! Utilities that help with using syn as part of a more robust parser.

pub use proc_macro2::{Span as Span2, TokenStream as TokenStream2, TokenTree as TokenTree2};
use syn::parse::{ParseStream, Parser as _};

pub struct ParseContext {
    errors: Vec<syn::Error>,
}

impl ParseContext {
    pub fn new() -> Self {
        Self { errors: Vec::new() }
    }

    pub fn parse2<R>(
        &mut self,
        fun: impl FnOnce(ParseStream, &mut ParseContext) -> Result<R, AlreadyReported>,
        ts: TokenStream2,
    ) -> Result<R, AlreadyReported> {
        let syn_parser = |stream: ParseStream| -> syn::Result<R> {
            // Make syn happy by returning a `Result<R, syn::Error>` ...
            fun(stream, self).map_err(|_| syn::Error::new(Span2::call_site(), "dummy error"))
        };

        match syn_parser.parse2(ts) {
            Ok(value) => Ok(value),
            // ... and then throw away the error because it was `AlreadyReported`.
            Err(_) => Err(AlreadyReported),
        }
    }

    pub fn report(&mut self, error: syn::Error) {
        self.errors.push(error);
    }

    pub fn into_error_stream(self) -> TokenStream2 {
        let mut result = TokenStream2::new();
        for error in self.errors {
            result.extend(error.into_compile_error());
        }
        result
    }
}

pub trait ReportIn: Sized {
    type Ok;

    fn report_in(self, ctx: &mut ParseContext) -> Result<Self::Ok, AlreadyReported>;

    fn report_in_and_set(
        self,
        ctx: &mut ParseContext,
        errored: &mut bool,
    ) -> Result<<Self as ReportIn>::Ok, AlreadyReported> {
        self.report_in(ctx).inspect_err(|_| *errored = true)
    }
}

impl<T> ReportIn for syn::Result<T> {
    type Ok = T;

    fn report_in(self, ctx: &mut ParseContext) -> Result<<Self as ReportIn>::Ok, AlreadyReported> {
        match self {
            Ok(value) => Ok(value),
            Err(error) => {
                ctx.report(error);
                Err(AlreadyReported)
            }
        }
    }
}

pub struct AlreadyReported;
