//! Throughput benchmark for the optimized JSON grammar.
//!
//!   cargo bench --bench json_optimized
//!
//! The grammar is copied from `examples/json_optimized.rs` (minus its CLI).

use std::hint::black_box;
use std::marker::PhantomData;
use std::time::Instant;

use tygr::*;

#[derive(Grammar)]
struct Json(Wrap<Ws, Element, Ws>);

#[derive(Grammar)]
enum Element {
    Object(Object),
    Array(Array),
    String(Range<PhantomData<String>>),
    Number(Range<PhantomData<Number>>),
    True(StringEq!("true")),
    False(StringEq!("false")),
    Null(StringEq!("null")),
}

#[derive(Grammar)]
struct Object(Wrap<(StringEq!("{"), Ws), Option<Members>, StringEq!("}")>);

#[derive(Grammar)]
struct Members(VecSep<Suffix<Member, Ws>, Suffix<StringEq!(","), Ws>>);

#[derive(Grammar)]
struct Member {
    pub string: String,
    col: Wrap<Ws, StringEq!(":"), Ws>,
    pub value: Element,
}

#[derive(Grammar)]
struct Array(Wrap<(StringEq!("["), Ws), Option<Elements>, StringEq!("]")>);

#[derive(Grammar)]
struct Elements(VecSep<Suffix<Element, Ws>, Suffix<StringEq!(","), Ws>>);

#[derive(Grammar)]
struct String(Wrap<StringEq!("\""), Characters, StringEq!("\"")>);

#[derive(Grammar)]
struct Characters(Vec<Character>);

#[derive(Grammar)]
enum Character {
    Normal(CharOf<NormalCharacter>),
    Escape(Prefix<StringEq!("\\"), Escape>),
}

char_class!(NormalCharacter, "0x20-0x10FFFF except for \" and \\", |c| {
    (0x20..=0x10FFFF).contains(&(c as u32)) && (c != '"') && c != '\\'
});

#[derive(Grammar)]
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
    },
}

#[derive(Grammar)]
enum Hex {
    Digit(Digit),
    UAToF(CharOf<UAToF>),
    LAToF(CharOf<LAToF>),
}

char_class!(UAToF, "A-F", |c| ('A'..='F').contains(&c));
char_class!(LAToF, "a-f", |c| ('a'..='f').contains(&c));

#[derive(Grammar)]
struct Number {
    neg: Option<StringEq!("-")>,
    nat: Nat,
    fraction: Option<Fraction>,
    exponent: Option<Exponent>,
}

#[derive(Grammar)]
enum Nat {
    Pos(OneNine, Option<Digits>),
    Zero(StringEq!("0")),
}

#[derive(Grammar)]
struct Digits(Vec<Digit>);

#[derive(Grammar)]
struct Digit(CharOf<DigitClass>);

char_class!(DigitClass, "0-9", |c| c.is_ascii_digit());

#[derive(Grammar)]
struct OneNine(CharOf<OneNineClass>);

char_class!(OneNineClass, "1-9", |c| ('1'..='9').contains(&c));

char_class!(EClass, "e or E", |c| c == 'e' || c == 'E');

#[derive(Grammar)]
struct Fraction(Prefix<StringEq!("."), Digits>);

#[derive(Grammar)]
struct Exponent {
    e: CharOf<EClass>,
    sign: Option<Sign>,
    digits: Digits,
}

#[derive(Grammar)]
enum Sign {
    Pos(StringEq!("+")),
    Neg(StringEq!("-")),
}

#[derive(Grammar)]
struct Ws(PhantomData<StringOf<WsClass>>);

char_class!(WsClass, "JSON whitespace", |c| matches!(
    c,
    ' ' | '\n' | '\r' | '\t'
));

const CANADA: &str = include_str!("../data/canada.json");
const DATA: &str = include_str!("../data/data.json");

fn bench(label: &str, input: &str) {
    let mb = input.len() as f64 / 1e6;
    let mut best = f64::MAX;
    for _ in 0..30 {
        let start = Instant::now();
        let v = Json::parse(input).unwrap();
        best = best.min(start.elapsed().as_secs_f64());
        black_box(&v);
    }
    println!("{label}: {:.3}ms  {:.1} MB/s", best * 1e3, mb / best);
}

fn main() {
    bench("CANADA", CANADA);
    bench("DATA", DATA);
}
