#![deny(rust_2018_idioms)]
use conch_parser::ast::builder::*;
use conch_parser::parse::ParseError::*;
use conch_parser::token::Token;

mod parse_support;
use crate::parse_support::*;

#[test]
fn test_brace_group_valid() {
    let mut p = make_parser("{ foo\nbar; baz\n#comment1\n#comment2\n }");
    let correct = CommandGroup {
        commands: vec![cmd("foo"), cmd("bar"), cmd("baz")],
        trailing_comments: vec![
            Newline(Some("#comment1".into())),
            Newline(Some("#comment2".into())),
        ],
    };
    assert_eq!(correct, p.brace_group().unwrap());
}

#[test]
fn test_brace_group_invalid_missing_separator() {
    assert_eq!(
        Err(Unmatched(Token::CurlyOpen, src(0, 1, 1))),
        make_parser("{ foo\nbar; baz }").brace_group()
    );
}

#[test]
fn test_brace_group_invalid_start_must_be_whitespace_delimited() {
    let mut p = make_parser("{foo\nbar; baz; }");
    assert_eq!(
        Err(Unexpected(Token::Name(String::from("foo")), src(1, 1, 2))),
        p.brace_group()
    );
}

#[test]
fn test_brace_group_valid_end_must_be_whitespace_and_separator_delimited() {
    let mut p = make_parser("{ foo\nbar}; baz; }");
    p.brace_group().unwrap();
    assert_eq!(p.complete_command().unwrap(), None); // Ensure stream is empty
    let mut p = make_parser("{ foo\nbar; }baz; }");
    p.brace_group().unwrap();
    assert_eq!(p.complete_command().unwrap(), None); // Ensure stream is empty
}

#[test]
fn test_brace_group_valid_keyword_delimited_by_separator() {
    let mut p = make_parser("{ foo }; }");
    let correct = CommandGroup {
        commands: vec![cmd_args("foo", &["}"])],
        trailing_comments: vec![],
    };
    assert_eq!(correct, p.brace_group().unwrap());
}

#[test]
fn test_brace_group_invalid_missing_keyword() {
    let mut p = make_parser("{ foo\nbar; baz");
    assert_eq!(
        Err(Unmatched(Token::CurlyOpen, src(0, 1, 1))),
        p.brace_group()
    );
    let mut p = make_parser("foo\nbar; baz; }");
    assert_eq!(
        Err(Unexpected(Token::Name(String::from("foo")), src(0, 1, 1))),
        p.brace_group()
    );
}

#[test]
fn test_brace_group_invalid_quoted() {
    let cmds = [
        (
            "'{' foo\nbar; baz; }",
            Unexpected(Token::SingleQuote, src(0, 1, 1)),
        ),
        (
            "{ foo\nbar; baz; '}'",
            Unmatched(Token::CurlyOpen, src(0, 1, 1)),
        ),
        (
            "\"{\" foo\nbar; baz; }",
            Unexpected(Token::DoubleQuote, src(0, 1, 1)),
        ),
        (
            "{ foo\nbar; baz; \"}\"",
            Unmatched(Token::CurlyOpen, src(0, 1, 1)),
        ),
    ];

    for (c, e) in &cmds {
        match make_parser(c).brace_group() {
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
fn test_brace_group_invalid_missing_body() {
    assert_eq!(
        Err(Unexpected(Token::CurlyClose, src(2, 2, 1))),
        make_parser("{\n}").brace_group()
    );
}
