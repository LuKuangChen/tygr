//! Character class support for grammars.
//!
//! - [`CharClass`] — trait for character predicates
//! - [`CharOf<M>`] — match one character
//! - [`StringOf<C>`] — match zero or more characters
//! - [`StringOf1<C>`] — match one or more characters
//! - [`StringEq<T>`] — match a literal token described by a type-level chain

use std::char;
use std::fmt;
use std::marker::PhantomData;

use crate::bnf::Expr;
use crate::first::{CharFirst, CharFirstCI, ClassFirst, EmptyFirst, First};
use crate::grammar::Grammar;
#[cfg(feature = "trace_expect")]
use crate::state::PendingExpectation;
use crate::state::State;
use crate::{IntoInner, Raw};

/// Trait for character-class predicates.
///
/// The easiest way to define one is the [`char_class!`](crate::char_class)
/// macro:
///
/// ```
/// # use tygr::*;
/// char_class!(IsDigit, "digit", |ch| ch.is_ascii_digit());
/// char_class!(IsSpace, "space", |ch| matches!(ch, ' ' | '\t'));
/// char_class!(IsLower, "lower", |ch| matches!(ch, 'a'..='z'));
/// ```
///
/// For advanced cases you can implement the trait manually on a zero-sized
/// struct:
///
/// ```
/// # use tygr::*;
/// struct IsDigit;
/// impl CharClass for IsDigit {
///     fn matches(ch: char) -> bool { ch.is_ascii_digit() }
///     fn name() -> &'static str { "digit" }
/// }
/// ```
pub trait CharClass: 'static {
    /// Does `ch` belong to this character class?
    fn matches(ch: char) -> bool;

    /// Human-readable label (used in error messages and BNF).
    fn name() -> &'static str;

    /// The class's answer for every ASCII char, when it is known at compile
    /// time (see [`const_char_class!`](crate::const_char_class)). `None` (the default)
    /// means ASCII chars are decided by calling `matches`.
    const ASCII: Option<&'static [bool; 128]> = None;

    /// The class's answer for non-ASCII chars, when it is the same for all of
    /// them: `Some(true)` accepts every non-ASCII char (see [`AllChars`]),
    /// `Some(false)` rejects every one. `None` (the default) means each
    /// non-ASCII char is decoded and passed to `matches`.
    const NON_ASCII: Option<bool> = None;

    /// `matches` for an ASCII byte, from the compile-time table when there is one.
    #[inline(always)]
    fn matches_ascii(x: u8) -> bool {
        match Self::ASCII {
            Some(t) => t[(x & 0x7F) as usize],
            None => Self::matches(x as char),
        }
    }
}

/// The class of every char. Use it with [`Except`] to name what to exclude.
pub struct AllChars;

impl CharClass for AllChars {
    #[inline(always)]
    fn matches(_: char) -> bool {
        true
    }

    fn name() -> &'static str {
        "any char"
    }

    const ASCII: Option<&'static [bool; 128]> = Some(&[true; 128]);

    const NON_ASCII: Option<bool> = Some(true);
}

/// The class of no char. The identity for [`Union`].
pub struct NoChars;

impl CharClass for NoChars {
    #[inline(always)]
    fn matches(_: char) -> bool {
        false
    }

    fn name() -> &'static str {
        "no char"
    }

    const ASCII: Option<&'static [bool; 128]> = Some(&[false; 128]);

    const NON_ASCII: Option<bool> = Some(false);
}

/// The chars from `LO` to `HI`, inclusive.
pub struct CharRange<const LO: char, const HI: char>;

impl<const LO: char, const HI: char> CharClass for CharRange<LO, HI> {
    #[inline(always)]
    fn matches(ch: char) -> bool {
        LO <= ch && ch <= HI
    }

    fn name() -> &'static str {
        "char range"
    }

    const ASCII: Option<&'static [bool; 128]> = Some(&{
        let mut t = [false; 128];
        let mut i = 0;
        while i < 128 {
            t[i] = LO as u32 <= i as u32 && i as u32 <= HI as u32;
            i += 1;
        }
        t
    });

    const NON_ASCII: Option<bool> = if (HI as u32) < 0x80 {
        Some(false)
    } else if (LO as u32) <= 0x80 && HI == char::MAX {
        Some(true)
    } else {
        None
    };
}

/// The chars of a `StringEq!` literal, as a set: `StringEq!("\"\\")` is
/// the class of `"` and `\\`.
impl<T: CharList> CharClass for StringEq<T> {
    #[inline(always)]
    fn matches(ch: char) -> bool {
        T::contains(ch)
    }

    fn name() -> &'static str {
        "listed char"
    }

    const ASCII: Option<&'static [bool; 128]> = Some(&T::ASCII);

    const NON_ASCII: Option<bool> = if T::ALL_ASCII { Some(false) } else { None };
}

/// A `StringEq!` chain read as a set of chars.
#[doc(hidden)]
pub trait CharList: 'static {
    const ASCII: [bool; 128];
    const ALL_ASCII: bool;
    fn contains(ch: char) -> bool;
}

impl CharList for () {
    const ASCII: [bool; 128] = [false; 128];
    const ALL_ASCII: bool = true;
    #[inline(always)]
    fn contains(_: char) -> bool {
        false
    }
}

impl<const CH: char, T: CharList> CharList for CharThen<CH, T> {
    const ASCII: [bool; 128] = {
        let mut t = T::ASCII;
        if (CH as u32) < 128 {
            t[CH as usize] = true;
        }
        t
    };
    const ALL_ASCII: bool = T::ALL_ASCII && (CH as u32) < 128;
    #[inline(always)]
    fn contains(ch: char) -> bool {
        ch == CH || T::contains(ch)
    }
}

/// The chars in `A` or `B`.
pub struct Union<A, B>(PhantomData<(A, B)>);

impl<A: CharClass, B: CharClass> CharClass for Union<A, B> {
    #[inline(always)]
    fn matches(ch: char) -> bool {
        A::matches(ch) || B::matches(ch)
    }

    fn name() -> &'static str {
        A::name()
    }

    const ASCII: Option<&'static [bool; 128]> = match (A::ASCII, B::ASCII) {
        (Some(a), Some(b)) => Some(&{
            let mut t = [false; 128];
            let mut i = 0;
            while i < 128 {
                t[i] = a[i] || b[i];
                i += 1;
            }
            t
        }),
        _ => None,
    };

    const NON_ASCII: Option<bool> = match (A::NON_ASCII, B::NON_ASCII) {
        (Some(true), _) | (_, Some(true)) => Some(true),
        (Some(false), Some(false)) => Some(false),
        _ => None,
    };
}

/// The chars in `A` but not in `B`.
pub struct Except<A, B>(PhantomData<(A, B)>);

impl<A: CharClass, B: CharClass> CharClass for Except<A, B> {
    #[inline(always)]
    fn matches(ch: char) -> bool {
        A::matches(ch) && !B::matches(ch)
    }

    fn name() -> &'static str {
        A::name()
    }

    const ASCII: Option<&'static [bool; 128]> = match (A::ASCII, B::ASCII) {
        (Some(a), Some(b)) => Some(&{
            let mut t = [false; 128];
            let mut i = 0;
            while i < 128 {
                t[i] = a[i] && !b[i];
                i += 1;
            }
            t
        }),
        _ => None,
    };

    const NON_ASCII: Option<bool> = match (A::NON_ASCII, B::NON_ASCII) {
        (Some(false), _) | (_, Some(true)) => Some(false),
        (Some(true), Some(false)) => Some(true),
        _ => None,
    };
}

/// Matches exactly one character satisfying a character class.
pub struct CharOf<M: CharClass>(pub char, PhantomData<M>);

impl<M: CharClass> Clone for CharOf<M> {
    fn clone(&self) -> Self {
        Self(self.0, self.1)
    }
}

impl<M: CharClass> PartialEq for CharOf<M> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<M: CharClass> Eq for CharOf<M> {}

impl<M: CharClass> fmt::Debug for CharOf<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CharOf({:?})", self.0)
    }
}

impl<M: CharClass> CharOf<M> {
    /// Construct a `CharOf` from an already-known character.
    pub fn new(ch: char) -> Self {
        CharOf(ch, PhantomData)
    }

    /// The matched character.
    pub fn value(&self) -> char {
        self.0
    }
}

impl<M: CharClass> Grammar for CharOf<M> {
    type First = ClassFirst<M>;

    #[inline]
    fn parse_at(
        input: &str,
        pos: usize,
        #[allow(unused_variables, unused_mut)] mut state: State,
    ) -> Option<(Self, usize)> {
        let ch = match input.as_bytes().get(pos) {
            // If ASCII, skips &str invariant checks
            Some(&b) if b < 0x80 => {
                if M::matches(b as char) {
                    Some((b as char, 1))
                } else {
                    None
                }
            }
            Some(_) => input[pos..]
                .chars()
                .next()
                .filter(|c: &char| M::matches(*c))
                .map(|c| (c, c.len_utf8())),
            None => None,
        };
        if let Some((ch, len)) = ch {
            Some((CharOf(ch, PhantomData), pos + len))
        } else {
            #[cfg(feature = "trace_pos")]
            state.expect(
                pos,
                #[cfg(feature = "trace_expect")]
                PendingExpectation::CharClass(M::name()),
            );
            None
        }
    }

    #[inline]
    fn scan_at(input: &str, pos: usize, state: State) -> Option<usize> {
        Self::parse_at(input, pos, state).map(|(_, pos)| pos)
    }

    fn print_to(&self, buf: &mut String) {
        buf.push(self.0);
    }

    fn to_bnf() -> Expr {
        Expr::CharOf(M::name().to_string())
    }

    fn fail_at(
        #[allow(unused_variables)] pos: usize,
        #[allow(unused_variables, unused_mut)] mut state: State,
    ) -> bool {
        #[cfg(feature = "trace_pos")]
        state.expect(
            pos,
            #[cfg(feature = "trace_expect")]
            PendingExpectation::CharClass(M::name()),
        );
        true
    }
}

/// Length of the longest run of chars in class `C` starting at `pos`.
#[inline(always)]
fn scan_run<C: CharClass>(input: &str, mut pos: usize) -> usize {
    let b = input.as_bytes();
    // Per-byte answers, derived at compile time: ASCII bytes from `ASCII`
    // (or `matches` when the class has no table), and bytes >= 0x80 accepted
    // only when every non-ASCII char is (`NON_ASCII == Some(true)`).
    let accepts: &[bool; 256] = &const {
        let mut t = [false; 256];
        if let Some(ascii) = C::ASCII {
            let mut i = 0;
            while i < 128 {
                t[i] = ascii[i];
                i += 1;
            }
        }
        if let Some(true) = C::NON_ASCII {
            let mut i = 128;
            while i < 256 {
                t[i] = true;
                i += 1;
            }
        }
        t
    };
    while let Some(&x) = b.get(pos) {
        // Optimistic: the byte is in the class.
        let hit = match C::ASCII {
            Some(_) => accepts[x as usize],
            None => x < 0x80 && C::matches(x as char),
        };
        if hit {
            pos += 1;
            continue;
        }
        // ASCII not in the class: the run ends.
        if x < 0x80 {
            break;
        }
        // A non-ASCII char the table didn't accept: decode and ask, unless
        // the class rejects every non-ASCII char.
        if C::NON_ASCII == Some(false) {
            break;
        }
        let c = input[pos..].chars().next().unwrap();
        if !C::matches(c) {
            break;
        }
        pos += c.len_utf8();
    }
    pos
}

/// Matches *zero or more* characters satisfying a character class, collected into a `String`.
///
/// ```
/// # use tygr::*;
/// # char_class!(IsSpace, "space", |ch| matches!(ch, ' ' | '\t'));
/// type Ws = StringOf<IsSpace>;  // optional whitespace
/// ```
pub struct StringOf<C: CharClass>(Raw<Vec<CharOf<C>>>);

impl<C: CharClass> Grammar for StringOf<C> {
    type First = <Raw<Vec<CharOf<C>>> as Grammar>::First;

    #[inline]
    fn parse_at(input: &str, pos: usize, state: State) -> Option<(Self, usize)> {
        let end = Self::scan_at(input, pos, state)?;
        Some((StringOf(Raw::new(&input[pos..end])), end))
    }

    #[inline(always)]
    fn scan_at(
        input: &str,
        pos: usize,
        #[allow(unused_variables, unused_mut)] mut state: State,
    ) -> Option<usize> {
        let end = scan_run::<C>(input, pos);
        #[cfg(feature = "trace_pos")]
        CharOf::<C>::fail_at(end, state);
        Some(end)
    }

    fn print_to(&self, buf: &mut String) {
        self.0.print_to(buf);
    }

    fn to_bnf() -> Expr {
        Raw::<Vec<CharOf<C>>>::to_bnf()
    }

    fn fail_at(pos: usize, state: State) -> bool {
        Raw::<Vec<CharOf<C>>>::fail_at(pos, state)
    }
}

/// Matches *one or more* characters satisfying a character class, collected into a `String`.
///
/// ```
/// # use tygr::*;
/// # char_class!(IsDigit, "digit", |ch| ch.is_ascii_digit());
/// #[derive(Grammar)]
/// struct Number(StringOf1<IsDigit>);  // one or more digits
/// ```
pub struct StringOf1<C: CharClass>(Raw<(CharOf<C>, Vec<CharOf<C>>)>);

impl<C: CharClass> Grammar for StringOf1<C> {
    type First = <Raw<(CharOf<C>, Vec<CharOf<C>>)> as Grammar>::First;

    #[inline]
    fn parse_at(input: &str, pos: usize, state: State) -> Option<(Self, usize)> {
        let end = Self::scan_at(input, pos, state)?;
        Some((StringOf1(Raw::new(&input[pos..end])), end))
    }

    #[inline(always)]
    fn scan_at(
        input: &str,
        pos: usize,
        #[allow(unused_variables, unused_mut)] mut state: State,
    ) -> Option<usize> {
        let end = scan_run::<C>(input, pos);
        if end == pos {
            CharOf::<C>::fail_at(pos, state);
            return None;
        }
        #[cfg(feature = "trace_pos")]
        CharOf::<C>::fail_at(end, state);
        Some(end)
    }

    fn print_to(&self, buf: &mut String) {
        self.0.print_to(buf);
    }

    fn to_bnf() -> Expr {
        Raw::<(CharOf<C>, Vec<CharOf<C>>)>::to_bnf()
    }

    fn fail_at(pos: usize, state: State) -> bool {
        Raw::<(CharOf<C>, Vec<CharOf<C>>)>::fail_at(pos, state)
    }
}

/// Delegates the string-facing trait impls to the inner `Raw`.
macro_rules! impl_string_wrapper {
    ($ty:ident) => {
        impl<C: CharClass> fmt::Debug for $ty<C> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Debug::fmt(&self.0, f)
            }
        }
        impl<C: CharClass> fmt::Display for $ty<C> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }
        impl<C: CharClass> Clone for $ty<C> {
            fn clone(&self) -> Self {
                $ty(self.0.clone())
            }
        }
        impl<C: CharClass> PartialEq for $ty<C> {
            fn eq(&self, other: &Self) -> bool {
                self.0 == other.0
            }
        }
        impl<C: CharClass> Eq for $ty<C> {}
        impl<C: CharClass> std::hash::Hash for $ty<C> {
            fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                self.0.hash(state);
            }
        }
        impl<C: CharClass> std::ops::Deref for $ty<C> {
            type Target = str;
            fn deref(&self) -> &str {
                &self.0
            }
        }
        impl<C: CharClass> AsRef<str> for $ty<C> {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
        impl<C: CharClass> IntoInner<String> for $ty<C> {
            fn into_inner(self) -> String {
                self.0.0
            }
        }
    };
}

impl_string_wrapper!(StringOf);
impl_string_wrapper!(StringOf1);

/// Describes a literal-token chain: enough to match (`scan_at`) and describe
/// (`add_expectation`) itself; no `Self` value is ever built.
// `pub`, not private: `StringEq<T>`'s `Grammar::First = T::First` would
// otherwise leak a private trait's associated type (E0446).
#[doc(hidden)]
pub trait Token: 'static {
    type First: First;

    // Err: the position the mismatch actually occurred at (past `pos` if a
    // prefix of the chain matched), paired with what was expected there.
    fn scan_at(input: &str, pos: usize, state: State) -> Result<usize, (usize, fn() -> String)>;

    fn add_expectation(str: &mut String);

    fn expectation() -> String {
        let mut str = String::new();
        Self::add_expectation(&mut str);
        str
    }
}

impl Token for () {
    type First = EmptyFirst;

    fn scan_at(_input: &str, pos: usize, _state: State) -> Result<usize, (usize, fn() -> String)> {
        Ok(pos)
    }

    fn add_expectation(_: &mut String) {}
}

/// Matches one literal character, then continues to the rest of the chain.
/// `StringEq!("…")` expands to a chain of these.
pub struct CharThen<const CH: char, T>(PhantomData<T>);

impl<const CH: char, T: Token> Token for CharThen<CH, T> {
    type First = CharFirst<CH>;

    #[inline]
    fn scan_at(
        input: &str,
        pos: usize,
        #[allow(unused_variables, unused_mut)] mut state: State,
    ) -> Result<usize, (usize, fn() -> String)> {
        // Compare bytes directly: slicing `input[pos..]` would add a
        // char-boundary check on every literal probe.
        let mut buf = [0u8; 4];
        let lit = CH.encode_utf8(&mut buf).as_bytes();
        if input.as_bytes()[pos..].starts_with(lit) {
            T::scan_at(input, pos + CH.len_utf8(), state)
        } else {
            Err((pos, Self::expectation))
        }
    }

    fn add_expectation(str: &mut String) {
        str.push(CH);
        T::add_expectation(str);
    }
}

/// Case-insensitive [`CharThen`]. `StringEqCI!("…")` expands to a chain of these.
pub struct CharCIThen<const CH: char, T>(PhantomData<T>);

impl<const CH: char, T: Token> Token for CharCIThen<CH, T> {
    type First = CharFirstCI<CH>;

    #[inline]
    fn scan_at(
        input: &str,
        pos: usize,
        #[allow(unused_variables, unused_mut)] mut state: State,
    ) -> Result<usize, (usize, fn() -> String)> {
        match input[pos..].chars().next() {
            Some(c) if c.eq_ignore_ascii_case(&CH) => T::scan_at(input, pos + c.len_utf8(), state),
            _ => Err((pos, Self::expectation)),
        }
    }

    fn add_expectation(str: &mut String) {
        str.push(CH);
        T::add_expectation(str);
    }
}

/// `Grammar` for a `StringEq!` chain. Case-sensitive, so the matched text is
/// always exactly `T::expectation()` — nothing needs to be stored per match.
pub struct StringEq<T>(PhantomData<T>);

impl<T: Token> Grammar for StringEq<T> {
    type First = T::First;

    #[inline]
    fn parse_at(input: &str, pos: usize, state: State) -> Option<(Self, usize)> {
        let end = Self::scan_at(input, pos, state)?;
        Some((StringEq(PhantomData), end))
    }

    #[inline]
    #[cfg_attr(not(feature = "trace_pos"), allow(clippy::manual_ok_err))]
    fn scan_at(input: &str, pos: usize, mut state: State) -> Option<usize> {
        match T::scan_at(input, pos, state.reborrow()) {
            Ok(pos) => Some(pos),
            #[allow(unused_variables)]
            Err((fail_pos, expectation)) => {
                #[cfg(feature = "trace_pos")]
                state.expect(
                    fail_pos,
                    #[cfg(feature = "trace_expect")]
                    PendingExpectation::StringEq(expectation, T::expectation),
                );
                None
            }
        }
    }

    fn print_to(&self, buf: &mut String) {
        T::add_expectation(buf);
    }

    fn to_bnf() -> Expr {
        Expr::StringEq(T::expectation())
    }

    fn fail_at(
        #[allow(unused_variables)] pos: usize,
        #[allow(unused_variables, unused_mut)] mut state: State,
    ) -> bool {
        #[cfg(feature = "trace_pos")]
        state.expect(
            pos,
            #[cfg(feature = "trace_expect")]
            PendingExpectation::StringEq(T::expectation, T::expectation),
        );
        true
    }
}

impl<T: Token> fmt::Debug for StringEq<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("StringEq").field(&T::expectation()).finish()
    }
}

// Hand-written: derive would add a `T` bound, but `T` is a phantom marker
// that's never stored, so these hold unconditionally.
impl<T> Clone for StringEq<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for StringEq<T> {}

impl<T> Default for StringEq<T> {
    fn default() -> Self {
        StringEq(PhantomData)
    }
}

impl<T> PartialEq for StringEq<T> {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl<T> Eq for StringEq<T> {}

/// `Grammar` for a `StringEqCI!` chain. Case-insensitive, so the matched text
/// can differ from `T::expectation()`'s casing — it's captured once here
/// rather than per chain link.
pub struct StringEqCI<T>(String, PhantomData<T>);

impl<T: Token> Grammar for StringEqCI<T> {
    type First = T::First;

    #[inline]
    fn parse_at(input: &str, pos: usize, state: State) -> Option<(Self, usize)> {
        let end = Self::scan_at(input, pos, state)?;
        Some((StringEqCI(input[pos..end].to_string(), PhantomData), end))
    }

    #[inline]
    #[cfg_attr(not(feature = "trace_pos"), allow(clippy::manual_ok_err))]
    fn scan_at(input: &str, pos: usize, mut state: State) -> Option<usize> {
        match T::scan_at(input, pos, state.reborrow()) {
            Ok(pos) => Some(pos),
            #[allow(unused_variables)]
            Err((fail_pos, expectation)) => {
                #[cfg(feature = "trace_pos")]
                state.expect(
                    fail_pos,
                    #[cfg(feature = "trace_expect")]
                    PendingExpectation::StringEqCI(expectation, T::expectation),
                );
                None
            }
        }
    }

    fn print_to(&self, buf: &mut String) {
        buf.push_str(&self.0);
    }

    fn to_bnf() -> Expr {
        Expr::StringEqCI(T::expectation())
    }

    fn fail_at(
        #[allow(unused_variables)] pos: usize,
        #[allow(unused_variables, unused_mut)] mut state: State,
    ) -> bool {
        #[cfg(feature = "trace_pos")]
        state.expect(
            pos,
            #[cfg(feature = "trace_expect")]
            PendingExpectation::StringEqCI(T::expectation, T::expectation),
        );
        true
    }
}

impl<T> fmt::Debug for StringEqCI<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("StringEqCI").field(&self.0).finish()
    }
}

impl<T> Clone for StringEqCI<T> {
    fn clone(&self) -> Self {
        StringEqCI(self.0.clone(), PhantomData)
    }
}

impl<T: Token> Default for StringEqCI<T> {
    fn default() -> Self {
        StringEqCI(T::expectation(), PhantomData)
    }
}

impl<T> PartialEq for StringEqCI<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<T> Eq for StringEqCI<T> {}
