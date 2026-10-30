use tygr::*;

type Quote = StringEq!("\"\\");
type Control = CharRange<'\0', '\x1F'>;
type Plain = Except<AllChars, Union<Quote, Control>>;
type NonAscii = Except<AllChars, CharRange<'\0', '\x7F'>>;
type Greek = CharRange<'α', 'ω'>;

fn with<C: CharClass>(answer: Lead) -> Vec<u8> {
    (0..=255u8)
        .filter(|&b| C::LEAD_BYTES[b as usize] == answer)
        .collect()
}

fn leads() -> Vec<u8> {
    (0..=255u8).filter(|&b| lead::span(b).is_some()).collect()
}

#[test]
fn matches_follow_the_set_algebra() {
    for ch in ['a', ' ', '~', '\x7F', 'é', '日', '😀'] {
        assert!(Plain::matches(ch), "{ch:?}");
    }
    for ch in ['"', '\\', '\0', '\n', '\x1F'] {
        assert!(!Plain::matches(ch), "{ch:?}");
    }
    assert!(Union::<NoChars, Greek>::matches('β'));
    assert!(!Except::<Greek, StringEq!("β")>::matches('β'));
    assert!(Except::<Greek, StringEq!("β")>::matches('γ'));
}

#[test]
fn byte_tables_follow_the_set_algebra() {
    use Lead::*;
    assert_eq!(with::<AllChars>(Accept), leads());
    assert!(with::<NoChars>(Accept).is_empty() && with::<NoChars>(Check).is_empty());
    assert_eq!(with::<Quote>(Accept), b"\"\\");
    assert!(with::<Quote>(Check).is_empty());
    assert_eq!(with::<StringEq!("aé")>(Accept), b"a");
    assert_eq!(with::<StringEq!("aé")>(Check), [0xC3]);
    assert_eq!(with::<Control>(Accept), (0..32).collect::<Vec<_>>());
    // Every char from U+0080 starts with a byte that only starts allowed chars.
    let plain: Vec<u8> = leads()
        .into_iter()
        .filter(|&b| b >= 32 && b != b'"' && b != b'\\')
        .collect();
    assert_eq!(with::<Plain>(Accept), plain);
    assert!(with::<Plain>(Check).is_empty());
    assert_eq!(
        with::<NonAscii>(Accept),
        leads()
            .into_iter()
            .filter(|&b| b >= 0x80)
            .collect::<Vec<_>>()
    );
    // α..ω is U+03B1..U+03C9: lead 0xCE (U+0380..U+03BF) partly, 0xCF (U+03C0..U+03FF) partly.
    assert_eq!(with::<Greek>(Check), [0xCE, 0xCF]);
    assert!(with::<Greek>(Accept).is_empty());
    assert_eq!(with::<Except<Greek, AllChars>>(Check), b"");
    assert_eq!(with::<Union<Greek, AllChars>>(Accept), leads());
}

const_char_class!(
    Latin1Letter,
    "Latin-1 letter",
    |c| matches!(c, 'a'..='z' | 'A'..='Z' | 'À'..='ÿ')
);

#[test]
fn const_predicates_are_settled_byte_by_byte() {
    use Lead::*;
    // U+00C0..U+00FF all start with 0xC3, so 0xC3 is settled without decoding.
    let mut accept: Vec<u8> = (b'A'..=b'Z').chain(b'a'..=b'z').collect();
    accept.push(0xC3);
    accept.sort();
    assert_eq!(with::<Latin1Letter>(Accept), accept);
    assert!(with::<Latin1Letter>(Check).iter().all(|&b| b >= 0xE0));
}

const_char_class!(PlainChar, "plain char", Plain);

#[test]
fn runs_use_the_classes() {
    assert_eq!(
        StringOf::<PlainChar>::scan_prefix("ab日本😀\"x"),
        Ok("ab日本😀".len())
    );
    assert_eq!(StringOf::<Greek>::scan_prefix("αβγ!"), Ok("αβγ".len()));
    assert_eq!(StringOf::<NonAscii>::scan_prefix("日本a"), Ok("日本".len()));
    assert!(StringOf1::<NoChars>::scan_prefix("a").is_err());
}
