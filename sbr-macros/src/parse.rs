//! Utilities that help with doing error recovery when using syn.

pub use proc_macro2::{TokenStream as TokenStream2, TokenTree as TokenTree2};
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
        let syn_parser = |stream: ParseStream| -> syn::Result<Result<R, AlreadyReported>> {
            Ok(fun(stream, self))
        };

        match syn_parser.parse2(ts).report_in(self) {
            Ok(Ok(result)) => Ok(result),
            Err(AlreadyReported) | Ok(Err(AlreadyReported)) => Err(AlreadyReported),
        }
    }

    pub fn report(&mut self, error: syn::Error) {
        self.errors.push(error);
    }

    pub fn into_errors(self) -> impl Iterator<Item = syn::Error> {
        self.errors.into_iter()
    }

    pub fn into_error_stream(self) -> TokenStream2 {
        let mut result = TokenStream2::new();
        for error in self.into_errors() {
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

#[derive(Debug)]
pub struct AlreadyReported;

macro_rules! wrap_syn_group_macro {
    (syn::$macro: ident in $stream: expr) => {
         (|| {
            let inner;
            let delim = syn::$macro!(inner in $stream);
            Ok((inner, delim))
        })()
    };
}
pub(crate) use wrap_syn_group_macro;
