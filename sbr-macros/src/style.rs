use quote::quote;
use syn::{parse::ParseStream, spanned::Spanned, Token};

use crate::{
    common::{advance_past_punct, parse_yes_no},
    parse::*,
};

#[derive(Debug, Clone)]
struct Property {
    name: syn::Ident,
    // Assumed to be `value_type` if not provided.
    specified_type: Option<syn::Type>,
    value_type: syn::Type,
    inherit: bool,
    append: bool,
    copy: bool,
    default: Option<syn::Expr>,
}

#[derive(Debug, Clone)]
struct Group {
    name: syn::Ident,
    properties: Vec<Property>,
}

impl Group {
    fn parse_properties(
        buffer: ParseStream,
        ctx: &mut ParseContext,
        result: &mut Vec<Property>,
    ) -> Result<(), AlreadyReported> {
        let mut errored = false;

        while !buffer.is_empty() {
            let mut inherit = true;
            let mut append = None;
            let mut copy = true;

            if let Ok(attrs) = buffer.call(syn::Attribute::parse_outer) {
                for attr in attrs {
                    if attr.path().is_ident("inherit") {
                        if let Ok(value) = attr
                            .parse_args_with(parse_yes_no)
                            .report_in_and_set(ctx, &mut errored)
                        {
                            inherit = value;
                        }
                    } else if attr.path().is_ident("append") {
                        if let Ok(value) = attr
                            .parse_args_with(parse_yes_no)
                            .report_in_and_set(ctx, &mut errored)
                        {
                            append = Some(value);
                        }
                    } else if attr.path().is_ident("copy") {
                        if let Ok(value) = attr
                            .parse_args_with(parse_yes_no)
                            .report_in_and_set(ctx, &mut errored)
                        {
                            copy = value;
                        }
                    } else {
                        errored = true;
                        ctx.report(syn::Error::new(
                            attr.path().span(),
                            "unrecognized attribute",
                        ));
                    }
                }
            } else {
                errored = true;
                advance_past_punct(buffer, ',');
                continue;
            }

            let Ok(name) = buffer.parse::<syn::Ident>().report_in(ctx) else {
                errored = true;
                advance_past_punct(buffer, ',');
                continue;
            };

            let Ok(_) = buffer.parse::<Token![:]>().report_in(ctx) else {
                errored = true;
                advance_past_punct(buffer, ',');
                continue;
            };

            let Ok(first_type_) = buffer.parse::<syn::Type>().report_in(ctx) else {
                errored = true;
                advance_past_punct(buffer, ',');
                continue;
            };

            let Ok(arrow) = buffer.parse::<Option<Token![->]>>().report_in(ctx) else {
                errored = true;
                advance_past_punct(buffer, ',');
                continue;
            };

            let (specified_type, value_type) = match arrow {
                Some(_) => {
                    let Ok(value_type_) = buffer.parse::<syn::Type>().report_in(ctx) else {
                        errored = true;
                        advance_past_punct(buffer, ',');
                        continue;
                    };

                    (Some(first_type_), value_type_)
                }
                None => (None, first_type_),
            };

            let default = if buffer.parse::<Token![=]>().is_ok() {
                let Ok(default) = buffer.parse::<syn::Expr>().report_in(ctx) else {
                    errored = true;
                    advance_past_punct(buffer, ',');
                    continue;
                };

                Some(default)
            } else {
                None
            };

            result.push(Property {
                name,
                append: append.unwrap_or(matches!(value_type, syn::Type::Slice(..))),
                inherit,
                copy,
                specified_type,
                value_type,
                default,
            });

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
struct Input {
    groups: Vec<Group>,
}

mod kw {
    syn::custom_keyword!(rc);
}

impl Input {
    fn parse(buffer: ParseStream, ctx: &mut ParseContext) -> Result<Self, AlreadyReported> {
        let mut errored = false;
        let mut result = Self { groups: Vec::new() };

        while !buffer.is_empty() {
            let Ok(_) = buffer
                .parse::<kw::rc>()
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

            let mut group = Group {
                name,
                properties: Vec::new(),
            };

            if let Ok(()) = Group::parse_properties(&inner, ctx, &mut group.properties) {
                result.groups.push(group);
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

impl Property {
    fn rcified_type(&self) -> syn::Type {
        match &self.value_type {
            syn::Type::Slice(slice) => syn::Type::Path(syn::parse_quote!(::util::rc::Rc<#slice>)),
            _ => self.value_type.clone(),
        }
    }

    fn effective_default(&self) -> syn::Expr {
        if let Some(default) = &self.default {
            default.clone()
        } else {
            let type_ = self.rcified_type();
            syn::parse_quote! { <#type_>::DEFAULT }
        }
    }
}

fn snake_case_to_pascal_case(ident: &syn::Ident) -> syn::Ident {
    let mut result = String::new();
    let id = ident.to_string();
    for component in id.split("_") {
        let mut it = component.chars();
        if let Some(first) = it.next() {
            result.extend(first.to_uppercase());
        }
        result.push_str(it.as_str());
    }
    syn::Ident::new(&result, ident.span())
}

pub fn implement_style_module_impl(ts: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = {
        let mut parse_ctx = ParseContext::new();
        let Ok(input) = parse_ctx.parse2(Input::parse, ts.into()) else {
            return parse_ctx.into_error_stream().into();
        };
        input
    };

    let mut result = TokenStream2::new();

    for group in &input.groups {
        let group_type_name = snake_case_to_pascal_case(&group.name);
        let mut inner_result = TokenStream2::new();
        let mut default_result = TokenStream2::new();
        for prop in &group.properties {
            let name = &prop.name;
            let type_ = prop.rcified_type();
            inner_result.extend(quote! { #name: #type_, });
            let default_expr = prop.effective_default();
            default_result.extend(quote! { #name: #default_expr, });
        }

        result.extend(quote! {
            #[derive(Debug, Clone)]
            struct #group_type_name {
                #inner_result
            }

            impl #group_type_name {
                const DEFAULT: Self = Self {
                    #default_result
                };
            }

            impl ::std::default::Default for #group_type_name {
                fn default() -> Self {
                    Self::DEFAULT
                }
            }
        });
    }

    let mut computed_style_fields = TokenStream2::new();
    let mut computed_style_impl = TokenStream2::new();
    let mut create_child_impl = TokenStream2::new();
    let mut default_const_impl = TokenStream2::new();
    for group in &input.groups {
        let group_name = &group.name;
        let group_type_name = snake_case_to_pascal_case(&group.name);

        computed_style_fields.extend(quote! {
            #group_name: ::util::rc::Rc<#group_type_name>,
        });

        default_const_impl.extend(quote! {
            #group_name: ::util::rc_static!(<#group_type_name>::DEFAULT),
        });

        let inherit_whole_group = group.properties.iter().all(|prop| prop.inherit);
        let mut group_apply_vars = TokenStream2::new();
        let mut group_apply_fields = TokenStream2::new();
        let mut group_create_child_impl = TokenStream2::new();

        for prop in &group.properties {
            let name = &prop.name;
            let type_name = snake_case_to_pascal_case(&prop.name);
            let make_mut_name = syn::Ident::new(&format!("make_{name}_mut"), name.span());
            let type_ = &prop.value_type;
            let rc_type_ = &prop.rcified_type();

            let ampersand_if_not_copy = if prop.copy {
                None
            } else {
                Some(proc_macro2::Punct::new('&', proc_macro2::Spacing::Alone))
            };

            computed_style_impl.extend(quote! {
                pub fn #name(&self) -> #ampersand_if_not_copy #type_ {
                    #ampersand_if_not_copy self.#group_name.#name
                }

                pub fn #make_mut_name(&mut self) -> &mut #rc_type_ {
                    &mut ::util::rc::Rc::make_mut(&mut self.#group_name).#name
                }
            });

            let inherit_expr = if prop.inherit {
                syn::parse_quote! {
                    self.#group_name.#name.clone()
                }
            } else {
                prop.effective_default()
            };

            if !inherit_whole_group {
                group_create_child_impl.extend(quote! {
                    #name: #inherit_expr,
                })
            }

            group_apply_vars.extend(quote! {
                let #name = map.get::<#type_name>();
            });

            if let Some(specified_type) = prop.specified_type.as_ref() {
                group_apply_fields.extend(quote! {
                    #name: if let Some(value) = #name {
                        <#specified_type>::compute(ctx, self, value)
                    } else {
                        #inherit_expr
                    },
                })
            } else if prop.append {
                group_apply_fields.extend(quote! {
                    #name: if let Some(value) = #name {
                        let inherited = self.#group_name.#name.iter();
                        let new = value.iter();
                        inherited.chain(new).cloned().collect()
                    } else {
                        #inherit_expr
                    },
                })
            } else {
                group_apply_fields.extend(quote! {
                    #name: if let Some(value) = #name {
                        ::std::clone::Clone::clone(value)
                    } else {
                        #inherit_expr
                    },
                })
            }
        }

        if inherit_whole_group {
            create_child_impl.extend(quote! { #group_name: self.#group_name.clone(), });
        } else {
            create_child_impl.extend(quote! {
                #group_name: ::util::rc::Rc::new(#group_type_name {
                    #group_create_child_impl
                }),
            });
        }
    }

    result.extend(quote! {
        #[derive(Debug, Clone)]
        pub struct ComputedStyle {
            #computed_style_fields
        }

        impl ComputedStyle {
            pub const DEFAULT: Self = Self {
                #default_const_impl
            };

            #computed_style_impl

            pub fn create_derived(&self) -> Self {
                Self {
                    #create_child_impl
                }
            }
        }

        impl ::std::default::Default for ComputedStyle {
            fn default() -> Self {
                Self::DEFAULT
            }
        }
    });

    result.into()
}
