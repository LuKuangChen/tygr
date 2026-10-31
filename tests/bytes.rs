use tygr::*;

char_class!(NotQuote, "not a quote", |c| c != '"');
const_char_class!(Plain, "plain", Except<AllChars, StringEq!("\"")>);

#[derive(Grammar, Debug)]
struct Quoted(Wrap<StringEq!("\""), StringOf<Plain>, StringEq!("\"")>);

#[derive(Debug, GrammarFromStr)]
struct Parsed(u32);
impl GrammarFrom for Parsed {
    type Source = StringOf1<Plain>;
    fn print_to(&self, buf: &mut String) {
        buf.push_str(&self.0.to_string());
    }
}
impl std::str::FromStr for Parsed {
    type Err = std::num::ParseIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(Parsed)
    }
}

#[test]
fn text_is_checked_when_read_not_when_parsed() {
    let ok = Quoted::parse_bytes("\"héllo\"".as_bytes()).unwrap();
    assert_eq!(ok.0.wrapped.as_str(), Ok("héllo"));

    // 0xC3 starts a two-byte char, but '(' is not a continuation byte.
    let bad = Quoted::parse_bytes(b"\"h\xC3(llo\"").unwrap();
    assert_eq!(bad.0.wrapped.as_bytes(), b"h\xC3(llo");
    assert!(bad.0.wrapped.as_str().is_err());
}

#[test]
fn conversions_from_text_reject_invalid_utf8() {
    assert_eq!(Parsed::parse_bytes(b"42").unwrap().0, 42);
    assert!(Parsed::parse_bytes(b"4\xFF2").is_err());
}

#[test]
fn predicate_classes_do_not_match_invalid_utf8() {
    assert_eq!(StringOf::<NotQuote>::scan_prefix_bytes(b"ab\xFFcd"), Ok(2));
    assert_eq!(
        StringOf::<NotQuote>::scan_prefix_bytes("abé".as_bytes()),
        Ok(4)
    );
}
