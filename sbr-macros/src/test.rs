use proc_macro2::{TokenStream as TokenStream2, TokenTree as TokenTree2};
use quote::quote;
use syn::{parse::ParseStream, Token};

use crate::parse::{AlreadyReported, ParseContext, ReportIn as _};

#[derive(Debug, Clone)]
struct DefineStyleClass {
    visibility: syn::Visibility,
    name: syn::Ident,
    source: syn::LitStr,
}

#[derive(Debug, Clone)]
struct DefineStyleInput {
    classes: Vec<DefineStyleClass>,
}

impl DefineStyleInput {
    fn parse(buffer: ParseStream, ctx: &mut ParseContext) -> Result<Self, AlreadyReported> {
        let mut errored = false;
        let mut result = Self {
            classes: Vec::new(),
        };

        while !buffer.is_empty() {
            let Ok(visibility) = buffer
                .parse::<syn::Visibility>()
                .report_in_and_set(ctx, &mut errored)
            else {
                _ = buffer.parse::<TokenTree2>();
                continue;
            };

            let Ok(_) = buffer
                .parse::<Token![.]>()
                .report_in_and_set(ctx, &mut errored)
            else {
                _ = buffer.parse::<TokenTree2>();
                continue;
            };

            let Ok(name) = buffer
                .parse::<syn::Ident>()
                .report_in_and_set(ctx, &mut errored)
            else {
                _ = buffer.parse::<TokenTree2>();
                continue;
            };

            let Ok(source) = buffer
                .parse::<syn::LitStr>()
                .report_in_and_set(ctx, &mut errored)
            else {
                _ = buffer.parse::<TokenTree2>();
                continue;
            };

            result.classes.push(DefineStyleClass {
                visibility,
                name,
                source,
            });
        }

        if errored {
            Err(AlreadyReported)
        } else {
            Ok(result)
        }
    }
}

fn declarations_name_for_class_name(class: &syn::Ident) -> syn::Ident {
    syn::Ident::new(&format!("STYLE_{class}_DECLARATIONS"), class.span())
}

pub fn test_define_style(ts: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = {
        let mut parse_ctx = ParseContext::new();
        let Ok(input) = parse_ctx.parse2(DefineStyleInput::parse, ts.into()) else {
            return parse_ctx.into_error_stream().into();
        };
        input
    };

    let mut result = TokenStream2::new();

    for class in input.classes {
        let visibility = class.visibility;
        let declarations_name = declarations_name_for_class_name(&class.name);
        let source = class.source;

        result.extend(quote! {
            #[allow(non_upper_case_globals)]
            #visibility static #declarations_name: std::sync::LazyLock<
                &'static [crate::csssyn::algorithms::Declaration<'static>]
            > =
                std::sync::LazyLock::new(|| {
                    let buffer = Box::leak(Box::new(crate::csssyn::TokenBuffer::from_source(#source).unwrap()));
                    let declarations = crate::csssyn::algorithms::parse_declaration_list(buffer.start());
                    Box::leak(declarations.collect::<Vec<_>>().into_boxed_slice())
                });
        });
    }

    result.into()
}

struct ApplyStyleInput {
    log: syn::Expr,
    parent: syn::Expr,
    classes: Vec<syn::Ident>,
}

impl ApplyStyleInput {
    fn parse(buffer: ParseStream, ctx: &mut ParseContext) -> Result<Self, AlreadyReported> {
        Ok(Self {
            log: {
                let log = buffer.parse::<syn::Expr>().report_in(ctx)?;
                buffer.parse::<Token![,]>().report_in(ctx)?;
                log
            },
            parent: {
                let parent = buffer.parse::<syn::Expr>().report_in(ctx)?;
                buffer.parse::<Token![,]>().report_in(ctx)?;
                parent
            },
            classes: {
                let mut result = Vec::new();
                while !buffer.is_empty() {
                    result.push(buffer.parse::<syn::Ident>().report_in(ctx)?);
                }
                result
            },
        })
    }
}

pub fn test_apply_styles(ts: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = {
        let mut parse_ctx = ParseContext::new();
        let Ok(input) = parse_ctx.parse2(ApplyStyleInput::parse, ts.into()) else {
            return parse_ctx.into_error_stream().into();
        };
        input
    };

    let log = input.log;
    let parent = input.parent;
    let class_declarations = input
        .classes
        .into_iter()
        .map(|class| declarations_name_for_class_name(&class));

    quote! {
        crate::style::compute_with_declarations(
            #log,
            &mut [ #(&#class_declarations[..],)* ].into_iter(),
            #parent
        )
    }
    .into()
}
