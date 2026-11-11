//! Terminal grammars: literals, `char` and character classes.

use tygr::*;

/// `G`'s own rule, as printed.
fn rule<G: Parse>() -> String {
    G::bnf_rules()[0].to_string()
}

// "é" is 0xC3 0xA9 in UTF-8; "è" is 0xC3 0xA8.

#[derive(Grammar, Debug)]
struct Accent(StringEqCI!("é"));

#[cfg(feature = "trace_one_node")]
fn token(text: &'static str, range: std::ops::Range<usize>) -> Expectation {
    Expectation::Token { text, range }
}

#[test]
fn string_eq_ci_matches_unicode_case_variants() {
    assert_eq!(Accent::parse("é").unwrap().print(), "é");
    assert_eq!(Accent::parse("É").unwrap().print(), "É");
    assert!(Accent::parse("e").is_err());
}

#[derive(Grammar, Debug)]
struct Strasse(StringEqCI!("straße"));

#[test]
fn string_eq_ci_variant_can_change_length() {
    // 'ß' uppercases to "SS".
    assert_eq!(Strasse::parse("STRASSE").unwrap().print(), "STRASSE");
    assert!(Strasse::parse("Straße").is_ok());
}

#[derive(Grammar, Debug)]
struct Long(StringEq!("abcdefghijklmnop"));

#[test]
fn string_eq_longer_than_a_flat_tuple() {
    assert!(Long::parse("abcdefghijklmnop").is_ok());
    assert!(Long::parse("abcdefghijklmnoq").is_err());
}

#[test]
fn char_decodes_one_character_of_any_width() {
    for s in ["a", "é", "平", "😀"] {
        let (ch, end) = char::parse_prefix(s).unwrap();
        assert_eq!((ch.to_string().as_str(), end), (s, s.len()));
    }
    assert!(char::parse("").is_err());
}

#[cfg(feature = "trace_one_node")]
#[test]
fn literal_errors_report_the_whole_literal_where_it_diverged() {
    #[derive(Grammar, Debug)]
    struct Arrow(StringEq!("->"));
    let err = Arrow::parse("-x").unwrap_err();
    assert_eq!(err.pos(), 1);
    assert_eq!(err.traces()[0].expectation, token("->", 0..1));

    // A mismatch inside a multi-byte char backs out to the char's start.
    #[derive(Grammar, Debug)]
    struct E(StringEq!("aé"));
    let err = E::parse("aè").unwrap_err();
    assert_eq!(err.pos(), 1);
    assert_eq!(err.traces()[0].expectation, token("aé", 0..1));

    // The range is the input matched, even where a case variant is longer.
    let err = Strasse::parse("STRASSX").unwrap_err();
    assert_eq!(err.pos(), 6);
    assert_eq!(err.traces()[0].expectation, token("straße", 0..6));

    // The case variants of one char give one expectation, not one each.
    #[derive(Grammar, Debug)]
    struct Kw(StringEqCI!("if"));
    let err = Kw::parse("x").unwrap_err();
    assert_eq!(err.traces().len(), 1);
    assert_eq!(err.traces()[0].expectation, token("if", 0..0));
}

#[test]
fn ci_marker_only_where_case_matters() {
    #[derive(Grammar)]
    #[tygr(name = "Num")]
    struct Num(StringEqCI!("123"));
    #[derive(Grammar)]
    #[tygr(name = "Kw")]
    struct Kw(StringEqCI!("if"));
    assert_eq!(rule::<Num>(), "Num = \"123\" .");
    assert_eq!(rule::<Kw>(), "Kw = \"if\"i .");
}

#[test]
fn char_class_computes_first_bytes() {
    char_class!(Lower, |ch| ch.is_ascii_lowercase());
    let starts = |b| tygr::first::contains::<<Lower as Parse>::First>(Some(b));
    assert!(starts(b'a') && starts(b'z'));
    assert!(!starts(b'A') && !starts(b'0'));
    // UTF-8 lead bytes are kept (the predicate isn't evaluated beyond
    // ASCII); continuation bytes can't start a char.
    assert!(starts(0xC3) && !starts(0x80));
    assert!(!tygr::first::contains::<<Lower as Parse>::First>(None));

    #[derive(Grammar, Debug)]
    #[tygr(name = "Word")]
    struct Word(StringOf1<Lower>);
    assert_eq!(Word::parse("abc").unwrap().print(), "abc");
    assert!(Word::parse("aBc").is_err());
}

// Members of each width, each beside a non-member that differs only in its
// last byte: 'é' C3 A9 / 'è' C3 A8, '平' E5 B9 B3 / '年' E5 B9 B4,
// '😀' F0 9F 98 80 / '😁' F0 9F 98 81.
char_class!(Letter, |ch| matches!(ch, 'a'..='z' | 'é' | '平' | '😀'));

/// Each input, matched up to its last member.
const RUNS: [(&str, usize); 10] = [
    ("", 0),
    ("abc", 3),
    ("abcD", 3),
    ("Dabc", 0),
    ("aé平😀b", 11),
    ("aè", 1),
    ("é平年", 5),
    ("😀😁", 4),
    ("è", 0),
    ("ab1é", 2),
];

#[test]
fn string_of_runs_over_chars_of_any_width() {
    for (input, end) in RUNS {
        let text = &input[..end];
        let (run, run_end) = StringOf::<Letter>::parse_prefix(input).unwrap();
        assert_eq!((run.as_str(), run_end), (text, end), "{input:?}");
        assert_eq!(StringOf::<Letter>::check_prefix(input), Ok(end));
        // `StringOf1` is the same run, unless it's empty.
        let run1 = StringOf1::<Letter>::parse_prefix(input).map(|(run, end)| (run.0, end));
        assert_eq!(run1.ok(), (end > 0).then(|| (text.to_string(), end)));
        assert_eq!(StringOf1::<Letter>::check_prefix(input).is_ok(), end > 0);
    }
    assert_eq!(StringOf::<Letter>::default().as_str(), "");
}

#[test]
fn string_of_matches_and_fails_as_its_vec() {
    fn text<T>(result: Result<(Raw<T>, usize), Error>) -> Result<(String, usize), Error> {
        result.map(|(raw, end)| (raw.0, end))
    }
    for (input, _) in RUNS {
        assert_eq!(
            text(StringOf::<Letter>::parse_prefix(input)),
            text(Raw::<Vec<Letter>>::parse_prefix(input))
        );
        assert_eq!(
            StringOf::<Letter>::check(input),
            Raw::<Vec<Letter>>::check(input)
        );
        assert_eq!(
            text(StringOf1::<Letter>::parse_prefix(input)),
            text(Raw::<Vec1<Letter>>::parse_prefix(input))
        );
        assert_eq!(
            StringOf1::<Letter>::check(input),
            Raw::<Vec1<Letter>>::check(input)
        );
    }
    // And reads as it in BNF.
    #[derive(Grammar)]
    struct Words(StringOf<Letter>, StringEq!(" "), StringOf1<Letter>);
    assert_eq!(
        rule::<Words>(),
        "Words = { 'Letter' } \" \" 'Letter' { 'Letter' } ."
    );
}

#[test]
fn char_first_is_ascii_and_lead_bytes() {
    let starts = |b| tygr::first::contains::<<char as Parse>::First>(Some(b));
    assert!(starts(b'a') && starts(0x00) && starts(0x7F));
    assert!(starts(0xC2) && starts(0xE4) && starts(0xF4));
    assert!(!starts(0x80) && !starts(0xBF) && !starts(0xC1));
    assert!(!starts(0xF5) && !starts(0xFF));
}

#[test]
fn bnf_escapes_only_the_quote_in_use() {
    char_class!(Quote, "\" or '", |ch| matches!(ch, '"' | '\''));
    #[derive(Grammar)]
    struct Quoted(StringEq!("\"'"), Quote);
    assert_eq!(rule::<Quoted>(), r#"Quoted = "\"'" '" or \'' ."#);
}
