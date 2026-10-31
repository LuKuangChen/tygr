//! # tygr — TYpe into Grammar Routines
//!
//! Define grammars as Rust types and derive the routine work of grammar
//! handling: a **parser**, a **printer**, and a **presentation** (in EBNF).
//!
//! ## Design
//!
//! Derive [`Grammar`] on a `struct` or `enum` to get a parser, a printer, and
//! a BNF presentation for free. Each supported Rust construct maps to an
//! EBNF concept.
//!
//! | Rust construct | EBNF ([Wirth 1977](https://doi.org/10.1145/359863.359883)) |
//! |----------------|-------------------------------|
//! | `struct`       | concatenation (`A B C`)      |
//! | `enum`         | alternation (`A B \| C`) |
//! | `(A, B, …)`    | inline concatenation         |
//! | [`Either<A, B>`](either::Either) | inline alternation (`A \| B`) |
//! | [`Vec<T>`]     | repetition (`{ T }`)         |
//! | [`Vec1<T>`]    | one-or-more repetition (`T { T }`)      |
//! | [`Option<T>`]  | optional (`[ T ]`)           |
//! | [`Box<T>`]     | recursive indirection        |
//! | [`NotFollowedBy<G>`] | negative lookahead (`!G`), consumes nothing |
//! | [`FollowedBy<G>`] | positive lookahead (`!!G`), consumes nothing |
//! | [`Hidden<T>`]  | parsed/printed, omitted from BNF |
//! | [`Raw<T>`]     | parsed via `T`, kept as the raw matched text |
//! | [`Range<T>`]   | parsed via `T`, kept alongside its `[start, end)` span |
//! | [`Wrap<L, T, R>`] / [`Prefix<P, T>`] / [`Suffix<T, S>`] | concatenation shorthands, [`Deref`]-ing to `T` |
//! | [`VecSep<T, S>`] | one-or-more repetition with separators (`T { S T }`) |
//!
//! For a type that isn't itself a direct concatenation/alternation of other
//! grammars, but is instead built *from* one (e.g. parsing digits into a
//! `u32`), derive [`GrammarFromStr`](derive@GrammarFromStr),
//! [`GrammarFromOther`](derive@GrammarFromOther), or
//! [`GrammarTryFromOther`](derive@GrammarTryFromOther) instead — each builds
//! `Self` from a parsed [`GrammarFrom::Source`] via `FromStr`, `From`, or
//! `TryFrom` respectively.
//!
//! `#[grammar(...)]` on a `#[derive(Grammar)]` type:
//! - `name = "..."` — override the BNF rule name (defaults to the type name).
//! - `hidden` — same effect as wrapping in [`Hidden<T>`], but on a whole struct/enum.
//! - `inline` — splice this type's own definition wherever it's
//!   referenced, instead of a rule reference.
//! - `validated` — after a successful parse, run [`Validate::validate`] on the
//!   value; a rejection backtracks as if the grammar hadn't matched.
//!
//! ## Feature Flags
//!
//! The `tygr` crate enables the `trace_one_node` feature by default.
//! Available features are:
//!
//! | Feature | Description |
//! | ------- | ----------- |
//! | `default` | Enables `trace_one_node`. |
//! | `trace` | Enables all tracing features. |
//! | `trace_pos` | Traces parser positions. |
//! | `trace_expect` | Traces what was expected at the farthest position, without grammar nodes. |
//! | `trace_one_node` | Traces only the nearest grammar node for each attempt. |
//! | `trace_all_nodes` | Traces the complete grammar node chain for each attempt. |
//! | `lower_bnf_name` | Converts generated BNF names to lowercase. |
//! | `upper_bnf_name` | Converts generated BNF names to uppercase. |
//!
//! ## Quick Example
//!
//! ```rust
//! use tygr::*;
//!
//! #[derive(Grammar)]
//! pub struct Expr(pub Expr1, pub Vec<(Wrap<Ws, Op1, Ws>, Expr1)>);
//!
//! #[derive(Grammar)]
//! pub enum Op1 {
//!     Add(StringEq!("+")),
//!     Sub(StringEq!("-")),
//! }
//!
//! #[derive(Grammar)]
//! pub struct Expr1(pub Expr2, pub Vec<(Wrap<Ws, Op2, Ws>, Expr2)>);
//!
//! #[derive(Grammar)]
//! pub enum Op2 {
//!     Mul(StringEq!("*")),
//!     Div(StringEq!("/")),
//! }
//!
//! #[derive(Grammar)]
//! pub enum Expr2 {
//!     Paren(Wrap<(StringEq!("("), Ws), Box<Expr>, (Ws, StringEq!(")"))>),
//!     Number(Int),
//! }
//!
//! char_class!(pub IsDigit, "digit", |ch| ch.is_ascii_digit());
//!
//! #[derive(Grammar)]
//! pub struct Int(pub StringOf1<IsDigit>);
//!
//! char_class!(pub IsSpace, "space", |ch| ch.is_ascii_whitespace());
//!
//! #[derive(Grammar)]
//! #[grammar(hidden)]
//! pub struct Ws(pub StringOf<IsSpace>);
//!
//! let e = Expr::parse("1 + 2 * 3").unwrap();
//! assert_eq!(e.print(), "1 + 2 * 3");
//! assert_eq!(
//!     bnf_rules![Expr, Op1, Expr1, Op2, Expr2, Int].to_string(),
//!     "Expr = Expr1 { Op1 Expr1 } .\n\
//! Op1 = \"+\" | \"-\" .\n\
//! Expr1 = Expr2 { Op2 Expr2 } .\n\
//! Op2 = \"*\" | \"/\" .\n\
//! Expr2 = \"(\" Expr \")\" | Int .\n\
//! Int = 'digit' { 'digit' } ."
//! );
//! ```
#![warn(missing_docs)]

pub mod bnf;
mod char;
mod grammar;
mod olc;
mod state;

use std::ops::Deref;

pub use char::*;
pub use grammar::*;
pub use olc::*;
pub use state::*;

// Re-exported so the `StringEq!()`/`StringEqCI!()` macro expansions can
// reference them, but hidden from docs so users always reach for the macros
// instead of naming these types directly. `StringEq`/`StringEqCI` here are
// types (in the type namespace); the macros of the same name below live in
// the macro namespace, so there's no conflict.
#[doc(hidden)]
pub use char::{CharCIThen, CharThen, StringEq, StringEqCI};

// Lets generated code inside this crate use `::tygr::...` paths, matching
// how downstream crates refer to it.
#[doc(hidden)]
extern crate self as tygr;

pub use tygr_derive::Grammar;
pub use tygr_derive::{GrammarFromOther, GrammarFromStr, GrammarTryFromOther};

/// Expand a string literal into a literal-token type.
///
/// - Single character: `StringEq!(",")` → `StringEq<CharThen<',', ()>>`
/// - Multiple characters: `StringEq!("->")` → `StringEq<CharThen<'-', CharThen<'>', ()>>>`
///
/// ```
/// # use tygr::*;
/// # char_class!(pub IsDigit, "digit", |ch| ch.is_ascii_digit());
/// # #[derive(Grammar)]
/// # pub struct Num(StringOf1<IsDigit>);
/// # #[derive(Grammar)]
/// # pub struct Expr(Num);
/// #[derive(Grammar)]
/// pub enum Factor {
///     Paren(StringEq!("("), Box<Expr>, StringEq!(")")),
///     Num(Num),
/// }
/// ```
pub use tygr_derive::StringEq;

/// Expand a string literal into a *case-insensitive* literal-token type.
///
/// Like [`StringEq!`], but matches both uppercase and lowercase variants.
///
/// ```
/// # use tygr::*;
/// # char_class!(IsSpace, "space", |ch| matches!(ch, ' ' | '\t'));
/// #[derive(Grammar)]
/// pub enum Keyword {
///     Class(StringEqCI!("class")),  // matches "class", "CLASS", "Class", etc.
///     Method(StringEqCI!("method")),
/// }
/// ```
pub use tygr_derive::StringEqCI;

// ── Macros ──────────────────────────────────────────────────────────────────

/// Collect BNF rule definitions from one or more [`GrammarRule`] types.
///
/// ```
/// # use tygr::*;
/// # char_class!(IsDigit, "digit", |ch| ch.is_ascii_digit());
/// # #[derive(Grammar)] struct Number(StringOf1<IsDigit>);
/// # #[derive(Grammar)] struct AddOp(StringEq!("+"));
/// # #[derive(Grammar)] struct MulOp(StringEq!("*"));
/// # #[derive(Grammar)] struct Factor(Number);
/// # #[derive(Grammar)] struct Term(Factor);
/// # #[derive(Grammar)] struct Expr(Term);
/// let bnf = bnf_rules![Expr, Term, Factor, Number, AddOp, MulOp];
/// println!("{bnf}");
/// ```
#[macro_export]
macro_rules! bnf_rules {
    ($($T:ty),+ $(,)?) => {{
        let mut _buf = ::std::string::String::new();
        $(
            if !_buf.is_empty() { _buf.push('\n'); }
            _buf.push_str(&<$T as $crate::GrammarRule>::bnf_rule());
        )+
        _buf
    }};
}

/// Define a [`CharClass`] in one line.
///
/// Three forms:
///
/// ```
/// # use tygr::*;
/// char_class!(IsDigit, "digit", |ch| ch.is_ascii_digit());
/// ```
///
/// A visibility qualifier is optional (defaults to private):
///
/// ```
/// # use tygr::*;
/// char_class!(pub IsDigit, "digit", |ch| ch.is_ascii_digit());
/// ```
///
/// ```
/// # use tygr::*;
/// char_class!(IsDigit, "digit", |ch| ch.is_ascii_digit());
///
/// #[derive(Grammar)]
/// struct Num(StringOf1<IsDigit>);
/// ```
#[macro_export]
macro_rules! char_class {
    ($vis:vis $name:ident, $label:expr, |$ch:ident| $pred:expr) => {
        $vis struct $name;
        impl $crate::CharClass for $name {
            fn matches($ch: char) -> bool { $pred }
            fn name() -> &'static str { $label }
        }
    };
}

/// Like [`char_class!`], but the predicate must be usable in a `const fn`
/// (comparisons, `matches!`, `is_ascii_*`, ...), which the compiler enforces.
/// In return tygr works out at compile time which bytes can start a matching
/// char, for faster runs in [`StringOf`]/[`StringOf1`].
///
/// A class can also be a union of classes, or one class built from
/// [`AllChars`], [`NoChars`], [`CharRange`], `StringEq!` (read as the set of
/// its chars), [`Union`] and [`Except`], which this form gives a label:
///
/// ```
/// # use tygr::*;
/// const_char_class!(AsciiLetter, "ASCII letter", |ch| ch.is_ascii_alphabetic());
/// const_char_class!(Word, "letter", AsciiLetter | CharRange<'\u{80}', { char::MAX }>);
/// const_char_class!(Plain, "plain char", Except<AllChars, Union<StringEq!("\"\\"), CharRange<'\0', '\x1F'>>>);
///
/// assert_eq!(StringOf::<Word>::scan_prefix("abc日本語 rest"), Ok("abc日本語".len()));
/// assert_eq!(StringOf::<Plain>::scan_prefix("abc日本語\"rest"), Ok("abc日本語".len()));
/// ```
#[macro_export]
macro_rules! const_char_class {
    ($vis:vis $name:ident, $label:expr, |$ch:ident| $pred:expr) => {
        $vis struct $name;
        impl $name {
            #[allow(dead_code)]
            const fn matches_const($ch: char) -> bool { $pred }
        }
        impl $crate::CharClass for $name {
            #[inline]
            fn matches($ch: char) -> bool { Self::matches_const($ch) }
            fn name() -> &'static str { $label }
            // A one-byte char: the inlined predicate is cheaper than a table load.
            #[inline(always)]
            fn has_char_at(input: &str, pos: usize, b: u8) -> bool {
                if b < 0x80 {
                    Self::matches_const(b as char)
                } else {
                    match Self::LEAD_BYTES[b as usize] {
                        $crate::Lead::Accept => true,
                        $crate::Lead::Reject => false,
                        $crate::Lead::Check => <Self as $crate::CharClass>::check_at(input, pos),
                    }
                }
            }
            // Settle each byte that starts at most 64 chars by trying them
            // all; chars of three or four bytes are checked when met.
            const LEAD_BYTES: [$crate::Lead; 256] = {
                let mut t = $crate::lead::REJECT;
                let mut i = 0;
                while i < 256 {
                    if let Some((lo, hi)) = $crate::lead::span(i as u8) {
                        t[i] = if hi - lo < 64 {
                            let (mut any, mut all) = (false, true);
                            let mut c = lo;
                            while c <= hi {
                                let m = match char::from_u32(c) {
                                    Some(ch) => Self::matches_const(ch),
                                    None => false,
                                };
                                any = any || m;
                                all = all && m;
                                c += 1;
                            }
                            if all { $crate::Lead::Accept } else if any { $crate::Lead::Check } else { $crate::Lead::Reject }
                        } else {
                            $crate::Lead::Check
                        };
                    }
                    i += 1;
                }
                t
            };
        }
    };
    ($vis:vis $name:ident, $label:expr, $first:ty $(| $rest:ty)*) => {
        $vis struct $name;
        impl $crate::CharClass for $name {
            #[inline]
            fn matches(ch: char) -> bool {
                <$first as $crate::CharClass>::matches(ch) $(|| <$rest as $crate::CharClass>::matches(ch))*
            }
            fn name() -> &'static str { $label }
            #[inline(always)]
            fn check_at(input: &str, pos: usize) -> bool {
                let b = input.as_bytes()[pos];
                <$first as $crate::CharClass>::has_char_at(input, pos, b) $(|| <$rest as $crate::CharClass>::has_char_at(input, pos, b))*
            }
            const LEAD_BYTES: [$crate::Lead; 256] = {
                let t = <$first as $crate::CharClass>::LEAD_BYTES;
                $(let t = $crate::lead::union(&t, &<$rest as $crate::CharClass>::LEAD_BYTES);)*
                t
            };
        }
    };
}

/// Post-parse validation for `#[grammar(validated)]` types.
///
/// Return `false` to reject the parsed value — the parse backtracks as if
/// the grammar hadn't matched, and [`REQUIREMENT`](Validate::REQUIREMENT) is reported
/// in traces as "must {REQUIREMENT}".
pub trait Validate {
    /// The property a value must satisfy — a fixed, per-type description,
    /// not a per-value diagnostic, since it doesn't depend on `self`. Shown
    /// in both error traces and generated BNF, so pick something concrete
    /// rather than a generic placeholder.
    const REQUIREMENT: &'static str;

    /// Check the just-parsed value; return `false` to reject it.
    fn validate(&self) -> bool;
}

// `pub`, not private: `VecSepSource` (public) has it as a field type, so it
// must stay nameable (E0446).
#[derive(Debug, Clone, PartialEq, Eq, Grammar)]
#[grammar(inline)]
#[doc(hidden)]
pub struct SepItem<S, T> {
    sep: S,
    item: T,
}

// The shape actually parsed (`T { S T }`); `VecSep` converts from this to
// expose `items`/`seps` as plain vectors instead.
#[derive(Debug, Clone, PartialEq, Eq, Grammar)]
#[grammar(inline)]
#[doc(hidden)]
pub struct VecSepSource<T, S> {
    head: Box<T>,
    rest: Vec<SepItem<S, T>>,
}

/// One or more items, with a separator between each pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VecSep<T, S> {
    /// The `T` items, in order.
    pub items: Vec<T>,
    /// The `S` separators, in order — always exactly one fewer than `items`.
    pub seps: Vec<S>,
}

impl<T, S> From<VecSepSource<T, S>> for VecSep<T, S> {
    fn from(source: VecSepSource<T, S>) -> Self {
        let mut items = Vec::with_capacity(source.rest.len() + 1);
        let mut seps = Vec::with_capacity(source.rest.len());
        items.push(*source.head);
        for sep_item in source.rest {
            seps.push(sep_item.sep);
            items.push(sep_item.item);
        }
        VecSep { items, seps }
    }
}

impl<T: Grammar, S: Grammar> GrammarFrom for VecSep<T, S> {
    type Source = VecSepSource<T, S>;

    fn print_to(&self, buf: &mut String) {
        let mut items = self.items.iter();
        if let Some(first) = items.next() {
            first.print_to(buf);
        }
        for (sep, item) in self.seps.iter().zip(items) {
            sep.print_to(buf);
            item.print_to(buf);
        }
    }
}

// Written by hand rather than `#[derive(GrammarFromOther)]` only so that
// `parse_at` can push straight into `items`/`seps`: going through
// `VecSepSource` would box the head, build `rest`, then copy everything into
// two fresh vectors. Everything else delegates to `VecSepSource`, exactly as
// the derive (with `#[grammar(inline)]`) would.
impl<T: Grammar, S: Grammar> Grammar for VecSep<T, S> {
    #[inline(always)]
    fn parse_at(
        input: &str,
        pos: usize,
        head: Option<u8>,
        mut state: State,
    ) -> Option<(Self, usize)> {
        let (first, mut pos) = T::parse_at(input, pos, head, state.reborrow())?;
        // Not `vec![first]`: that allocates room for exactly one item, so a
        // second item always reallocates. Let `Vec`'s growth policy size it.
        let mut items = Vec::new();
        items.push(first);
        let mut seps = Vec::new();
        // Same as `Vec<SepItem<S, T>>`: stop at the first `S T` that fails.
        loop {
            let Some((sep, after_sep)) =
                S::parse_at(input, pos, head_at(input, pos), state.reborrow())
            else {
                break;
            };
            let Some((item, after_item)) = T::parse_at(
                input,
                after_sep,
                head_at(input, after_sep),
                state.reborrow(),
            ) else {
                break;
            };
            // A grammar where `S` and `T` can both match empty is not
            // supported. As a courtesy, stop instead of looping forever when
            // that happens.
            if after_item == pos {
                break;
            }
            seps.push(sep);
            items.push(item);
            pos = after_item;
        }
        Some((VecSep { items, seps }, pos))
    }

    #[inline(always)]
    fn scan_at(input: &str, pos: usize, head: Option<u8>, state: State) -> Option<usize> {
        <VecSepSource<T, S> as Grammar>::scan_at(input, pos, head, state)
    }

    fn print_to(&self, buf: &mut String) {
        <Self as GrammarFrom>::print_to(self, buf);
    }

    fn to_bnf() -> bnf::Expr {
        <VecSepSource<T, S> as Grammar>::to_bnf()
    }
}

impl<T: Grammar, S: Grammar> GrammarRule for VecSep<T, S> {
    // Matches what the derive's default naming would give.
    #[cfg(not(any(feature = "lower_bnf_name", feature = "upper_bnf_name")))]
    const NAME: &'static str = "VecSep";
    #[cfg(all(feature = "lower_bnf_name", not(feature = "upper_bnf_name")))]
    const NAME: &'static str = "vecsep";
    #[cfg(all(feature = "upper_bnf_name", not(feature = "lower_bnf_name")))]
    const NAME: &'static str = "VECSEP";
    #[cfg(all(feature = "lower_bnf_name", feature = "upper_bnf_name"))]
    const NAME: &'static str = "VecSep";

    fn to_bnf_def() -> bnf::Expr {
        <VecSepSource<T, S> as Grammar>::to_bnf()
    }
}

impl<T, S> VecSep<T, S> {
    /// Consume into `(item, separator)` pairs, one per separator (the first
    /// item is paired with the separator that follows it).
    pub fn into_item_sep(self) -> impl Iterator<Item = (T, S)> {
        self.items.into_iter().zip(self.seps)
    }

    /// The first `T` item.
    pub fn first(&self) -> &T {
        &self.items[0]
    }

    /// The last `T` item.
    pub fn last(&self) -> &T {
        self.items.last().unwrap()
    }
}

/// Sequence of three grammars, [`Deref`]-ing to the middle one.
#[derive(Debug, Clone, PartialEq, Eq, Grammar)]
#[grammar(inline)]
pub struct Wrap<L, T, R> {
    /// The leading element.
    pub before: L,
    /// The wrapped value.
    pub wrapped: T,
    /// The trailing element.
    pub after: R,
}

impl<L, T, R> Wrap<L, T, R> {
    /// Construct a `Wrap` from its three parts.
    pub fn new(before: L, wrapped: T, after: R) -> Self {
        Self {
            before,
            wrapped,
            after,
        }
    }

    /// Apply `f` to the wrapped value, keeping `before`/`after` unchanged.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Wrap<L, U, R> {
        Wrap {
            before: self.before,
            wrapped: f(self.wrapped),
            after: self.after,
        }
    }
}

impl<L, T, E, R> Wrap<L, Result<T, E>, R> {
    /// Swap `Wrap<L, Result<T, E>, R>` for `Result<Wrap<L, T, R>, E>`.
    pub fn transpose(self) -> Result<Wrap<L, T, R>, E> {
        Ok(Wrap {
            before: self.before,
            wrapped: self.wrapped?,
            after: self.after,
        })
    }
}

/// Sequence of two grammars, [`Deref`]-ing to the second.
#[derive(Debug, Clone, PartialEq, Eq, Grammar)]
#[grammar(inline)]
pub struct Prefix<P, T> {
    prefix: P,
    /// The prefixed value.
    pub prefixed: T,
}

/// Sequence of two grammars, [`Deref`]-ing to the first.
#[derive(Debug, Clone, PartialEq, Eq, Grammar)]
#[grammar(inline)]
pub struct Suffix<T, S> {
    suffixed: T,
    suffix: S,
}

impl<L, T, R> Deref for Wrap<L, T, R> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.wrapped
    }
}

impl<T, S> Deref for Suffix<T, S> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.suffixed
    }
}

impl<P, T> Deref for Prefix<P, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.prefixed
    }
}

impl<P, T> Prefix<P, T> {
    /// Construct a `Prefix` from its two parts.
    pub fn new(prefix: P, prefixed: T) -> Self {
        Self { prefix, prefixed }
    }

    /// Apply `f` to the prefixed value, keeping `prefix` unchanged.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Prefix<P, U> {
        Prefix {
            prefix: self.prefix,
            prefixed: f(self.prefixed),
        }
    }
}

impl<P, T, E> Prefix<P, Result<T, E>> {
    /// Swap `Prefix<P, Result<T, E>>` for `Result<Prefix<P, T>, E>`.
    pub fn transpose(self) -> Result<Prefix<P, T>, E> {
        Ok(Prefix {
            prefix: self.prefix,
            prefixed: self.prefixed?,
        })
    }
}

impl<T, S> Suffix<T, S> {
    /// Construct a `Suffix` from its two parts.
    pub fn new(suffixed: T, suffix: S) -> Self {
        Self { suffixed, suffix }
    }

    /// Apply `f` to the suffixed value, keeping `suffix` unchanged.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Suffix<U, S> {
        Suffix {
            suffixed: f(self.suffixed),
            suffix: self.suffix,
        }
    }
}

impl<T, S, E> Suffix<Result<T, E>, S> {
    /// Swap `Suffix<Result<T, E>, S>` for `Result<Suffix<T, S>, E>`.
    pub fn transpose(self) -> Result<Suffix<T, S>, E> {
        Ok(Suffix {
            suffixed: self.suffixed?,
            suffix: self.suffix,
        })
    }
}

/// Unwrap a grammar wrapper (e.g. [`Wrap`], [`Prefix`], [`Suffix`]) to get
/// at the value it wraps.
pub trait IntoInner<T> {
    /// Consume `self`, discarding whatever structure wrapped `T`.
    fn into_inner(self) -> T;
}

impl<L, T, R> IntoInner<T> for Wrap<L, T, R> {
    fn into_inner(self) -> T {
        self.wrapped
    }
}

impl<T, S> IntoInner<T> for Suffix<T, S> {
    fn into_inner(self) -> T {
        self.suffixed
    }
}

impl<P, T> IntoInner<T> for Prefix<P, T> {
    fn into_inner(self) -> T {
        self.prefixed
    }
}
