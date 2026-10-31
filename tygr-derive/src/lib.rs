use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::{parse_quote, Data, DataEnum, DataStruct, DeriveInput, Fields, Generics, Ident};

/// Derive `Grammar` (and `GrammarRule`) for a `struct` or `enum`.
///
/// `struct`s become a concatenation of their fields; `enum`s become an
/// alternation of their variants' own fields — see `tygr`'s crate-level
/// `Design` section for the full mapping from Rust constructs to EBNF.
///
/// `#[grammar(...)]` on the derived type is optional:
/// - `name = "..."` — override the BNF rule name (defaults to the type name).
/// - `hidden` — omit this type from BNF output; parsing/printing unaffected.
/// - `inline` — splice this type's own definition wherever it's
///   referenced, instead of a rule reference.
/// - `validated` — after a successful parse, run `Validate::validate` on the
///   value; a rejection backtracks as if the grammar hadn't matched.
#[proc_macro_derive(Grammar, attributes(grammar))]
pub fn derive_grammar(input: TokenStream) -> TokenStream {
    let mut input = syn::parse_macro_input!(input as DeriveInput);
    for param in input.generics.type_params_mut() {
        param.bounds.push(parse_quote!(::tygr::Grammar));
    }
    match impl_grammar(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// `Grammar` via `GrammarFrom` + [`FromStr`](std::str::FromStr) on the matched text.
///
/// Parses `GrammarFrom::Source`, then calls `Self::from_str` on the exact
/// substring it matched. `Err` backtracks the parse as if nothing had
/// matched, and is traced under the `trace`/`trace_pos` features.
///
/// `#[grammar(...)]` here supports `name = "..."`, `hidden`/`inline`, and
/// `validated` (as for `#[derive(Grammar)]`) — `Validate::validate` runs
/// after a successful conversion, independent of the conversion's own
/// `Err`. Since the conversion can also reject an otherwise-matching
/// `Source`, the rule's own BNF definition shows a side-condition marker.
#[proc_macro_derive(GrammarFromStr, attributes(grammar))]
pub fn derive_grammar_from_str(input: TokenStream) -> TokenStream {
    derive_convert(input, Convert::FromStr)
}

/// `Grammar` via `GrammarFrom` + [`From<Source>`](From).
///
/// Parses `GrammarFrom::Source`, then builds `Self` with `From::from` — this
/// conversion can't fail, so nothing is traced.
///
/// `#[grammar(...)]` here supports `name = "..."`, `hidden`/`inline`, and
/// `validated` (as for `#[derive(Grammar)]`) — `From` itself can't fail, but
/// `Validate::validate` still runs after the conversion and can reject the
/// value.
#[proc_macro_derive(GrammarFromOther, attributes(grammar))]
pub fn derive_grammar_from_source(input: TokenStream) -> TokenStream {
    derive_convert(input, Convert::From)
}

/// `Grammar` via `GrammarFrom` + [`TryFrom<Source>`](TryFrom).
///
/// Parses `GrammarFrom::Source`, then calls `Self::try_from`. `Err`
/// backtracks the parse as if nothing had matched, and is traced under the
/// `trace`/`trace_pos` features.
///
/// `#[grammar(...)]` here supports `name = "..."`, `hidden`/`inline`, and
/// `validated` (as for `#[derive(Grammar)]`) — `Validate::validate` runs
/// after a successful conversion, independent of the conversion's own
/// `Err`. Since the conversion can also reject an otherwise-matching
/// `Source`, the rule's own BNF definition shows a side-condition marker.
#[proc_macro_derive(GrammarTryFromOther, attributes(grammar))]
pub fn derive_grammar_try_from_source(input: TokenStream) -> TokenStream {
    derive_convert(input, Convert::TryFrom)
}

#[derive(Clone, Copy)]
enum Convert {
    FromStr,
    From,
    TryFrom,
}

fn derive_convert(input: TokenStream, convert: Convert) -> TokenStream {
    let mut input = syn::parse_macro_input!(input as DeriveInput);
    for param in input.generics.type_params_mut() {
        param.bounds.push(parse_quote!(::tygr::Grammar));
    }
    match impl_convert(&input, convert) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Expands a string literal into a type-level literal token.
///
/// This is the *primary way* to create literal tokens — the underlying
/// `StringEq<CharThen<CH, T>>` type is an implementation detail and should not
/// be named directly.
///
/// - Single character: `StringEq!(",")` → `StringEq<CharThen<',', ()>>`
/// - Multiple characters: `StringEq!("->")` → `StringEq<CharThen<'-', CharThen<'>', ()>>>`
#[proc_macro]
#[allow(non_snake_case)]
pub fn StringEq(input: TokenStream) -> TokenStream {
    let lit = syn::parse_macro_input!(input as syn::LitStr);
    let value = lit.value();
    let chars: Vec<char> = value.chars().collect();

    if chars.is_empty() {
        return syn::Error::new(
            lit.span(),
            "StringEq!() requires a non-empty string literal",
        )
        .to_compile_error()
        .into();
    }

    let chain = build_nested_chain(&chars, |ch, rest| quote! { ::tygr::CharThen<#ch, #rest> });
    quote! { ::tygr::StringEq<#chain> }.into()
}

/// Build a right-nested chain of literal-token types terminated by `()`.
fn build_nested_chain<F>(items: &[char], mapper: F) -> TokenStream2
where
    F: Fn(&char, TokenStream2) -> TokenStream2,
{
    items
        .iter()
        .rev()
        .fold(quote! { () }, |rest, ch| mapper(ch, rest))
}

#[proc_macro]
#[allow(non_snake_case)]
pub fn StringEqCI(input: TokenStream) -> TokenStream {
    let lit = syn::parse_macro_input!(input as syn::LitStr);
    let value = lit.value();
    let chars: Vec<char> = value.chars().collect();

    if chars.is_empty() {
        return syn::Error::new(
            lit.span(),
            "StringEqCI!() requires a non-empty string literal",
        )
        .to_compile_error()
        .into();
    }

    let chain = build_nested_chain(&chars, |ch, rest| quote! { ::tygr::CharCIThen<#ch, #rest> });
    quote! { ::tygr::StringEqCI<#chain> }.into()
}

#[derive(Clone)]
struct Tag {
    name: TokenStream2,
    case: Option<TokenStream2>,
}

impl Tag {
    fn new(name: TokenStream2, case: Option<TokenStream2>) -> Self {
        Self { name, case }
    }

    fn as_constructor(&self) -> TokenStream2 {
        let name = &self.name;
        let case = if let Some(case) = &self.case {
            quote! { :: #case }
        } else {
            quote! {}
        };
        quote! { #name #case }
    }
}

fn with_node(inline: bool, body: TokenStream2) -> TokenStream2 {
    if inline {
        body
    } else {
        quote! {
            let mut state = state.node(Self::NAME, pos);
            #body
        }
    }
}

/// The BNF rule name a type gets when `#[grammar(name = "...")]` isn't given.
fn default_name(ident: &Ident) -> String {
    let ident = ident.to_string();
    #[cfg(all(feature = "lower_bnf_name", not(feature = "upper_bnf_name")))]
    let ident = ident.to_ascii_lowercase();
    #[cfg(all(feature = "upper_bnf_name", not(feature = "lower_bnf_name")))]
    let ident = ident.to_ascii_uppercase();
    ident
}

fn impl_grammar(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let ident = &input.ident;
    let generics = &input.generics;
    let GrammarAttr {
        name,
        hidden,
        inline,
        validated,
    } = grammar_attr(input)?;
    let name = name.unwrap_or_else(|| default_name(ident));
    match &input.data {
        Data::Struct(data) => impl_struct(ident, generics, name, hidden, inline, validated, data),
        Data::Enum(data) => impl_enum(ident, generics, name, hidden, inline, validated, data),
        Data::Union(_) => Err(syn::Error::new_spanned(
            ident,
            "Grammar cannot be derived for unions",
        )),
    }
}

fn grammar_attr(input: &DeriveInput) -> syn::Result<GrammarAttr> {
    for attr in &input.attrs {
        if attr.path().is_ident("grammar") {
            return attr.parse_args::<GrammarAttr>();
        }
    }
    Ok(GrammarAttr::default())
}

#[derive(Default)]
struct GrammarAttr {
    name: Option<String>,
    hidden: bool,
    inline: bool,
    validated: bool,
}

impl syn::parse::Parse for GrammarAttr {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut attr = GrammarAttr::default();

        while !input.is_empty() {
            let ident: syn::Ident = input.parse()?;
            if ident == "name" {
                let _: syn::Token![=] = input.parse()?;
                let lit: syn::LitStr = input.parse()?;
                attr.name = Some(lit.value());
            } else if ident == "hidden" {
                attr.hidden = true;
                // Hidden nodes are not presented in BNF so they must be inline
                // as non-inline node names are presented in traces
                attr.inline = true;
            } else if ident == "inline" {
                attr.inline = true;
            } else if ident == "validated" {
                attr.validated = true;
            } else {
                return Err(syn::Error::new(
                    ident.span(),
                    "expected `name`, `hidden`, `inline`, or `validated`",
                ));
            }
            if input.is_empty() {
                break;
            }
            let _: syn::Token![,] = input.parse()?;
        }

        Ok(attr)
    }
}

type ProcessedFields<'a> = Vec<(String, Ident, &'a syn::Type)>;

fn components_and_xts(tag: TokenStream2, fields: &Fields) -> (TokenStream2, ProcessedFields<'_>) {
    match fields {
        Fields::Named(fields) => {
            let fields: Vec<_> = fields
                .named
                .iter()
                .map(|field| {
                    let x = field.ident.as_ref().unwrap().clone();
                    let t = &field.ty;
                    (x.to_string(), x, t)
                })
                .collect();
            let xs = fields.iter().map(|(_, x, _)| x);
            (quote! { #tag { #(#xs),* } }, fields)
        }
        Fields::Unnamed(fields) => {
            let fields: Vec<_> = fields
                .unnamed
                .iter()
                .enumerate()
                .map(|(i, field)| {
                    let x = Ident::new(&format!("_f{i}"), Span::call_site());
                    let t = &field.ty;
                    (i.to_string(), x, t)
                })
                .collect();
            let xs = fields.iter().map(|(_, x, _)| x);
            (quote! { #tag ( #(#xs),* ) }, fields)
        }
        Fields::Unit => (quote! { #tag }, vec![]),
    }
}

/// Before each field after the first, read the byte at its position: the
/// first field gets the caller's.
fn next_head(k: usize) -> TokenStream2 {
    if k == 0 {
        quote! {}
    } else {
        quote! { let __head = ::tygr::head_at(input, pos); }
    }
}

fn parse_at(validated: bool, fields: &ProcessedFields, constructor: &TokenStream2) -> TokenStream2 {
    let steps: Vec<_> = fields
        .iter()
        .enumerate()
        .map(|(k, (_, x, t))| {
            let head = next_head(k);
            quote! {
                #head
                let (#x, pos) = <#t as ::tygr::Grammar>::parse_at(input, pos, __head, state.reborrow())?;
            }
        })
        .collect();
    let (start_pos, validate) = if validated {
        let trace = if cfg!(feature = "trace") {
            quote! {
                state.expect(pos, ::tygr::Expectation::Valid {
                    node: Self::NAME,
                    text: input[start_pos..pos].to_string(),
                    requirement: <Self as ::tygr::Validate>::REQUIREMENT,
                });
            }
        } else if cfg!(feature = "trace_pos") {
            quote! {
                state.expect(pos);
            }
        } else {
            quote! {}
        };
        (
            quote! { let start_pos = pos; },
            quote! {
                if !::tygr::Validate::validate(&value) {
                    #trace
                    return None
                }
            },
        )
    } else {
        (quote! {}, quote! {})
    };
    quote! {
        #start_pos
        #(#steps)*
        let value = #constructor;
        #validate
        Some((value, pos))
    }
}

fn scan_at(validated: bool, fields: &ProcessedFields, constructor: &TokenStream2) -> TokenStream2 {
    if validated {
        let parse_at = parse_at(true, fields, constructor);
        quote! { ({#parse_at}).map(|(_, pos)| pos) }
    } else {
        let steps: Vec<_> = fields
            .iter()
            .enumerate()
            .map(|(k, (_, _, t))| {
                let head = next_head(k);
                quote! {
                    #head
                    let pos = <#t as ::tygr::Grammar>::scan_at(input, pos, __head, state.reborrow())?;
                }
            })
            .collect();
        quote! {
            #(#steps)*
            Some(pos)
        }
    }
}

fn print_steps(fields: &ProcessedFields) -> Vec<TokenStream2> {
    fields
        .iter()
        .map(|(_, x, _)| {
            quote! {
                ::tygr::Grammar::print_to(#x, buf);
            }
        })
        .collect()
}

fn to_bnf(fields: &ProcessedFields) -> TokenStream2 {
    let ts = fields.iter().map(|(_, _, t)| quote! { #t });
    quote! {
        ::tygr::bnf::Expr::sequence(vec![
            #(<#ts as ::tygr::Grammar>::to_bnf()),*
        ])
    }
}

fn bnf_ref(grammar_name: &str, hidden: bool, to_bnf: &TokenStream2, inline: bool) -> TokenStream2 {
    if hidden {
        quote! { ::tygr::bnf::Expr::empty() }
    } else if inline {
        quote! { #to_bnf }
    } else {
        quote! { ::tygr::bnf::Expr::RuleRef(#grammar_name.to_string()) }
    }
}

struct FieldsInfo {
    constructor: TokenStream2,
    parse_at: TokenStream2,
    scan_at: TokenStream2,
    print_to: TokenStream2,
    to_bnf: TokenStream2,
}

impl FieldsInfo {
    fn from(validated: bool, tag: Tag, fields: &Fields) -> Self {
        let (constructor, fields) = components_and_xts(tag.as_constructor(), fields);
        let print_steps = print_steps(&fields);
        let parse_at = parse_at(validated, &fields, &constructor);
        let scan_at = scan_at(validated, &fields, &constructor);
        Self {
            parse_at: quote! { #parse_at },
            scan_at: quote! { #scan_at },
            print_to: quote! {#(#print_steps)*},
            to_bnf: to_bnf(&fields),
            constructor,
        }
    }
}

fn impl_convert(input: &DeriveInput, convert: Convert) -> syn::Result<TokenStream2> {
    let ident = &input.ident;
    let generics = &input.generics;
    let GrammarAttr {
        name,
        hidden,
        inline,
        validated,
    } = grammar_attr(input)?;
    let name = name.unwrap_or_else(|| default_name(ident));
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let self_ty = quote! { #ident #ty_generics };
    let source = quote! { <#self_ty as ::tygr::GrammarFrom>::Source };
    // parse_at: parse Source, then build Self via the forward conversion.
    let trace = if cfg!(feature = "trace") {
        quote! {
            state.expect(
                end,
                ::tygr::Expectation::GrammarFrom {
                    from: input[pos..end].to_string(),
                    into: Self::NAME,
                    fail: ::std::string::ToString::to_string(&err),
                },
            );
        }
    } else if cfg!(feature = "trace_pos") {
        quote! {
            state.expect(end);
        }
    } else {
        quote! {}
    };
    // After the conversion succeeds, `#[grammar(validated)]` can still
    // reject `value` — independent of whether the conversion itself can fail.
    let validate = if validated {
        let validate_trace = if cfg!(feature = "trace") {
            quote! {
                state.expect(end, ::tygr::Expectation::Valid {
                    node: Self::NAME,
                    text: input[pos..end].to_string(),
                    requirement: <Self as ::tygr::Validate>::REQUIREMENT,
                });
            }
        } else if cfg!(feature = "trace_pos") {
            quote! {
                state.expect(end);
            }
        } else {
            quote! {}
        };
        quote! {
            if !::tygr::Validate::validate(&value) {
                #validate_trace
                return None
            }
        }
    } else {
        quote! {}
    };
    let parse_at = match convert {
        Convert::FromStr => quote! {
            let end = <#source as ::tygr::Grammar>::scan_at(input, pos, __head, state.reborrow())?;
            match <#self_ty as ::core::str::FromStr>::from_str(&input[pos..end]) {
                Ok(value) => {
                    #validate
                    Some((value, end))
                }
                Err(err) => {
                    #trace
                    let _ = &err;
                    None
                }
            }
        },
        Convert::From => quote! {
            let (source, end) = <#source as ::tygr::Grammar>::parse_at(input, pos, __head, state.reborrow())?;
            let value = <#self_ty as ::core::convert::From<#source>>::from(source);
            #validate
            Some((value, end))
        },
        Convert::TryFrom => quote! {
            let (source, end) = <#source as ::tygr::Grammar>::parse_at(input, pos, __head, state.reborrow())?;
            match <#self_ty as ::core::convert::TryFrom<#source>>::try_from(source) {
                Ok(value) => {
                    #validate
                    Some((value, end))
                }
                Err(err) => {
                    #trace
                    let _ = &err;
                    None
                }
            }
        },
    };
    // scan_at: infallible `From` alone can just delegate to `Source`; anything
    // that can still reject (a fallible conversion, or `validated`) has to
    // run the full check via `parse_at`.
    let scan_at = if !validated && matches!(convert, Convert::From) {
        quote! {
            <#source as ::tygr::Grammar>::scan_at(input, pos, __head, state)
        }
    } else {
        quote! {
            Self::parse_at(input, pos, __head, state).map(|(_, end)| end)
        }
    };
    let to_bnf = quote! { <#source as ::tygr::Grammar>::to_bnf() };
    let bnf_ref = bnf_ref(&name, hidden, &to_bnf, inline);
    // The rule's own definition shows the side-condition; a reference to it
    // from elsewhere (`RuleRef`, or a spliced `#[grammar(inline)]` body)
    // doesn't — nesting `^N` inside another rule's sequence would make its
    // scope ambiguous. The conversion's own side-condition is BNF-only (the
    // trace already has the real conversion error's message); `validated`'s
    // is also traced, same as for `#[derive(Grammar)]`.
    let to_bnf_def = match convert {
        Convert::From => to_bnf,
        Convert::FromStr | Convert::TryFrom => quote! {
            ::tygr::bnf::Expr::side_condition(#to_bnf, "be convertible")
        },
    };
    let to_bnf_def = if validated {
        quote! { ::tygr::bnf::Expr::side_condition(#to_bnf_def, <Self as ::tygr::Validate>::REQUIREMENT) }
    } else {
        to_bnf_def
    };
    Ok(quote! {
        impl #impl_generics ::tygr::Grammar for #self_ty #where_clause {
            #[inline(always)]
            fn parse_at(input: &str, pos: usize, __head: Option<u8>, #[allow(unused_mut)] mut state: ::tygr::State) -> Option<(Self, usize)> {
                #parse_at
            }

            #[inline(always)]
            fn scan_at(input: &str, pos: usize, __head: Option<u8>, #[allow(unused_mut)] mut state: ::tygr::State) -> Option<usize> {
                #scan_at
            }

            fn print_to(&self, buf: &mut ::std::string::String) {
                <#self_ty as ::tygr::GrammarFrom>::print_to(self, buf);
            }

            fn to_bnf() -> ::tygr::bnf::Expr {
                #bnf_ref
            }
        }

        impl #impl_generics ::tygr::GrammarRule for #self_ty #where_clause {
            const NAME: &'static str = #name;

            fn to_bnf_def() -> ::tygr::bnf::Expr {
                #to_bnf_def
            }
        }
    })
}

fn impl_struct(
    ident: &Ident,
    generics: &Generics,
    name: String,
    hidden: bool,
    inline: bool,
    validated: bool,
    data: &DataStruct,
) -> syn::Result<TokenStream2> {
    let tag = Tag::new(quote! { #ident }, None);
    let fields = &data.fields;
    let FieldsInfo {
        constructor,
        parse_at,
        scan_at,
        print_to,
        to_bnf,
    } = FieldsInfo::from(validated, tag, fields);
    let parse_at = with_node(inline, parse_at);
    let scan_at = with_node(inline, scan_at);
    let to_bnf = if validated {
        quote! { ::tygr::bnf::Expr::side_condition(#to_bnf, <Self as ::tygr::Validate>::REQUIREMENT) }
    } else {
        to_bnf
    };
    let bnf_ref = bnf_ref(&name, hidden, &to_bnf, inline);
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics ::tygr::Grammar for #ident #ty_generics #where_clause {
            #[inline(always)]
            fn parse_at(input: &str, pos: usize, __head: Option<u8>, #[allow(unused_mut)] mut state: ::tygr::State) -> Option<(Self, usize)> {
                #parse_at
            }

            #[inline(always)]
            fn scan_at(input: &str, pos: usize, __head: Option<u8>, #[allow(unused_mut)] mut state: ::tygr::State) -> Option<usize> {
                #scan_at
            }

            fn print_to(&self, buf: &mut ::std::string::String) {
                let #constructor = &self;
                #print_to
            }

            fn to_bnf() -> ::tygr::bnf::Expr {
                #bnf_ref
            }
        }

        impl #impl_generics ::tygr::GrammarRule for #ident #ty_generics #where_clause {
            const NAME: &'static str = #name;

            fn to_bnf_def() -> ::tygr::bnf::Expr {
                #to_bnf
            }
        }
    })
}

// ── Enum → alternation ──────────────────────────────────────────────────────

fn impl_enum(
    ident: &Ident,
    generics: &Generics,
    name: String,
    hidden: bool,
    inline: bool,
    validated: bool,
    data: &DataEnum,
) -> syn::Result<TokenStream2> {
    let mut each_constructor = vec![];
    let mut each_parse_at = vec![];
    let mut each_scan_at = vec![];
    let mut each_print_to = vec![];
    let mut each_to_bnf = vec![];
    for variant in &data.variants {
        let variant_ident = &variant.ident;
        let tag = Tag::new(quote! { #ident }, Some(quote! {#variant_ident}));
        let FieldsInfo {
            constructor,
            parse_at,
            scan_at,
            print_to,
            to_bnf,
        } = FieldsInfo::from(validated, tag, &variant.fields);
        each_constructor.push(constructor);
        each_parse_at.push(parse_at);
        each_scan_at.push(scan_at);
        each_print_to.push(print_to);
        each_to_bnf.push(to_bnf);
    }
    let to_bnf = quote! { ::tygr::bnf::Expr::alternation(vec![ #(#each_to_bnf),* ]) };
    let to_bnf = if validated {
        quote! { ::tygr::bnf::Expr::side_condition(#to_bnf, <Self as ::tygr::Validate>::REQUIREMENT) }
    } else {
        to_bnf
    };
    let bnf_ref = bnf_ref(&name, hidden, &to_bnf, inline);
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let (parse_body, parse_items) = enum_choice("parse", &each_parse_at, quote! { Option<(Self, usize)> });
    let (scan_body, scan_items) = enum_choice("scan", &each_scan_at, quote! { Option<usize> });
    let parse_body = with_node(inline, parse_body);
    let scan_body = with_node(inline, scan_body);
    Ok(quote! {
        impl #impl_generics #ident #ty_generics #where_clause {
            #parse_items
            #scan_items
        }

        impl #impl_generics ::tygr::Grammar for #ident #ty_generics #where_clause {
            #[inline(always)]
            fn parse_at(input: &str, pos: usize, __head: Option<u8>, #[allow(unused_mut)] mut state: ::tygr::State) -> Option<(Self, usize)> {
                #parse_body
            }

            #[inline(always)]
            fn scan_at(input: &str, pos: usize, __head: Option<u8>, #[allow(unused_mut)] mut state: ::tygr::State) -> Option<usize> {
                #scan_body
            }

            fn print_to(&self, buf: &mut ::std::string::String) {
                match self {
                    #(#each_constructor => { #each_print_to }),*
                }
            }

            fn to_bnf() -> ::tygr::bnf::Expr {
                #bnf_ref
            }
        }

        impl #impl_generics ::tygr::GrammarRule for #ident #ty_generics #where_clause {
            const NAME: &'static str = #name;

            fn to_bnf_def() -> ::tygr::bnf::Expr {
                #to_bnf
            }
        }
    })
}

/// An enum's `parse_at` or `scan_at` body (`kind` is "parse" or "scan"), and
/// the methods it uses: ordered choice over the variants, each an
/// always-inlined method so that a known first byte reaches its first test.
/// From `ARMS_THRESHOLD` variants on, the choice is compiled once per first
/// byte (and once for the end of the input), and the body jumps to the copy
/// for the actual byte. Knowing the byte, the compiler cuts each variant that
/// cannot start with it down to its failure branch and skips the first test
/// of those that can; copies that come out the same are merged.
fn enum_choice(kind: &str, bodies: &[TokenStream2], ret: TokenStream2) -> (TokenStream2, TokenStream2) {
    const ARMS_THRESHOLD: usize = 4;
    let variants: Vec<Ident> = (0..bodies.len()).map(|k| format_ident!("{kind}_variant_{k}")).collect();
    let choice = quote! {
        #( if let Some(result) = Self::#variants(input, pos, __head, state.reborrow()) { return Some(result); } )*
        None
    };
    let mut items = quote! {
        #(
            #[inline(always)]
            #[allow(unused_variables, unused_mut)]
            fn #variants(input: &str, pos: usize, __head: Option<u8>, mut state: ::tygr::State) -> #ret {
                #bodies
            }
        )*
    };
    if bodies.len() < ARMS_THRESHOLD {
        return (choice, items);
    }
    let arm = format_ident!("{kind}_arm");
    let arms = format_ident!("{}_ARMS", kind.to_uppercase());
    let xs = 0..=256u16;
    items.extend(quote! {
        #[inline(never)]
        #[allow(unused_mut)]
        fn #arm<const X: u16>(input: &str, pos: usize, mut state: ::tygr::State) -> #ret {
            let __head = u8::try_from(X).ok();
            #choice
        }

        const #arms: [fn(&str, usize, ::tygr::State<'_>) -> #ret; 257] = [ #(Self::#arm::<#xs>),* ];
    });
    (quote! { Self::#arms[__head.map_or(256, usize::from)](input, pos, state) }, items)
}
