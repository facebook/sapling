#![deny(rust_2018_idioms)]
#![recursion_limit = "128"]

use conch_parser::ast::builder::*;
use conch_parser::parse::*;

mod parse_support;
use crate::parse_support::*;

#[test]
fn test_parser_should_yield_none_after_error() {
    let mut iter = make_parser("foo && ||").into_iter();
    let _ = iter.next().expect("failed to get error").unwrap_err();
    assert_eq!(iter.next(), None);
}

#[test]
fn test_linebreak_valid_with_comments_and_whitespace() {
    let mut p = make_parser("\n\t\t\t\n # comment1\n#comment2\n   \n");
    assert_eq!(
        p.linebreak(),
        vec!(
            Newline(None),
            Newline(None),
            Newline(Some(String::from("# comment1"))),
            Newline(Some(String::from("#comment2"))),
            Newline(None)
        )
    );
}

#[test]
fn test_linebreak_valid_empty() {
    let mut p = make_parser("");
    assert_eq!(p.linebreak(), vec!());
}

#[test]
fn test_linebreak_valid_nonnewline() {
    let mut p = make_parser("hello world");
    assert_eq!(p.linebreak(), vec!());
}

#[test]
fn test_linebreak_valid_eof_instead_of_newline() {
    let mut p = make_parser("#comment");
    assert_eq!(p.linebreak(), vec!(Newline(Some(String::from("#comment")))));
}

#[test]
fn test_linebreak_single_quote_insiginificant() {
    let mut p = make_parser("#unclosed quote ' comment");
    assert_eq!(
        p.linebreak(),
        vec!(Newline(Some(String::from("#unclosed quote ' comment"))))
    );
}

#[test]
fn test_linebreak_double_quote_insiginificant() {
    let mut p = make_parser("#unclosed quote \" comment");
    assert_eq!(
        p.linebreak(),
        vec!(Newline(Some(String::from("#unclosed quote \" comment"))))
    );
}

#[test]
fn test_linebreak_escaping_newline_insignificant() {
    let mut p = make_parser("#comment escapes newline\\\n");
    assert_eq!(
        p.linebreak(),
        vec!(Newline(Some(String::from("#comment escapes newline\\"))))
    );
}

#[test]
fn test_skip_whitespace_preserve_newline() {
    let mut p = make_parser("    \t\t \t \t\n   ");
    p.skip_whitespace();
    assert_eq!(p.linebreak().len(), 1);
}

#[test]
fn test_skip_whitespace_preserve_comments() {
    let mut p = make_parser("    \t\t \t \t#comment\n   ");
    p.skip_whitespace();
    assert_eq!(
        p.linebreak().pop().unwrap(),
        Newline(Some(String::from("#comment")))
    );
}

#[test]
fn test_skip_whitespace_comments_capture_all_up_to_newline() {
    let mut p = make_parser("#comment&&||;;()<<-\n");
    assert_eq!(
        p.linebreak().pop().unwrap(),
        Newline(Some(String::from("#comment&&||;;()<<-")))
    );
}

#[test]
fn test_skip_whitespace_comments_may_end_with_eof() {
    let mut p = make_parser("#comment");
    assert_eq!(
        p.linebreak().pop().unwrap(),
        Newline(Some(String::from("#comment")))
    );
}

#[test]
fn test_skip_whitespace_skip_escapes_dont_affect_newlines() {
    let mut p = make_parser("  \t \\\n  \\\n#comment\n");
    p.skip_whitespace();
    assert_eq!(
        p.linebreak().pop().unwrap(),
        Newline(Some(String::from("#comment")))
    );
}

#[test]
fn test_skip_whitespace_skips_escaped_newlines() {
    let mut p = make_parser("\\\n\\\n   \\\n");
    p.skip_whitespace();
    assert_eq!(p.linebreak(), vec!());
}

#[test]
fn test_comment_cannot_start_mid_whitespace_delimited_word() {
    let mut p = make_parser("hello#world");
    let w = p.word().unwrap().expect("no valid word was discovered");
    assert_eq!(w, word("hello#world"));
}

#[test]
fn test_comment_can_start_if_whitespace_before_pound() {
    let mut p = make_parser("hello #world");
    p.word().unwrap().expect("no valid word was discovered");
    let comment = p.linebreak();
    assert_eq!(comment, vec!(Newline(Some(String::from("#world")))));
}

#[test]
fn test_braces_literal_unless_brace_group_expected() {
    let source = "echo {} } {";
    let mut p = make_parser(source);
    assert_eq!(p.word().unwrap().unwrap(), word("echo"));
    assert_eq!(p.word().unwrap().unwrap(), word("{}"));
    assert_eq!(p.word().unwrap().unwrap(), word("}"));
    assert_eq!(p.word().unwrap().unwrap(), word("{"));

    let correct = Some(cmd_args("echo", &["{}", "}", "{"]));
    assert_eq!(correct, make_parser(source).complete_command().unwrap());
}

#[test]
fn ensure_parse_errors_are_send_and_sync() {
    fn send_and_sync<T: Send + Sync>() {}
    send_and_sync::<ParseError<()>>();
}

#[test]
fn ensure_parser_could_be_send_and_sync() {
    use conch_parser::token::Token;

    fn send_and_sync<T: Send + Sync>() {}
    send_and_sync::<Parser<std::vec::IntoIter<Token>, ArcBuilder>>();
}
