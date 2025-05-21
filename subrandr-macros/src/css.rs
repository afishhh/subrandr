use std::{collections::HashSet, str::FromStr};

use quote::{ToTokens, quote};
use syn::{
    Token,
    parse::{Lookahead1, ParseStream},
    punctuated::Punctuated,
};

use crate::{common::advance_past_punct, parse::*};

#[derive(Debug, Clone)]
struct MakePropertyListInput {
    properties: Vec<(syn::LitStr, syn::Ident)>,
}

impl MakePropertyListInput {
    fn parse(buffer: ParseStream, ctx: &mut ParseContext) -> Result<Self, AlreadyReported> {
        let mut errored = false;
        let mut result = Self {
            properties: Vec::new(),
        };

        while !buffer.is_empty() {
            let Ok(css_name) = buffer
                .parse::<syn::LitStr>()
                .report_in_and_set(ctx, &mut errored)
            else {
                advance_past_punct(buffer, ';');
                continue;
            };

            let Ok(ident) = buffer
                .parse::<syn::Ident>()
                .report_in_and_set(ctx, &mut errored)
            else {
                advance_past_punct(buffer, ';');
                continue;
            };

            result.properties.push((css_name, ident));

            errored |= buffer.parse::<Token![;]>().report_in(ctx).is_err();
        }

        if errored {
            Err(AlreadyReported)
        } else {
            Ok(result)
        }
    }
}

pub fn make_css_property_parser_list_impl(ts: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let MakePropertyListInput { mut properties } = {
        let mut parse_ctx = ParseContext::new();
        let Ok(input) = parse_ctx.parse2(MakePropertyListInput::parse, ts.into()) else {
            return parse_ctx.into_error_stream().into();
        };
        input
    };

    let mut result = TokenStream2::new();

    properties.sort_by_cached_key(|(name, _)| name.value());

    let idents = properties.iter().map(|(_, ident)| ident);
    result.extend(quote! {
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub enum AnyPropertyValue {
            #(#idents(CssWideKeywordOr<#idents>),)*
        }
    });

    let values = properties.iter().map(|(css_name, ident)| {
        quote! { (#css_name, (|stream| Ok(AnyPropertyValue::#ident(stream.parse()?))) as PropertyValueParserFn) }
    });

    result.extend(quote! {
        const PROPERTY_LIST: &[(&str, PropertyValueParserFn)] = &[
            #(#values,)*
        ];
    });

    result.into()
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct Keyword(Punctuated<syn::Ident, Token![-]>);

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
enum BasicPlaceholder {
    Length,
    Percentage,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
enum Placeholder {
    Basic(BasicPlaceholder),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Combinator {
    Juxtaposition,
    Either,
    Any,
    All,
}

impl Combinator {
    fn precedence(self) -> u32 {
        match self {
            Self::Either => 3,
            Self::Any => 2,
            Self::All => 1,
            Self::Juxtaposition => 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Combination {
    combinator: Combinator,
    contents: Vec<ValueGrammar>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
enum ValueToken {
    Keyword(Keyword),
    BasicPlaceholder(BasicPlaceholder),
    Delimiter(char),
}

#[derive(Debug, Clone, PartialEq, Eq)]
// https://drafts.csswg.org/css-values-4/#value-defs
enum ValueGrammar {
    Token(ValueToken),
    NonTerminal(syn::Ident),
    Property(syn::LitStr),
    Combinator(Combination),
}

impl ValueGrammar {
    fn parse_leaf(
        stream: ParseStream,
        ctx: &mut ParseContext,
        lk: &Lookahead1,
    ) -> Result<Option<Self>, AlreadyReported> {
        Ok(Some(if lk.peek(syn::Ident) {
            ValueGrammar::Token(ValueToken::Keyword(Keyword(
                Punctuated::parse_separated_nonempty(stream).unwrap(),
            )))
        } else if lk.peek(Token![<]) {
            todo!()
        } else if lk.peek(Token![/]) {
            stream.parse::<Token![/]>().unwrap();
            ValueGrammar::Token(ValueToken::Delimiter('/'))
        } else if lk.peek(Token![,]) {
            stream.parse::<Token![,]>().unwrap();
            ValueGrammar::Token(ValueToken::Delimiter(','))
        } else if lk.peek(Token![:]) {
            stream.parse::<Token![:]>().unwrap();
            ValueGrammar::Token(ValueToken::Delimiter(':'))
        } else if lk.peek(Token![;]) {
            stream.parse::<Token![;]>().unwrap();
            ValueGrammar::Token(ValueToken::Delimiter(';'))
        } else if lk.peek(syn::LitChar) {
            ValueGrammar::Token(ValueToken::Delimiter(
                stream.parse::<syn::LitChar>().unwrap().value(),
            ))
        } else if lk.peek(syn::token::Bracket) {
            let (inner, _) = wrap_syn_group_macro!(syn::bracketed in stream).unwrap();
            Self::parse_level(&inner, ctx, u32::MAX)?
        } else {
            return Ok(None);
        }))
    }

    fn parse_level(
        stream: ParseStream,
        ctx: &mut ParseContext,
        level: u32,
    ) -> Result<Self, AlreadyReported> {
        let mut errored = false;

        let mut result = {
            let lk = stream.lookahead1();

            match Self::parse_leaf(stream, ctx, &lk) {
                Ok(Some(leaf)) => leaf,
                Ok(None) => {
                    ctx.report(lk.error());
                    return Err(AlreadyReported);
                }
                Err(AlreadyReported) => {
                    errored = true;
                    // Construct dummy value just so we can continue parsing,
                    // it will get discarded at the end anyway.
                    ValueGrammar::Token(ValueToken::Delimiter(char::REPLACEMENT_CHARACTER))
                }
            }
        };

        loop {
            let lk = stream.lookahead1();
            let combinator = if lk.peek(Token![&&]) {
                Combinator::All
            } else if lk.peek(Token![||]) {
                Combinator::Any
            } else if lk.peek(Token![|]) {
                Combinator::Either
            } else if lk.peek(syn::parse::End) {
                break;
            } else {
                Combinator::Juxtaposition
            };

            if combinator.precedence() < level {
                match combinator {
                    Combinator::All => _ = stream.parse::<Token![&&]>().unwrap(),
                    Combinator::Any => _ = stream.parse::<Token![||]>().unwrap(),
                    Combinator::Either => _ = stream.parse::<Token![|]>().unwrap(),
                    Combinator::Juxtaposition => {}
                };

                let next = Self::parse_level(stream, ctx, combinator.precedence())?;
                match &mut result {
                    ValueGrammar::Combinator(combination)
                        if combination.combinator == combinator =>
                    {
                        combination.contents.push(next)
                    }
                    _ => {
                        result = ValueGrammar::Combinator(Combination {
                            combinator,
                            contents: vec![result, next],
                        });
                    }
                }
            } else {
                break;
            }
        }

        if errored {
            Err(AlreadyReported)
        } else {
            Ok(result)
        }
    }

    fn parse(stream: ParseStream, ctx: &mut ParseContext) -> Result<Self, AlreadyReported> {
        Self::parse_level(stream, ctx, u32::MAX)
    }
}

struct ParserGenerator {
    output: TokenStream2,
    next_unnamed_type_index: usize,
}

impl ParserGenerator {
    fn new() -> Self {
        Self {
            output: TokenStream2::new(),
            next_unnamed_type_index: 0,
        }
    }

    fn possible_first_tokens_rec(&self, grammar: &ValueGrammar, output: &mut HashSet<ValueToken>) {
        match grammar {
            ValueGrammar::Token(value) => _ = output.insert(value.clone()),
            ValueGrammar::NonTerminal(_) => todo!(),
            ValueGrammar::Property(_) => todo!(),
            ValueGrammar::Combinator(Combination {
                combinator,
                contents,
            }) => match combinator {
                Combinator::Either | Combinator::Any => {
                    for child in contents {
                        self.possible_first_tokens_rec(child, output)
                    }
                }
                Combinator::Juxtaposition | Combinator::All => {
                    self.possible_first_tokens_rec(&contents[0], output)
                }
            },
        }
    }

    fn possible_first_tokens(&self, grammar: &ValueGrammar) -> HashSet<ValueToken> {
        let mut out = HashSet::new();
        self.possible_first_tokens_rec(grammar, &mut out);
        out
    }

    fn unnamed_pascal_ident_for(&mut self, i: usize) -> syn::Ident {
        syn::Ident::new(&format!("Unnamed{}", i), Span2::call_site())
    }

    fn next_unnamed_pascal_ident(&mut self) -> syn::Ident {
        let ident = self.unnamed_pascal_ident_for(self.next_unnamed_type_index);
        self.next_unnamed_type_index += 1;
        ident
    }

    fn generate_structure_for(
        &mut self,
        name: Option<&syn::Ident>,
        grammar: &ValueGrammar,
    ) -> (bool, syn::Type) {
        match grammar {
            ValueGrammar::Token(ValueToken::Keyword(keyword)) => {
                let keyword_value = &keyword.0;
                (false, syn::parse_quote! { Token![#keyword_value] })
            }
            ValueGrammar::Token(ValueToken::BasicPlaceholder(_)) => todo!(),
            ValueGrammar::Token(ValueToken::Delimiter(delimiter)) => {
                (false, syn::parse_quote! { Token![#delimiter] })
            }
            ValueGrammar::NonTerminal(ident) => todo!(),
            ValueGrammar::Property(property) => todo!(),
            ValueGrammar::Combinator(combination) => {
                let mut inner = TokenStream2::new();

                for (i, child) in combination.contents.iter().enumerate() {
                    match combination.combinator {
                        Combinator::Juxtaposition | Combinator::All => {
                            self.generate_structure_for(None, child)
                                .1
                                .to_tokens(&mut inner);
                            inner.extend(quote! { , });
                        }
                        Combinator::Either => {
                            let ty = self.generate_structure_for(None, child).1;
                            let variant = self.unnamed_pascal_ident_for(i);
                            inner.extend(quote! { #variant(#ty), });
                        }
                        Combinator::Any => {
                            let ty = self.generate_structure_for(None, child).1;
                            inner.extend(quote! { Option<#ty>, });
                        }
                    }
                }

                let name = name
                    .cloned()
                    .unwrap_or_else(|| self.next_unnamed_pascal_ident());

                if matches!(combination.combinator, Combinator::Either) {
                    self.output.extend(quote! {
                        enum #name {
                            #inner
                        };
                    });
                } else {
                    self.output.extend(quote! {
                        struct #name(#inner);
                    });
                }

                (true, syn::parse_quote! { #name })
            }
        }
    }

    fn generate_parser_into(&self, ts: &mut TokenStream2, grammar: &ValueGrammar) {}

    fn generate_parser_for(&mut self, name: syn::Ident, grammar: &ValueGrammar) {
        let (is_structure, parsed_type) = self.generate_structure_for(Some(&name), grammar);

        if !is_structure {
            self.output.extend(quote! { struct #name(#parsed_type); });
        }
    }
}

pub fn make_css_value_parser_impl(ts: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let grammar = {
        let mut parse_ctx = ParseContext::new();
        let Ok(input) = parse_ctx.parse2(ValueGrammar::parse, ts.into()) else {
            return parse_ctx.into_error_stream().into();
        };
        input
    };

    let mut generator = ParserGenerator::new();
    generator.generate_parser_for(syn::Ident::new("Parser", Span2::call_site()), &grammar);
    generator.output.into()
}

#[cfg(test)]
mod test {
    use crate::parse::{AlreadyReported, ParseContext, Span2};

    use super::{Combination, Combinator, Keyword, ValueGrammar, ValueToken};

    impl ValueGrammar {
        fn test_keyword(text: &str) -> Self {
            Self::Token(ValueToken::Keyword(Keyword({
                let mut result = syn::punctuated::Punctuated::new();
                result.push_value(syn::Ident::new(text, Span2::call_site()));
                result
            })))
        }
    }

    fn test_parse(text: &str) -> ValueGrammar {
        let mut ctx = ParseContext::new();
        match ctx.parse_str(ValueGrammar::parse, text) {
            Ok(grammar) => grammar,
            Err(AlreadyReported) => {
                eprintln!("failed to parse {text:?} as a CSS value grammar:");
                for error in ctx.into_errors() {
                    eprintln!("{error}")
                }
                panic!()
            }
        }
    }

    #[test]
    fn simple_keyword() {
        assert_eq!(test_parse("abc"), ValueGrammar::test_keyword("abc"))
    }

    #[test]
    fn simple_either() {
        assert_eq!(
            test_parse("a | b | c"),
            ValueGrammar::Combinator(Combination {
                combinator: Combinator::Either,
                contents: vec![
                    ValueGrammar::test_keyword("a"),
                    ValueGrammar::test_keyword("b"),
                    ValueGrammar::test_keyword("c")
                ]
            })
        )
    }

    #[test]
    fn grouped_any() {
        assert_eq!(
            test_parse("a || [b || c]"),
            ValueGrammar::Combinator(Combination {
                combinator: Combinator::Any,
                contents: vec![
                    ValueGrammar::test_keyword("a"),
                    ValueGrammar::Combinator(Combination {
                        combinator: Combinator::Any,
                        contents: vec![
                            ValueGrammar::test_keyword("b"),
                            ValueGrammar::test_keyword("c"),
                        ]
                    })
                ]
            })
        )
    }

    #[test]
    fn grouping() {
        assert_eq!(
            test_parse("[ a | b ] || [ c && d ]"),
            ValueGrammar::Combinator(Combination {
                combinator: Combinator::Any,
                contents: vec![
                    ValueGrammar::Combinator(Combination {
                        combinator: Combinator::Either,
                        contents: vec![
                            ValueGrammar::test_keyword("a"),
                            ValueGrammar::test_keyword("b"),
                        ]
                    }),
                    ValueGrammar::Combinator(Combination {
                        combinator: Combinator::All,
                        contents: vec![
                            ValueGrammar::test_keyword("c"),
                            ValueGrammar::test_keyword("d"),
                        ]
                    })
                ]
            })
        );

        assert_ne!(test_parse("a || b || c"), test_parse("a || [b || c]"));
    }

    // Example from https://drafts.csswg.org/css-values-4/#component-combinators
    #[test]
    fn spec_precedence_example() {
        assert_eq!(
            test_parse("  a b   |   c ||   d &&   e f"),
            test_parse("[ a b ] | [ c || [ d && [ e f ]]]"),
        )
    }
}
