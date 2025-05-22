use std::{
    collections::{HashMap, HashSet},
    ops::{Range, RangeFrom},
    rc::Rc,
    str::FromStr,
    sync::Once,
};

use quote::{ToTokens, quote};
use syn::{
    Token,
    ext::IdentExt,
    parse::{Lookahead1, ParseStream},
    parse_quote,
    punctuated::Punctuated,
    spanned::Spanned,
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
            #(#idents(CssWideKeywordOr<pst::#idents>),)*
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

impl syn::parse::Parse for Keyword {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Punctuated::parse_separated_nonempty_with(input, syn::Ident::parse_any).map(Self)
    }
}

impl Keyword {
    fn to_pascal_case(&self) -> syn::Ident {
        let mut result = String::new();
        for ident in &self.0 {
            let value = ident.to_string();
            let mut it = value.chars();
            if let Some(first) = it.next() {
                result.extend(first.to_uppercase());
            }
            result.push_str(it.as_str());
        }
        syn::Ident::new(&result, self.0.span())
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
enum BasicPlaceholder {
    Length,
    Percentage,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
enum Placeholder {
    Basic(BasicPlaceholder),
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
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

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct Combination {
    combinator: Combinator,
    contents: Vec<ValueGrammar>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
enum GrammarToken {
    Keyword(Keyword),
    BasicPlaceholder(BasicPlaceholder),
    Delimiter(char),
}

impl GrammarToken {
    fn to_token_type(&self) -> syn::Type {
        match self {
            GrammarToken::Keyword(Keyword(keyword)) => parse_quote! { Token![#keyword] },
            GrammarToken::BasicPlaceholder(basic_placeholder) => todo!(),
            GrammarToken::Delimiter(delim) => parse_quote! { Token![#delim] },
        }
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
enum CountMultiplier {
    Exactly(usize),
    Range(usize, usize),
    UnboundedRange(usize),
}

impl CountMultiplier {
    fn requires_at_least_one(self) -> bool {
        match self {
            CountMultiplier::Exactly(1..) => true,
            CountMultiplier::Range(1.., _) => true,
            CountMultiplier::UnboundedRange(1..) => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
enum VecMultiplier {
    ZeroOrMore,
    OneOrMore,
    Count(CountMultiplier),
    CommaSeparated(Option<CountMultiplier>),
}

impl VecMultiplier {
    fn requires_at_least_one(self) -> bool {
        match self {
            VecMultiplier::OneOrMore => true,
            VecMultiplier::Count(mul) => mul.requires_at_least_one(),
            VecMultiplier::CommaSeparated(mul) => mul.is_none_or(|mul| mul.requires_at_least_one()),
            VecMultiplier::ZeroOrMore => false,
        }
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
enum Multiplier {
    Vec(VecMultiplier),
    OneOrZero,
    Required,
}

impl Multiplier {
    fn parse(
        stream: ParseStream,
        _ctx: &mut ParseContext,
    ) -> Result<Option<Self>, AlreadyReported> {
        let lk = stream.lookahead1();
        Ok(if lk.peek(Token![?]) {
            stream.parse::<Token![?]>().unwrap();
            Some(Multiplier::OneOrZero)
        } else {
            None
        })
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
// https://drafts.csswg.org/css-values-4/#value-defs
enum ValueGrammar {
    Token(GrammarToken),
    NonTerminal(Keyword),
    Multiplied(Box<ValueGrammar>, Multiplier),
    Property(syn::LitStr),
    Combinator(Combination),
}

impl ValueGrammar {
    fn parse_leaf(
        stream: ParseStream,
        ctx: &mut ParseContext,
        lk: &Lookahead1,
    ) -> Result<Option<Self>, AlreadyReported> {
        let mut result = if lk.peek(syn::Ident::peek_any) {
            ValueGrammar::Token(GrammarToken::Keyword(Keyword(
                Punctuated::parse_separated_nonempty_with(stream, syn::Ident::parse_any).unwrap(),
            )))
        } else if lk.peek(Token![<]) {
            stream.parse::<Token![<]>().unwrap();
            let Ok(keyword) = stream.parse::<Keyword>().report_in(ctx) else {
                advance_past_punct(stream, '>');
                return Err(AlreadyReported);
            };
            stream.parse::<Token![>]>().report_in(ctx)?;
            ValueGrammar::NonTerminal(keyword)
        } else if lk.peek(Token![/]) {
            stream.parse::<Token![/]>().unwrap();
            ValueGrammar::Token(GrammarToken::Delimiter('/'))
        } else if lk.peek(Token![,]) {
            stream.parse::<Token![,]>().unwrap();
            ValueGrammar::Token(GrammarToken::Delimiter(','))
        } else if lk.peek(Token![:]) {
            stream.parse::<Token![:]>().unwrap();
            ValueGrammar::Token(GrammarToken::Delimiter(':'))
        } else if lk.peek(Token![;]) {
            stream.parse::<Token![;]>().unwrap();
            ValueGrammar::Token(GrammarToken::Delimiter(';'))
        } else if lk.peek(syn::LitChar) {
            ValueGrammar::Token(GrammarToken::Delimiter(
                stream.parse::<syn::LitChar>().unwrap().value(),
            ))
        } else if lk.peek(syn::token::Bracket) {
            let (inner, _) = wrap_syn_group_macro!(syn::bracketed in stream).unwrap();
            Self::parse_level(&inner, ctx, u32::MAX)?
        } else {
            return Ok(None);
        };

        if let Some(multiplier) = Multiplier::parse(stream, ctx)? {
            result = ValueGrammar::Multiplied(Box::new(result), multiplier);
        }

        Ok(Some(result))
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
                    ValueGrammar::Token(GrammarToken::Delimiter(char::REPLACEMENT_CHARACTER))
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

#[derive(Debug)]
enum ParserNode {
    Token(GrammarToken),
    Enum(ParserEnum),
    Struct(ParserStruct),
    Vec(ParserVec),
    Option(ParserOption),
}

#[derive(Debug)]
struct ParserAdtState {
    generated_parse: Once,
    generated_decl: Once,
}

impl ParserAdtState {
    fn new() -> Self {
        Self {
            generated_parse: Once::new(),
            generated_decl: Once::new(),
        }
    }
}

impl ParserNode {
    fn to_type_name(&self) -> syn::Type {
        match self {
            ParserNode::Token(token) => token.to_token_type(),
            ParserNode::Enum(ParserEnum { name, .. }) => {
                parse_quote! { #name }
            }
            ParserNode::Struct(ParserStruct { name, .. }) => {
                parse_quote! { #name }
            }
            Self::Vec(ParserVec { item, .. }) => {
                let ty = item.to_type_name();
                parse_quote! { Vec<#ty> }
            }
            ParserNode::Option(ParserOption { inner }) => {
                let ty = inner.to_type_name();
                parse_quote! { Option<#ty> }
            }
        }
    }

    fn name(&self) -> syn::Ident {
        match self {
            ParserNode::Token(token) => match token {
                GrammarToken::Keyword(keyword) => keyword.to_pascal_case(),
                GrammarToken::BasicPlaceholder(_) => todo!(),
                GrammarToken::Delimiter(c) => match c {
                    ',' => syn::Ident::new("Comma", Span2::call_site()),
                    '/' => syn::Ident::new("Slash", Span2::call_site()),
                    _ => todo!("heuristic name for token {token:?}"),
                },
            },
            ParserNode::Enum(enum_) => enum_.name.clone(),
            ParserNode::Struct(struct_) => struct_.name.clone(),
            ParserNode::Vec(vec_) => vec_.item.name(),
            ParserNode::Option(opt) => opt.inner.name(),
        }
    }
}

#[derive(Debug)]
struct ParserEnum {
    name: syn::Ident,
    variants: Vec<(syn::Ident, Rc<ParserNode>)>,
    state: ParserAdtState,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
enum StructCombinator {
    Juxtaposition,
    Any,
    All,
}

#[derive(Debug)]
struct ParserStruct {
    name: syn::Ident,
    combinator: StructCombinator,
    fields: Vec<Rc<ParserNode>>,
    state: ParserAdtState,
}

#[derive(Debug)]
struct ParserVec {
    item: Rc<ParserNode>,
    multiplier: VecMultiplier,
}

#[derive(Debug)]
struct ParserOption {
    inner: Rc<ParserNode>,
}

struct ParserGenerator {
    output: TokenStream2,
    non_terminals: HashMap<Keyword, Rc<ParserNode>>,
    node_for_unnamed: HashMap<ValueGrammar, Rc<ParserNode>>,
    next_unnamed_type_index: usize,
}

impl ParserGenerator {
    fn new() -> Self {
        Self {
            output: TokenStream2::new(),
            non_terminals: HashMap::new(),
            node_for_unnamed: HashMap::new(),
            next_unnamed_type_index: 0,
        }
    }

    fn possible_first_tokens_rec(&self, grammar: &ParserNode, output: &mut HashSet<GrammarToken>) {
        match grammar {
            ParserNode::Token(value) => _ = output.insert(value.clone()),
            ParserNode::Enum(ParserEnum {
                name: _,
                variants,
                state: _,
            }) => {
                for (_, child) in variants {
                    self.possible_first_tokens_rec(child, output);
                }
            }
            ParserNode::Struct(ParserStruct {
                name: _,
                combinator,
                fields,
                state: _,
            }) => match combinator {
                StructCombinator::Any => {
                    for child in fields {
                        self.possible_first_tokens_rec(child, output)
                    }
                }
                StructCombinator::Juxtaposition | StructCombinator::All => {
                    self.possible_first_tokens_rec(&fields[0], output)
                }
            },
            ParserNode::Vec(ParserVec { item, multiplier }) => {
                if !multiplier.requires_at_least_one() {
                    panic!("LL(2+) grammar")
                }

                self.possible_first_tokens_rec(item, output);
            }
            ParserNode::Option(ParserOption { .. }) => {
                panic!("LL(2) grammar")
            }
        }
    }

    fn possible_first_tokens(&self, grammar: &ParserNode) -> HashSet<GrammarToken> {
        let mut out = HashSet::new();
        self.possible_first_tokens_rec(grammar, &mut out);
        out
    }

    fn unnamed_pascal_ident_for(&mut self, i: usize) -> syn::Ident {
        syn::Ident::new(&format!("Unnamed{i}"), Span2::call_site())
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
    ) -> Rc<ParserNode> {
        match grammar {
            ValueGrammar::Token(token) => Rc::new(ParserNode::Token(token.clone())),
            ValueGrammar::NonTerminal(ident) => self.non_terminals[ident].clone(),
            &ValueGrammar::Multiplied(ref inner, multiplier) => match multiplier {
                Multiplier::Vec(mul) => Rc::new(ParserNode::Vec(ParserVec {
                    item: self.generate_structure_for(None, inner),
                    multiplier: mul,
                })),
                Multiplier::OneOrZero => Rc::new(ParserNode::Option(ParserOption {
                    inner: self.generate_structure_for(None, inner),
                })),
                Multiplier::Required => todo!(),
            },
            ValueGrammar::Property(property) => todo!(),
            ValueGrammar::Combinator(combination) => {
                let (name, named) = match name {
                    Some(name) => (name.clone(), true),
                    None => {
                        if let Some(node) = self.node_for_unnamed.get(grammar) {
                            return node.clone();
                        } else {
                            (self.next_unnamed_pascal_ident(), false)
                        }
                    }
                };

                let mut contents = Vec::new();

                for child in &combination.contents {
                    match combination.combinator {
                        Combinator::Juxtaposition | Combinator::All => {
                            contents.push(self.generate_structure_for(None, child));
                        }
                        Combinator::Either => {
                            contents.push(self.generate_structure_for(None, child));
                        }
                        Combinator::Any => {
                            contents.push(self.generate_structure_for(None, child));
                        }
                    }
                }

                let node = Rc::new(match combination.combinator {
                    Combinator::Either => ParserNode::Enum(ParserEnum {
                        name,
                        variants: contents
                            .into_iter()
                            .map(|node| (node.name(), node))
                            .collect(),
                        state: ParserAdtState::new(),
                    }),
                    Combinator::Juxtaposition => ParserNode::Struct(ParserStruct {
                        name,
                        combinator: StructCombinator::Juxtaposition,
                        fields: contents,
                        state: ParserAdtState::new(),
                    }),
                    Combinator::Any => ParserNode::Struct(ParserStruct {
                        name,
                        combinator: StructCombinator::Any,
                        fields: contents,
                        state: ParserAdtState::new(),
                    }),
                    Combinator::All => ParserNode::Struct(ParserStruct {
                        name,
                        combinator: StructCombinator::All,
                        fields: contents,
                        state: ParserAdtState::new(),
                    }),
                });

                if !named {
                    self.node_for_unnamed.insert(grammar.clone(), node.clone());
                }

                node
            }
        }
    }

    fn generate_decls_for(&mut self, node: &ParserNode) {
        match node {
            ParserNode::Token(_) | ParserNode::Vec(_) | ParserNode::Option(_) => (),
            ParserNode::Enum(parser_enum) => {
                parser_enum.state.generated_decl.call_once(|| {
                    let mut inner = TokenStream2::new();
                    for (name, node) in &parser_enum.variants {
                        let node_typename = node.to_type_name();
                        self.generate_decls_for(node);
                        inner.extend(quote! { #name(#node_typename), });
                    }

                    let name = &parser_enum.name;
                    self.output.extend(quote! {
                        #[derive(Debug, Clone, PartialEq, Eq)]
                        pub enum #name {
                            #inner
                        }
                    });
                });
            }
            ParserNode::Struct(parser_struct) => {
                parser_struct.state.generated_decl.call_once(|| {
                    let mut inner = TokenStream2::new();
                    for node in &parser_struct.fields {
                        let node_typename = node.to_type_name();
                        self.generate_decls_for(node);
                        inner.extend(match parser_struct.combinator {
                            StructCombinator::All | StructCombinator::Juxtaposition => {
                                quote! { pub #node_typename, }
                            }
                            StructCombinator::Any => quote! { pub Option<#node_typename>, },
                        })
                    }

                    let name = &parser_struct.name;
                    self.output.extend(quote! {
                        #[derive(Debug, Clone, PartialEq, Eq)]
                        pub struct #name(#inner);
                    });
                })
            }
        }
    }

    fn ensure_no_next_token_ambiguity(
        &mut self,
        grammars: impl IntoIterator<Item = impl AsRef<ParserNode>>,
    ) {
        let mut sets = Vec::new();
        for grammar in grammars {
            sets.push(self.possible_first_tokens(grammar.as_ref()));
        }

        for set1 in &sets {
            for set2 in &sets {
                if std::ptr::eq(set1, set2) {
                    continue;
                }

                if set1.intersection(set2).next().is_some() {
                    panic!("Parser ambiguity encountered!")
                }
            }
        }
    }

    fn generate_parser_impl(&mut self, node: &ParserNode) -> TokenStream2 {
        match node {
            ParserNode::Token(token) => {
                let token_type = token.to_token_type();
                quote! { stream.parse::<#token_type>() }
            }
            ParserNode::Enum(ParserEnum {
                name,
                variants,
                state,
            }) => {
                state.generated_parse.call_once(|| {
                    self.ensure_no_next_token_ambiguity(variants.iter().map(|(_, c)| c));
                    let mut out = TokenStream2::new();

                    for (variant_name, child) in variants {
                        let tokens = self
                            .possible_first_tokens(child)
                            .into_iter()
                            .map(|token| token.to_token_type());
                        let child_expr = self.generate_parser_impl(child);

                        out.extend(quote! {
                            if #(lk.peek::<#tokens>())||* {
                                #name::#variant_name(#child_expr?)
                            } else
                        })
                    }

                    self.output.extend(quote! {
                        impl Parse<'_> for #name {
                            fn parse(stream: &mut ParseStream) -> Result<Self, ParseError> {
                                stream.skip_whitespace();
                                let mut lk = stream.lookahead1();
                                Ok({
                                    let result = #out {
                                        return Err(lk.error())
                                    };
                                    stream.skip_whitespace();
                                    result
                                })
                            }
                        }
                    });
                });

                quote! { #name::parse(stream) }
            }
            ParserNode::Struct(ParserStruct {
                name,
                combinator,
                fields,
                state,
            }) => {
                state.generated_parse.call_once(|| {
                    match combinator {
                        StructCombinator::Juxtaposition => {
                            self.ensure_no_next_token_ambiguity(fields);
                            let exprs = fields
                                .iter()
                                .map(|child| self.generate_parser_impl(child))
                                .collect::<Vec<_>>();

                            self.output.extend(quote! {
                                impl Parse<'_> for #name {
                                    fn parse(stream: &mut ParseStream) -> Result<Self, ParseError> {
                                        stream.skip_whitespace();
                                        Ok({
                                            let result = Self(#(#exprs?,)*);
                                            stream.skip_whitespace();
                                            result
                                        })
                                    }
                                }
                            })
                        }
                        StructCombinator::Any => {
                            self.ensure_no_next_token_ambiguity(fields);
                            let mut out = TokenStream2::new();
                            // HACK: no
                            let mut lkout = TokenStream2::new();

                            for (i, child) in fields.iter().enumerate() {
                                let tokens = self
                                    .possible_first_tokens(child)
                                    .into_iter()
                                    .map(|token| token.to_token_type())
                                    .collect::<Vec<_>>();
                                let child_expr = self.generate_parser_impl(child);
                                let f = syn::Index::from(i);

                                out.extend(quote! {
                                    if #(lk.peek::<#tokens>())||* {
                                        result.#f = Some(#child_expr?);
                                        stream.skip_whitespace();
                                        continue;
                                    } else
                                });

                                lkout.extend(quote! {
                                    #(lk.peek::<#tokens>();)*
                                });
                            }

                            let nones = fields.iter().map(|_| quote! { None });
                            let is_none = fields.iter().enumerate().map(|(i, _)| {
                                let f = syn::Index::from(i);
                                quote! { result.#f.is_none() }
                            });

                            self.output.extend(quote! {
                                impl Parse<'_> for #name {
                                    fn parse(stream: &mut ParseStream) -> Result<Self, ParseError> {
                                        stream.skip_whitespace();
                                        let mut result = Self(#(#nones,)*);

                                        while !stream.is_empty() {
                                            let mut lk = stream.lookahead1();
                                            #out {
                                                return Err(lk.error())
                                            }
                                        }

                                        if #(#is_none)&&* {
                                            let mut lk = stream.lookahead1();
                                            #lkout
                                            return Err(lk.error())
                                        }

                                        stream.skip_whitespace();
                                        Ok(result)
                                    }
                                }
                            })
                        }
                        StructCombinator::All => {
                            self.ensure_no_next_token_ambiguity(fields);
                        }
                    }
                });

                quote! { #name::parse(stream) }
            }
            ParserNode::Vec(_) => todo!("parse parservec"),
            ParserNode::Option(ParserOption { inner }) => {
                let tokens = self
                    .possible_first_tokens(inner)
                    .into_iter()
                    .map(|x| x.to_token_type());
                let child_expr = self.generate_parser_impl(inner);

                quote! {
                    if #(lk.peek::<#tokens>())||* {
                        Some(#child_expr?)
                    } else
                }
            }
        }
    }

    fn generate_parser_for(&mut self, name: syn::Ident, grammar: &ValueGrammar) {
        let node = self.generate_structure_for(Some(&name), grammar);

        self.generate_decls_for(&node);

        self.generate_parser_impl(&node);
    }

    fn insert_non_terminal(&mut self, name: Keyword, grammar: &ValueGrammar) {
        let node = self.generate_structure_for(Some(&name.to_pascal_case()), grammar);
        self.non_terminals.insert(name, node.clone());
        self.generate_decls_for(&node);
    }
}

struct ParserGeneratorInput {
    properties: Vec<(syn::Ident, ValueGrammar)>,
    non_terminals: Vec<(Keyword, ValueGrammar)>,
}

impl ParserGeneratorInput {
    fn parse(stream: ParseStream, ctx: &mut ParseContext) -> Result<Self, AlreadyReported> {
        let mut errored = false;
        let mut result = Self {
            properties: Vec::new(),
            non_terminals: Vec::new(),
        };

        while !stream.is_empty() {
            enum Lhs {
                NonTerminal(Keyword),
                Property(syn::Ident),
            }

            let lk = stream.lookahead1();
            let lhs = if lk.peek(Token![<]) {
                stream.parse::<Token![<]>().unwrap();
                let Ok(name) = stream
                    .parse::<Keyword>()
                    .and_then(|keyword| {
                        stream.parse::<Token![>]>()?;
                        Ok(keyword)
                    })
                    .report_in_and_set(ctx, &mut errored)
                else {
                    advance_past_punct(stream, ';');
                    continue;
                };

                Lhs::NonTerminal(name)
            } else if lk.peek(syn::Ident) {
                let Ok(name) = stream
                    .parse::<syn::Ident>()
                    .report_in_and_set(ctx, &mut errored)
                else {
                    advance_past_punct(stream, ';');
                    continue;
                };

                Lhs::Property(name)
            } else {
                ctx.report(lk.error());
                errored = true;
                advance_past_punct(stream, ';');
                continue;
            };

            let Ok(_) = stream
                .parse::<Token![=]>()
                .report_in_and_set(ctx, &mut errored)
            else {
                advance_past_punct(stream, ';');
                continue;
            };

            let Ok(grammar) = wrap_syn_group_macro!(syn::braced in stream)
                .report_in_and_set(ctx, &mut errored)
                .and_then(|(inner, _)| ValueGrammar::parse(&inner, ctx))
            else {
                errored = true;
                advance_past_punct(stream, ';');
                continue;
            };

            match lhs {
                Lhs::NonTerminal(name) => {
                    result.non_terminals.push((name, grammar));
                }
                Lhs::Property(name) => {
                    result.properties.push((name, grammar));
                }
            }

            errored |= stream.parse::<Token![;]>().report_in(ctx).is_err();
        }

        if errored {
            Err(AlreadyReported)
        } else {
            Ok(result)
        }
    }
}

pub fn make_css_value_parser_impl(ts: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = {
        let mut parse_ctx = ParseContext::new();
        let Ok(input) = parse_ctx.parse2(ParserGeneratorInput::parse, ts.into()) else {
            return parse_ctx.into_error_stream().into();
        };
        input
    };

    let mut generator = ParserGenerator::new();
    for (name, grammar) in input.non_terminals {
        generator.insert_non_terminal(name, &grammar);
    }
    for (name, grammar) in input.properties {
        generator.generate_parser_for(name, &grammar);
    }
    generator.output.into()
}

#[cfg(test)]
mod test {
    use crate::parse::{AlreadyReported, ParseContext, Span2};

    use super::{Combination, Combinator, GrammarToken, Keyword, ValueGrammar};

    impl ValueGrammar {
        fn test_keyword(text: &str) -> Self {
            Self::Token(GrammarToken::Keyword(Keyword({
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
    fn punctuated_keyword() {
        assert_eq!(
            test_parse("a-b-c"),
            ValueGrammar::Token(GrammarToken::Keyword(Keyword(syn::parse_quote!(a - b - c))))
        )
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
