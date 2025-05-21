use quote::quote;
use syn::{Token, parse::ParseStream, spanned::Spanned};

mod parse;
use parse::*;

fn parse_yes_no(stream: ParseStream) -> syn::Result<bool> {
    let lookahead1 = stream.lookahead1();
    let lk = lookahead1;
    if lk.peek(kw::yes) {
        stream.parse::<kw::yes>()?;
        Ok(true)
    } else if lk.peek(kw::no) {
        stream.parse::<kw::no>()?;
        Ok(false)
    } else {
        Err(lk.error())
    }
}

#[derive(Debug, Clone)]
struct Property {
    name: syn::Ident,
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

        let skip_past_comma = || loop {
            match buffer.parse::<TokenTree2>() {
                Ok(TokenTree2::Punct(punct)) if punct.as_char() == ',' => {
                    break;
                }
                Err(_) => break,
                _ => (),
            }
        };

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
                skip_past_comma();
                continue;
            }

            let Ok(name) = buffer.parse::<syn::Ident>().report_in(ctx) else {
                errored = true;
                skip_past_comma();
                continue;
            };

            let Ok(_) = buffer.parse::<Token![:]>().report_in(ctx) else {
                errored = true;
                skip_past_comma();
                continue;
            };

            let Ok(type_) = buffer.parse::<syn::Type>().report_in(ctx) else {
                errored = true;
                skip_past_comma();
                continue;
            };

            let default = if buffer.parse::<Token![=]>().is_ok() {
                let Ok(default) = buffer.parse::<syn::Expr>().report_in(ctx) else {
                    errored = true;
                    skip_past_comma();
                    continue;
                };

                Some(default)
            } else {
                None
            };

            result.push(Property {
                name,
                append: append.unwrap_or(matches!(type_, syn::Type::Slice(..))),
                inherit,
                copy,
                value_type: type_,
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
    syn::custom_keyword!(yes);
    syn::custom_keyword!(no);
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

            let Ok((inner, _)) = (|| {
                let inner;
                let brace = syn::braced!(inner in buffer);
                Ok((inner, brace))
            })()
            .report_in_and_set(ctx, &mut errored) else {
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
            syn::Type::Slice(slice) => syn::Type::Path(syn::parse_quote!(::std::rc::Rc<#slice>)),
            _ => self.value_type.clone(),
        }
    }

    fn effective_default(&self) -> syn::Expr {
        if let Some(default) = &self.default {
            default.clone()
        } else {
            let type_ = self.rcified_type();
            syn::parse_quote! { <#type_ as ::std::default::Default>::default() }
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

#[proc_macro]
pub fn implement_style_module(ts: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = {
        let mut parse_ctx = ParseContext::new();
        let Ok(input) = parse_ctx.parse2(Input::parse, ts.into()) else {
            return parse_ctx.into_error_stream().into();
        };
        input
    };

    let mut result = TokenStream2::new();

    for prop in input.groups.iter().flat_map(|g| g.properties.iter()) {
        let pascal_name = snake_case_to_pascal_case(&prop.name);
        let type_ = prop.rcified_type();
        result.extend(quote! {
            pub struct #pascal_name(#type_);

            impl StyleValue for #pascal_name {
                type Inner = #type_;
            }
        });
    }

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

            impl ::std::default::Default for #group_type_name {
                fn default() -> Self {
                    Self {
                        #default_result
                    }
                }
            }
        });
    }

    let mut computed_style_fields = TokenStream2::new();
    let mut computed_style_impl = TokenStream2::new();
    let mut create_child_impl = TokenStream2::new();
    // PERF: Currently `ComputedStyle::apply_all` optimizes for amount of `Rc::make_mut` calls
    //       instead of for amount of `StyleMap` lookups. Maybe the other way ends up being
    //       better because `Rc::make_mut` on an already unique `Rc` should be very cheap but
    //       `StyleMap` lookups are always `HashMap` lookups.
    //       On this scale it probably doesn't matter much yet though.
    let mut apply_all_impl = TokenStream2::new();
    for group in &input.groups {
        let group_name = &group.name;
        let group_type_name = snake_case_to_pascal_case(&group.name);

        computed_style_fields.extend(quote! {
            #group_name: ::std::rc::Rc<#group_type_name>,
        });

        let inherit_whole_group = group.properties.iter().all(|prop| prop.inherit);
        let mut group_apply_vars = TokenStream2::new();
        let mut group_apply_block = TokenStream2::new();
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
                    &mut ::std::rc::Rc::make_mut(&mut self.#group_name).#name
                }
            });

            if !inherit_whole_group {
                if prop.inherit {
                    group_create_child_impl.extend(quote! {
                        #name: self.#group_name.#name.clone(),
                    })
                } else {
                    let default = prop.effective_default();
                    group_create_child_impl.extend(quote! { #name: #default, })
                }
            }

            group_apply_vars.extend(quote! {
                let #name = map.get::<#type_name>();
            });

            group_apply_block.extend(quote! {
                if let Some(value) = #name
            });

            if prop.append {
                group_apply_block.extend(quote! { {
                    let inherited = group.#name.iter();
                    let new = value.iter();
                    group.#name = inherited.chain(new).cloned().collect();
                } })
            } else {
                group_apply_block.extend(quote! { {
                    group.#name = ::std::clone::Clone::clone(value);
                } })
            }
        }

        let prop_names = group.properties.iter().map(|prop| &prop.name);
        group_apply_vars.extend(quote! {
            if #(#prop_names.is_some())||* {
                let mut group = ::std::rc::Rc::make_mut(&mut self.#group_name);

                #group_apply_block
            }
        });

        if inherit_whole_group {
            create_child_impl.extend(quote! { #group_name: self.#group_name.clone(), });
        } else {
            create_child_impl.extend(quote! {
                #group_name: ::std::rc::Rc::new(#group_type_name {
                    #group_create_child_impl
                }),
            });
        }

        apply_all_impl.extend(quote! { { #group_apply_vars } });
    }

    result.extend(quote! {
        #[derive(Default, Debug, Clone)]
        pub struct ComputedStyle {
            #computed_style_fields
        }

        impl ComputedStyle {
            #computed_style_impl

            pub fn apply_all(&mut self, map: &StyleMap) {
                #apply_all_impl
            }

            pub fn create_child(&self) -> Self {
                Self {
                    #create_child_impl
                }
            }
        }
    });

    result.into()
}
