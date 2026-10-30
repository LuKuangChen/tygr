use tygr::*;

char_class!(IsDigit, "digit", |ch| ch.is_ascii_digit());

#[derive(Grammar, Debug)]
struct Csv(VecSep<StringOf1<IsDigit>, StringEq!(",")>);

#[test]
fn parses_items_and_separators() {
    let v = Csv::parse("1,22,333").unwrap();
    assert_eq!(
        v.0.items.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        ["1", "22", "333"]
    );
    assert_eq!(v.0.seps.len(), 2);
}
