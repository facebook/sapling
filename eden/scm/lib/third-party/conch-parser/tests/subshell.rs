#![deny(rust_2018_idioms)]
use conch_parser::ast::builder::*;
use conch_parser::parse::ParseError::*;
use conch_parser::token::Token;

mod parse_support;
use crate::parse_support::*;

#[test]
fn test_subshell_valid() {
    let mut p = make_parser("( foo\nbar; baz\n#comment\n )");
    let correct = CommandGroup {
        commands: vec![cmd("foo"), cmd("bar"), cmd("baz")],
        trailing_comments: vec![Newline(Some("#comment".into()))],
    };
    assert_eq!(correct, p.subshell().unwrap());
}

#[test]
fn test_subshell_valid_separator_not_needed() {
    let correct = CommandGroup {
        commands: vec![cmd("foo")],
        trailing_comments: vec![],
    };
    assert_eq!(correct, make_parser("( foo )").subshell().unwrap());

    let correct_with_comment = CommandGroup {
        commands: vec![cmd("foo")],
        trailing_comments: vec![Newline(Some("#comment".into()))],
    };
    assert_eq!(
        correct_with_comment,
        make_parser("( foo\n#comment\n )").subshell().unwrap()
    );
}

#[test]
fn test_subshell_space_between_parens_not_needed() {
    let mut p = make_parser("(foo )");
    p.subshell().unwrap();
    let mut p = make_parser("( foo)");
    p.subshell().unwrap();
    let mut p = make_parser("(foo)");
    p.subshell().unwrap();
}

#[test]
fn test_subshell_invalid_missing_keyword() {
    assert_eq!(
        Err(Unmatched(Token::ParenOpen, src(0, 1, 1))),
        make_parser("( foo\nbar; baz").subshell()
    );
    assert_eq!(
        Err(Unexpected(Token::Name(String::from("foo")), src(0, 1, 1))),
        make_parser("foo\nbar; baz; )").subshell()
    );
}

#[test]
fn test_subshell_invalid_quoted() {
    let cmds = [
        (
            "'(' foo\nbar; baz; )",
            Unexpected(Token::SingleQuote, src(0, 1, 1)),
        ),
        (
            "( foo\nbar; baz; ')'",
            Unmatched(Token::ParenOpen, src(0, 1, 1)),
        ),
        (
            "\"(\" foo\nbar; baz; )",
            Unexpected(Token::DoubleQuote, src(0, 1, 1)),
        ),
        (
            "( foo\nbar; baz; \")\"",
            Unmatched(Token::ParenOpen, src(0, 1, 1)),
        ),
    ];

    for (c, e) in &cmds {
        match make_parser(c).subshell() {
            Ok(result) => panic!("Unexpectedly parsed \"{}\" as\n{:#?}", c, result),
            Err(ref err) => {
                if err != e {
                    panic!(
                        "Expected the source \"{}\" to return the error `{:?}`, but got `{:?}`",
                        c, e, err
                    );
                }
            }
        }
    }
}

#[test]
fn test_subshell_invalid_missing_body() {
    assert_eq!(
        Err(Unexpected(Token::ParenClose, src(2, 2, 1))),
        make_parser("(\n)").subshell()
    );
    assert_eq!(
        Err(Unexpected(Token::ParenClose, src(1, 1, 2))),
        make_parser("()").subshell()
    );
}
