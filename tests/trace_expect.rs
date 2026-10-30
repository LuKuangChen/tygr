//! `trace_expect` alone: expectations are recorded, rule context is not.
#![cfg(all(feature = "trace_expect", not(feature = "trace_one_node")))]

use tygr::*;

#[derive(Grammar, Debug)]
struct Lit(StringEq!("foo"));

#[derive(Grammar, Debug)]
struct Wrapper(Lit);

#[test]
fn records_the_expectation_without_context() {
    let err = Wrapper::parse("bar").unwrap_err();
    assert_eq!(
        err,
        Error {
            traces: vec![Trace {
                context: vec![],
                expectation: Expectation::StringEq("foo".to_string(), "foo".to_string()),
            }],
            pos: 0,
        }
    );
}

#[test]
fn display_lists_the_expectation() {
    let err = Wrapper::parse("bar").unwrap_err();
    assert_eq!(
        err.to_string(),
        "parse error at byte 0, expected:\n\t- \"foo\"\n"
    );
}
