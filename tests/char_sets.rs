use tygr::*;

type Quote = StringEq!("\"\\");
type Control = CharRange<'\0', '\x1F'>;
type Plain = Except<AllChars, Union<Quote, Control>>;
type NonAscii = Except<AllChars, CharRange<'\0', '\x7F'>>;
type Greek = CharRange<'α', 'ω'>;

fn ascii<C: CharClass>() -> Vec<u8> {
    let t = C::ASCII.expect("compile-time table");
    (0..128u8).filter(|&b| t[b as usize]).collect()
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
fn ascii_tables_match_the_predicates() {
    assert_eq!(ascii::<AllChars>().len(), 128);
    assert!(ascii::<NoChars>().is_empty());
    assert_eq!(ascii::<Quote>(), b"\"\\");
    assert_eq!(ascii::<Control>(), (0..32).collect::<Vec<_>>());
    let plain: Vec<u8> = (32..128).filter(|&b| b != b'"' && b != b'\\').collect();
    assert_eq!(ascii::<Plain>(), plain);
    assert!(ascii::<NonAscii>().is_empty());
}

#[test]
fn non_ascii_is_settled_when_the_algebra_allows() {
    assert_eq!(AllChars::NON_ASCII, Some(true));
    assert_eq!(NoChars::NON_ASCII, Some(false));
    assert_eq!(Quote::NON_ASCII, Some(false));
    assert_eq!(<StringEq!("aé")>::NON_ASCII, None);
    assert_eq!(Control::NON_ASCII, Some(false));
    assert_eq!(CharRange::<'\u{80}', { char::MAX }>::NON_ASCII, Some(true));
    assert_eq!(Greek::NON_ASCII, None);
    assert_eq!(Plain::NON_ASCII, Some(true));
    assert_eq!(NonAscii::NON_ASCII, Some(true));
    assert_eq!(Union::<Quote, Greek>::NON_ASCII, None);
    assert_eq!(Union::<Greek, AllChars>::NON_ASCII, Some(true));
    assert_eq!(Except::<Greek, AllChars>::NON_ASCII, Some(false));
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
