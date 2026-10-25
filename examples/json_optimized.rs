//! JSON (optimized grammar) parser/printer/EBNF, driven from the command line.
//!
//!   cargo run --example json_optimized -- bnf
//!   echo '{"a": [1, true, null]}' | cargo run --example json_optimized -- parse
//!   echo '{"a": 1}' | cargo run --example json_optimized -- test
//!
//! Same language as `json`, hand-tuned for parse throughput; see
//! `benches/json_optimized.rs`.

use std::marker::PhantomData;
use std::process::ExitCode;

use tygr::*;

#[path = "common/mod.rs"]
mod common;

#[derive(Grammar, Debug)]
struct Json(Wrap<Ws, Element, Ws>);

#[derive(Grammar, Debug)]
enum Element {
    Object(Object),
    Array(Array),
    String(String),
    Number(Range<PhantomData<Number>>),
    True(StringEq!("true")),
    False(StringEq!("false")),
    Null(StringEq!("null")),
}

#[derive(Grammar, Debug)]
struct Object(Wrap<(StringEq!("{"), Ws), Option<Members>, StringEq!("}")>);

#[derive(Grammar, Debug)]
struct Members(VecSep<Suffix<Member, Ws>, Suffix<StringEq!(","), Ws>>);

#[derive(Grammar, Debug)]
struct Member {
    pub string: String,
    col: Wrap<Ws, StringEq!(":"), Ws>,
    pub value: Element,
}

#[derive(Grammar, Debug)]
struct Array(Wrap<(StringEq!("["), Ws), Option<Elements>, StringEq!("]")>);

#[derive(Grammar, Debug)]
struct Elements(VecSep<Suffix<Element, Ws>, Suffix<StringEq!(","), Ws>>);

#[derive(Grammar, Debug)]
struct String(Wrap<StringEq!("\""), Range<PhantomData<Characters>>, StringEq!("\"")>);

#[derive(Grammar, Debug)]
struct Characters(StringOf<NormalCharacter>, Vec<EscapedRun>);

/// An escape followed by the ordinary characters after it.
#[derive(Grammar, Debug)]
struct EscapedRun(Prefix<StringEq!("\\"), Escape>, StringOf<NormalCharacter>);

const_char_class!(AsciiNormal, "0x20-0x7F except for \" and \\", |c| {
    matches!(c, ' '..='\u{7F}') && c != '"' && c != '\\'
});
const_char_class!(
    NormalCharacter,
    "0x20-0x10FFFF except for \" and \\",
    AsciiNormal | AnyNonAscii
);

#[derive(Grammar, Debug)]
enum Escape {
    DQ(StringEq!("\"")),
    BS(StringEq!("\\")),
    FS(StringEq!("/")),
    B(StringEq!("b")),
    F(StringEq!("f")),
    N(StringEq!("n")),
    R(StringEq!("r")),
    T(StringEq!("t")),
    U {
        u: StringEq!("u"),
        hex1: Hex,
        hex2: Hex,
        hex3: Hex,
        hex4: Hex,
    },
}

#[derive(Grammar, Debug)]
enum Hex {
    Digit(Digit),
    UAToF(CharOf<UAToF>),
    LAToF(CharOf<LAToF>),
}

const_char_class!(UAToF, "A-F", |c| matches!(c, 'A'..='F'));
const_char_class!(LAToF, "a-f", |c| matches!(c, 'a'..='f'));

#[derive(Grammar, Debug)]
struct Number {
    neg: Option<StringEq!("-")>,
    nat: Nat,
    fraction: Option<Fraction>,
    exponent: Option<Exponent>,
}

#[derive(Grammar, Debug)]
enum Nat {
    Pos(OneNine, Option<Digits>),
    Zero(StringEq!("0")),
}

#[derive(Grammar, Debug)]
struct Digits(Vec<Digit>);

#[derive(Grammar, Debug)]
struct Digit(CharOf<DigitClass>);

const_char_class!(DigitClass, "0-9", |c| c.is_ascii_digit());

#[derive(Grammar, Debug)]
struct OneNine(CharOf<OneNineClass>);

const_char_class!(OneNineClass, "1-9", |c| matches!(c, '1'..='9'));

const_char_class!(EClass, "e or E", |c| c == 'e' || c == 'E');

#[derive(Grammar, Debug)]
struct Fraction(Prefix<StringEq!("."), Digits>);

#[derive(Grammar, Debug)]
struct Exponent {
    e: CharOf<EClass>,
    sign: Option<Sign>,
    digits: Digits,
}

#[derive(Grammar, Debug)]
enum Sign {
    Pos(StringEq!("+")),
    Neg(StringEq!("-")),
}

#[derive(Grammar, Debug)]
struct Ws(PhantomData<StringOf<WsClass>>);

const_char_class!(WsClass, "JSON whitespace", |c| matches!(
    c,
    ' ' | '\n' | '\r' | '\t'
));

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("bnf") => {
            println!(
                "{}",
                bnf_rules![
                    Json, Element, Object, Members, Member, Array, Elements, String, Characters,
                    EscapedRun, Escape, Hex, Number, Nat, Digits, Digit, OneNine, Fraction,
                    Exponent, Sign, Ws,
                ]
            );
            ExitCode::SUCCESS
        }
        Some("parse") => common::parse::<Json>(),
        Some("test") => common::test::<Json>(),
        _ => common::usage("json_optimized"),
    }
}
