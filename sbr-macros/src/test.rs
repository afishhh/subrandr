use proc_macro2::{TokenStream as TokenStream2, TokenTree as TokenTree2};
use quote::quote;
use syn::{parse::ParseStream, Token};

use crate::{
    common::advance_past_punct,
    parse::{wrap_syn_group_macro, AlreadyReported, ParseContext, ReportIn as _},
};

#[derive(Debug, Clone)]
struct DefineStyleClass {
    visibility: syn::Visibility,
    name: syn::Ident,
    properties: Vec<(syn::Ident, syn::Expr)>,
}

impl DefineStyleClass {
    fn parse_properties(
        buffer: ParseStream,
        ctx: &mut ParseContext,
        result: &mut Vec<(syn::Ident, syn::Expr)>,
    ) -> Result<(), AlreadyReported> {
        let mut errored = false;

        while !buffer.is_empty() {
            let Ok(name) = buffer.parse::<syn::Ident>().report_in(ctx) else {
                errored = true;
                advance_past_punct(buffer, ';');
                continue;
            };

            let Ok(_) = buffer.parse::<Token![:]>().report_in(ctx) else {
                errored = true;
                advance_past_punct(buffer, ';');
                continue;
            };

            let Ok(value) = buffer.parse::<syn::Expr>().report_in(ctx) else {
                errored = true;
                advance_past_punct(buffer, ';');
                continue;
            };

            result.push((name, value));

            if buffer.is_empty() {
                break;
            }

            errored |= buffer.parse::<Token![,]>().report_in(ctx).is_err();
        }

        if errored {
            Err(AlreadyReported)
        } else {
            Ok(())
        }
    }
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

            let Ok((inner, _)) =
                wrap_syn_group_macro!(syn::braced in buffer).report_in_and_set(ctx, &mut errored)
            else {
                _ = buffer.parse::<TokenTree2>();
                continue;
            };

            let mut group = DefineStyleClass {
                visibility,
                name,
                properties: Vec::new(),
            };

            if let Ok(()) = DefineStyleClass::parse_properties(&inner, ctx, &mut group.properties) {
                result.classes.push(group);
            } else {
                errored = true;
            }
        }

        if errored {
            Err(AlreadyReported)
        } else {
            Ok(result)
        }
    }
}

fn apply_function_for_class_name(class: &syn::Ident) -> syn::Ident {
    syn::Ident::new(&format!("apply_style_{class}"), class.span())
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
        let visibility = &class.visibility;
        let apply_name = apply_function_for_class_name(&class.name);
        let mut apply_body = TokenStream2::new();

        for (name, value) in &class.properties {
            let make_mut_name = syn::Ident::new(&format!("make_{name}_mut"), name.span());

            apply_body.extend(quote! {
                *current.#make_mut_name() = #value;
            });
        }

        result.extend(quote! {
            #visibility fn #apply_name(current: &mut crate::style::ComputedStyle) {
                #apply_body
            }
        })
    }

    result.into()
}

struct ApplyStyleInput {
    target: syn::Expr,
    name: syn::Ident,
}

impl ApplyStyleInput {
    fn parse(buffer: ParseStream, ctx: &mut ParseContext) -> Result<Self, AlreadyReported> {
        Ok(Self {
            target: {
                let target = buffer.parse::<syn::Expr>().report_in(ctx)?;
                buffer.parse::<Token![,]>().report_in(ctx)?;
                target
            },
            name: buffer.parse::<syn::Ident>().report_in(ctx)?,
        })
    }
}

pub fn test_apply_style(ts: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = {
        let mut parse_ctx = ParseContext::new();
        let Ok(input) = parse_ctx.parse2(ApplyStyleInput::parse, ts.into()) else {
            return parse_ctx.into_error_stream().into();
        };
        input
    };

    let apply_name = apply_function_for_class_name(&input.name);
    let target = input.target;

    quote! { #apply_name(#target) }.into()
}
