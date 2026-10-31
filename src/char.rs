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

    /// For each byte, the answer for every char whose UTF-8 encoding starts
    /// with it: all match, none do, or ask [`check_at`](CharClass::check_at).
    /// Computed by [`const_char_class!`](crate::const_char_class) and the set
    /// types ([`AllChars`], [`Except`], ...); the default asks every time.
    #[doc(hidden)]
    const LEAD_BYTES: [Lead; 256] = lead::CHECK;

    /// Is the char at byte `pos` in the class? Called only where
    /// `LEAD_BYTES` says [`Lead::Check`]. The set types answer from the bytes;
    /// the default decodes the char and calls `matches`.
    #[doc(hidden)]
    #[inline(always)]
    fn check_at(input: &[u8], pos: usize) -> bool {
        decode_at(input, pos, input[pos]).is_some_and(Self::matches)
    }

    /// Is the char at byte `pos`, whose first byte is `first`, in the class?
    /// `LEAD_BYTES`, then `check_at`.
    #[doc(hidden)]
    #[inline(always)]
    fn has_char_at(input: &[u8], pos: usize, first: u8) -> bool {
        match Self::LEAD_BYTES[first as usize] {
            Lead::Accept => true,
            Lead::Reject => false,
            Lead::Check => Self::check_at(input, pos),
        }
    }
}

/// What a char class says about every char that starts with a given byte.
#[doc(hidden)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lead {
    /// Every such char is in the class.
    Accept,
    /// No such char is, or the byte never starts a char.
    Reject,
    /// Some are: ask [`CharClass::check_at`].
    Check,
}

/// Compile-time construction of [`CharClass::LEAD_BYTES`] tables.
#[doc(hidden)]
pub mod lead {
    use super::Lead::{self, *};

    /// The chars (as an inclusive range of scalar values) whose UTF-8
    /// encoding starts with byte `b`, if any.
    pub const fn span(b: u8) -> Option<(u32, u32)> {
        let b32 = b as u32;
        match b {
            0x00..=0x7F => Some((b32, b32)),
            0xC2..=0xDF => Some(((b32 & 0x1F) << 6, ((b32 & 0x1F) << 6) | 0x3F)),
            0xE0 => Some((0x800, 0xFFF)),
            0xED => Some((0xD000, 0xD7FF)),
            0xE1..=0xEF => Some(((b32 & 0x0F) << 12, ((b32 & 0x0F) << 12) | 0xFFF)),
            0xF0 => Some((0x10000, 0x3FFFF)),
            0xF1..=0xF3 => Some(((b32 & 0x07) << 18, ((b32 & 0x07) << 18) | 0x3FFFF)),
            0xF4 => Some((0x100000, 0x10FFFF)),
            _ => None,
        }
    }

    const fn fill(answer: Lead) -> [Lead; 256] {
        let mut t = [Reject; 256];
        let mut i = 0;
        while i < 256 {
            if span(i as u8).is_some() {
                t[i] = answer;
            }
            i += 1;
        }
        t
    }

    pub const ACCEPT: [Lead; 256] = fill(Accept);
    pub const REJECT: [Lead; 256] = fill(Reject);
    pub const CHECK: [Lead; 256] = fill(Check);

    /// The chars from `lo` to `hi`, inclusive.
    pub const fn range(lo: char, hi: char) -> [Lead; 256] {
        let (lo, hi) = (lo as u32, hi as u32);
        let mut t = [Reject; 256];
        let mut i = 0;
        while i < 256 {
            if let Some((a, b)) = span(i as u8) {
                t[i] = if lo <= a && b <= hi {
                    Accept
                } else if b < lo || hi < a {
                    Reject
                } else {
                    Check
                };
            }
            i += 1;
        }
        t
    }

    pub const fn union(a: &[Lead; 256], b: &[Lead; 256]) -> [Lead; 256] {
        let mut t = [Reject; 256];
        let mut i = 0;
        while i < 256 {
            t[i] = match (a[i], b[i]) {
                (Accept, _) | (_, Accept) => Accept,
                (Reject, Reject) => Reject,
                _ => Check,
            };
            i += 1;
        }
        t
    }

    pub const fn except(a: &[Lead; 256], b: &[Lead; 256]) -> [Lead; 256] {
        let mut t = [Reject; 256];
        let mut i = 0;
        while i < 256 {
            t[i] = match (a[i], b[i]) {
                (Reject, _) | (_, Accept) => Reject,
                (Accept, Reject) => Accept,
                _ => Check,
            };
            i += 1;
        }
        t
    }

    /// A char range as UTF-8 byte-sequence ranges: a char is in the range
    /// exactly when its bytes fall in one of the pieces, byte by byte.
    #[derive(Clone, Copy)]
    pub struct Piece {
        len: usize,
        lo: [u8; 4],
        hi: [u8; 4],
    }

    pub struct Pieces {
        list: [Piece; 32],
        n: usize,
    }

    const fn max_of_len(len: usize) -> u32 {
        match len {
            1 => 0x7F,
            2 => 0x7FF,
            3 => 0xFFFF,
            _ => 0x10FFFF,
        }
    }

    const fn encode(c: u32) -> ([u8; 4], usize) {
        let mut buf = [0u8; 4];
        let len = match char::from_u32(c) {
            Some(ch) => ch.encode_utf8(&mut buf).len(),
            None => panic!("not a char"),
        };
        (buf, len)
    }

    /// Split `lo..=hi` into byte-sequence ranges (as regex-syntax's
    /// `Utf8Sequences` does): first where the encoded length changes and
    /// around the surrogates, then until every byte position is either one
    /// aligned sub-range or a full continuation range.
    pub const fn pieces(lo: char, hi: char) -> Pieces {
        let empty = Piece {
            len: 0,
            lo: [0; 4],
            hi: [0; 4],
        };
        let mut out = Pieces {
            list: [empty; 32],
            n: 0,
        };
        let mut stack = [(0u32, 0u32); 32];
        let mut sp = 0;
        if (lo as u32) <= (hi as u32) {
            stack[0] = (lo as u32, hi as u32);
            sp = 1;
        }
        while sp > 0 {
            sp -= 1;
            let (s, mut e) = stack[sp];
            'split: loop {
                if s < 0xD800 && e > 0xDFFF {
                    stack[sp] = (0xE000, e);
                    sp += 1;
                    e = 0xD7FF;
                }
                let mut len = 1;
                while len < 4 {
                    let m = max_of_len(len);
                    if s <= m && m < e {
                        stack[sp] = (m + 1, e);
                        sp += 1;
                        e = m;
                        continue 'split;
                    }
                    len += 1;
                }
                let mut i = 1;
                while i < 4 {
                    let max = (1u32 << (6 * i)) - 1;
                    if (s & !max) != (e & !max) {
                        if (s & max) != 0 {
                            stack[sp] = ((s | max) + 1, e);
                            sp += 1;
                            e = s | max;
                            continue 'split;
                        }
                        if (e & max) != max {
                            stack[sp] = (e & !max, e);
                            sp += 1;
                            e = (e & !max) - 1;
                            continue 'split;
                        }
                    }
                    i += 1;
                }
                let (a, len) = encode(s);
                let (b, _) = encode(e);
                out.list[out.n] = Piece { len, lo: a, hi: b };
                out.n += 1;
                break;
            }
        }
        out
    }

    /// Do the bytes at `pos` fall in one of `p`'s pieces?
    #[inline(always)]
    pub fn pieces_match(p: &Pieces, input: &[u8], pos: usize) -> bool {
        let b = &input[pos..];
        p.list[..p.n]
            .iter()
            .any(|q| q.len <= b.len() && (0..q.len).all(|k| q.lo[k] <= b[k] && b[k] <= q.hi[k]))
    }

    /// `t` with the chars that start with byte `b` added: settled if `b`
    /// starts only this one char, otherwise checked.
    pub const fn with_char(mut t: [Lead; 256], c: char) -> [Lead; 256] {
        let mut buf = [0u8; 4];
        let b = c.encode_utf8(&mut buf).as_bytes()[0] as usize;
        t[b] = match (t[b], span(b as u8)) {
            (Accept, _) => Accept,
            (_, Some((a, z))) if a == z => Accept,
            _ => Check,
        };
        t
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

    const LEAD_BYTES: [Lead; 256] = lead::ACCEPT;
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

    const LEAD_BYTES: [Lead; 256] = lead::REJECT;
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

    const LEAD_BYTES: [Lead; 256] = lead::range(LO, HI);

    #[inline(always)]
    fn check_at(input: &[u8], pos: usize) -> bool {
        lead::pieces_match(&Self::PIECES, input, pos)
    }
}

impl<const LO: char, const HI: char> CharRange<LO, HI> {
    const PIECES: lead::Pieces = lead::pieces(LO, HI);
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

    const LEAD_BYTES: [Lead; 256] = T::LEAD_BYTES;

    #[inline(always)]
    fn check_at(input: &[u8], pos: usize) -> bool {
        T::starts(&input[pos..])
    }
}

/// A `StringEq!` chain read as a set of chars.
#[doc(hidden)]
pub trait CharList: 'static {
    const LEAD_BYTES: [Lead; 256];
    fn contains(ch: char) -> bool;
    /// Do `bytes` start with one of the chars?
    fn starts(bytes: &[u8]) -> bool;
}

impl CharList for () {
    const LEAD_BYTES: [Lead; 256] = lead::REJECT;
    #[inline(always)]
    fn contains(_: char) -> bool {
        false
    }
    #[inline(always)]
    fn starts(_: &[u8]) -> bool {
        false
    }
}

impl<const CH: char, T: CharList> CharList for CharThen<CH, T> {
    const LEAD_BYTES: [Lead; 256] = lead::with_char(T::LEAD_BYTES, CH);
    #[inline(always)]
    fn contains(ch: char) -> bool {
        ch == CH || T::contains(ch)
    }
    #[inline(always)]
    fn starts(bytes: &[u8]) -> bool {
        let mut buf = [0u8; 4];
        bytes.starts_with(CH.encode_utf8(&mut buf).as_bytes()) || T::starts(bytes)
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

    const LEAD_BYTES: [Lead; 256] = lead::union(&A::LEAD_BYTES, &B::LEAD_BYTES);

    #[inline(always)]
    fn check_at(input: &[u8], pos: usize) -> bool {
        let b = input[pos];
        A::has_char_at(input, pos, b) || B::has_char_at(input, pos, b)
    }
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

    const LEAD_BYTES: [Lead; 256] = lead::except(&A::LEAD_BYTES, &B::LEAD_BYTES);

    #[inline(always)]
    fn check_at(input: &[u8], pos: usize) -> bool {
        let b = input[pos];
        A::has_char_at(input, pos, b) && !B::has_char_at(input, pos, b)
    }
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
    #[inline(always)]
    fn parse_at(
        input: &[u8],
        pos: usize,
        head: u32,
        #[allow(unused_variables, unused_mut)] mut state: State,
    ) -> Option<(Self, usize)> {
        let ch = if head < 256 && M::has_char_at(input, pos, head as u8) {
            decode_at(input, pos, head as u8).map(|c| (c, c.len_utf8()))
        } else {
            None
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

    #[inline(always)]
    fn scan_at(input: &[u8], pos: usize, head: u32, state: State) -> Option<usize> {
        Self::parse_at(input, pos, head, state).map(|(_, pos)| pos)
    }

    fn print_to(&self, buf: &mut String) {
        buf.push(self.0);
    }

    fn to_bnf() -> Expr {
        Expr::CharOf(M::name().to_string())
    }
}

/// Record that a char of class `C` was expected at `pos`.
#[inline(always)]
fn expect_char<C: CharClass>(
    #[allow(unused_variables)] pos: usize,
    #[allow(unused_variables, unused_mut)] mut state: State,
) {
    #[cfg(feature = "trace_pos")]
    state.expect(
        pos,
        #[cfg(feature = "trace_expect")]
        PendingExpectation::CharClass(C::name()),
    );
}

/// The char starting at byte `pos`, whose first byte is `b`, or `None` if
/// the bytes there are not UTF-8.
#[inline(always)]
fn decode_at(input: &[u8], pos: usize, b: u8) -> Option<char> {
    if b < 0x80 {
        return Some(b as char);
    }
    // Leading ones give the sequence length; a byte that cannot start one
    // gets a length `from_utf8` will reject.
    let len = (b.leading_ones() as usize).clamp(2, 4);
    let bytes = input.get(pos..pos + len)?;
    std::str::from_utf8(bytes).ok()?.chars().next()
}

/// Length of the longest run of chars in class `C` starting at `pos`.
#[inline(always)]
fn scan_run<C: CharClass>(input: &[u8], mut pos: usize) -> usize {
    let b = input;
    // Bytes that start only accepted chars, plus continuation bytes: the
    // loop meets one only right after accepting the byte before it, which
    // was part of the same char. So an accepted char is consumed a byte at a
    // time, and the run stops only where a char starts.
    let accept: &[bool; 256] = &const {
        let mut t = [false; 256];
        let mut i = 0;
        while i < 256 {
            t[i] = matches!(C::LEAD_BYTES[i], Lead::Accept) || (0x80 <= i && i < 0xC0);
            i += 1;
        }
        t
    };
    let check: &[bool; 256] = &const {
        let mut t = [false; 256];
        let mut i = 0;
        while i < 256 {
            t[i] = matches!(C::LEAD_BYTES[i], Lead::Check);
            i += 1;
        }
        t
    };
    let any_check = const {
        let mut any = false;
        let mut i = 0;
        while i < 256 {
            any = any || matches!(C::LEAD_BYTES[i], Lead::Check);
            i += 1;
        }
        any
    };
    while let Some(&x) = b.get(pos) {
        if accept[x as usize] {
            pos += 1;
            continue;
        }
        if !any_check || !check[x as usize] {
            break;
        }
        if !C::check_at(input, pos) {
            break;
        }
        pos += if x < 0x80 {
            1
        } else {
            x.leading_ones() as usize
        };
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
    #[inline(always)]
    fn parse_at(input: &[u8], pos: usize, head: u32, state: State) -> Option<(Self, usize)> {
        let end = Self::scan_at(input, pos, head, state)?;
        Some((StringOf(Raw::from_bytes(&input[pos..end])), end))
    }

    #[inline(always)]
    fn scan_at(
        input: &[u8],
        pos: usize,
        _head: u32,
        #[allow(unused_variables, unused_mut)] mut state: State,
    ) -> Option<usize> {
        let end = scan_run::<C>(input, pos);
        #[cfg(feature = "trace_pos")]
        expect_char::<C>(end, state);
        Some(end)
    }

    fn print_to(&self, buf: &mut String) {
        self.0.print_to(buf);
    }

    fn to_bnf() -> Expr {
        Raw::<Vec<CharOf<C>>>::to_bnf()
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
    #[inline(always)]
    fn parse_at(input: &[u8], pos: usize, head: u32, state: State) -> Option<(Self, usize)> {
        let end = Self::scan_at(input, pos, head, state)?;
        Some((StringOf1(Raw::from_bytes(&input[pos..end])), end))
    }

    #[inline(always)]
    fn scan_at(
        input: &[u8],
        pos: usize,
        _head: u32,
        #[allow(unused_variables, unused_mut)] mut state: State,
    ) -> Option<usize> {
        let end = scan_run::<C>(input, pos);
        if end == pos {
            expect_char::<C>(pos, state);
            return None;
        }
        #[cfg(feature = "trace_pos")]
        expect_char::<C>(end, state);
        Some(end)
    }

    fn print_to(&self, buf: &mut String) {
        self.0.print_to(buf);
    }

    fn to_bnf() -> Expr {
        Raw::<(CharOf<C>, Vec<CharOf<C>>)>::to_bnf()
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
            type Target = [u8];
            fn deref(&self) -> &[u8] {
                &self.0
            }
        }
        impl<C: CharClass> AsRef<[u8]> for $ty<C> {
            fn as_ref(&self) -> &[u8] {
                &self.0
            }
        }
        impl<C: CharClass> IntoInner<Vec<u8>> for $ty<C> {
            fn into_inner(self) -> Vec<u8> {
                self.0.into_bytes()
            }
        }
        impl<C: CharClass> $ty<C> {
            /// The matched bytes.
            pub fn as_bytes(&self) -> &[u8] {
                self.0.as_bytes()
            }

            /// The matched text, if it is UTF-8.
            pub fn as_str(&self) -> Result<&str, std::str::Utf8Error> {
                self.0.as_str()
            }

            /// The matched text, owned, if it is UTF-8.
            pub fn into_string(self) -> Result<String, std::string::FromUtf8Error> {
                self.0.into_string()
            }
        }
    };
}

impl_string_wrapper!(StringOf);
impl_string_wrapper!(StringOf1);

/// Describes a literal-token chain: enough to match (`scan_at`) and describe
/// (`add_expectation`) itself; no `Self` value is ever built.
#[doc(hidden)]
pub trait Token: 'static {
    // Err: the position the mismatch actually occurred at (past `pos` if a
    // prefix of the chain matched), paired with what was expected there.
    fn scan_at(
        input: &[u8],
        pos: usize,
        head: u32,
        state: State,
    ) -> Result<usize, (usize, fn() -> String)>;

    fn add_expectation(str: &mut String);

    fn expectation() -> String {
        let mut str = String::new();
        Self::add_expectation(&mut str);
        str
    }
}

impl Token for () {
    fn scan_at(
        _input: &[u8],
        pos: usize,
        _head: u32,
        _state: State,
    ) -> Result<usize, (usize, fn() -> String)> {
        Ok(pos)
    }

    fn add_expectation(_: &mut String) {}
}

/// Matches one literal character, then continues to the rest of the chain.
/// `StringEq!("…")` expands to a chain of these.
pub struct CharThen<const CH: char, T>(PhantomData<T>);

impl<const CH: char, T: Token> Token for CharThen<CH, T> {
    #[inline(always)]
    fn scan_at(
        input: &[u8],
        pos: usize,
        head: u32,
        #[allow(unused_variables, unused_mut)] mut state: State,
    ) -> Result<usize, (usize, fn() -> String)> {
        // The first byte is `head`; any further bytes of a multi-byte char
        // come from the input.
        let mut buf = [0u8; 4];
        let lit = CH.encode_utf8(&mut buf).as_bytes();
        if head == lit[0] as u32
            && input
                .get(pos + 1..)
                .is_some_and(|rest| rest.starts_with(&lit[1..]))
        {
            let next = pos + lit.len();
            T::scan_at(input, next, crate::head_at(input, next), state)
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
    #[inline(always)]
    fn scan_at(
        input: &[u8],
        pos: usize,
        head: u32,
        #[allow(unused_variables, unused_mut)] mut state: State,
    ) -> Result<usize, (usize, fn() -> String)> {
        let mut buf = [0u8; 4];
        let lit = CH.encode_utf8(&mut buf).as_bytes();
        let hit = if CH.is_ascii() {
            head < 256 && (head as u8).eq_ignore_ascii_case(&(CH as u8))
        } else {
            head == lit[0] as u32
                && input
                    .get(pos + 1..)
                    .is_some_and(|rest| rest.starts_with(&lit[1..]))
        };
        if hit {
            let next = pos + lit.len();
            T::scan_at(input, next, crate::head_at(input, next), state)
        } else {
            Err((pos, Self::expectation))
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
    #[inline(always)]
    fn parse_at(input: &[u8], pos: usize, head: u32, state: State) -> Option<(Self, usize)> {
        let end = Self::scan_at(input, pos, head, state)?;
        Some((StringEq(PhantomData), end))
    }

    #[inline(always)]
    #[cfg_attr(not(feature = "trace_pos"), allow(clippy::manual_ok_err))]
    fn scan_at(input: &[u8], pos: usize, head: u32, mut state: State) -> Option<usize> {
        match T::scan_at(input, pos, head, state.reborrow()) {
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
    #[inline(always)]
    fn parse_at(input: &[u8], pos: usize, head: u32, state: State) -> Option<(Self, usize)> {
        let end = Self::scan_at(input, pos, head, state)?;
        // The match is the literal with some ASCII letters' case changed,
        // so it is UTF-8 whatever the rest of the input is.
        let text =
            String::from_utf8(input[pos..end].to_vec()).expect("case variant of a str literal");
        Some((StringEqCI(text, PhantomData), end))
    }

    #[inline(always)]
    #[cfg_attr(not(feature = "trace_pos"), allow(clippy::manual_ok_err))]
    fn scan_at(input: &[u8], pos: usize, head: u32, mut state: State) -> Option<usize> {
        match T::scan_at(input, pos, head, state.reborrow()) {
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
