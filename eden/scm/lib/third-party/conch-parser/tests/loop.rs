#![deny(rust_2018_idioms)]
use conch_parser::ast::builder::*;
use conch_parser::parse::ParseError::*;
use conch_parser::token::Token;

mod parse_support;
use crate::parse_support::*;

#[test]
fn test_loop_command_while_valid() {
    let mut p = make_parser(
        "while guard1; guard2;\n#guard_comment\n do foo\nbar; baz\n#body_comment\n done",
    );
    let (until, GuardBodyPairGroup { guard, body }) = p.loop_command().unwrap();

    let correct_guard = CommandGroup {
        commands: vec![cmd("guard1"), cmd("guard2")],
        trailing_comments: vec![Newline(Some("#guard_comment".into()))],
    };
    let correct_body = CommandGroup {
        commands: vec![cmd("foo"), cmd("bar"), cmd("baz")],
        trailing_comments: vec![Newline(Some("#body_comment".into()))],
    };

    assert_eq!(until, LoopKind::While);
    assert_eq!(correct_guard, guard);
    assert_eq!(correct_body, body);
}

#[test]
fn test_loop_command_until_valid() {
    let mut p = make_parser(
        "until guard1; guard2;\n#guard_comment\n do foo\nbar; baz\n#body_comment\n done",
    );
    let (until, GuardBodyPairGroup { guard, body }) = p.loop_command().unwrap();

    let correct_guard = CommandGroup {
        commands: vec![cmd("guard1"), cmd("guard2")],
        trailing_comments: vec![Newline(Some("#guard_comment".into()))],
    };
    let correct_body = CommandGroup {
        commands: vec![cmd("foo"), cmd("bar"), cmd("baz")],
        trailing_comments: vec![Newline(Some("#body_comment".into()))],
    };

    assert_eq!(until, LoopKind::Until);
    assert_eq!(correct_guard, guard);
    assert_eq!(correct_body, body);
}

#[test]
fn test_loop_command_invalid_missing_separator() {
    let mut p = make_parser("while guard do foo\nbar; baz; done");
    assert_eq!(
        Err(IncompleteCmd("while", src(0, 1, 1), "do", src(33, 2, 15))),
        p.loop_command()
    );
    let mut p = make_parser("while guard; do foo\nbar; baz done");
    assert_eq!(
        Err(IncompleteCmd("do", src(13, 1, 14), "done", src(33, 2, 14))),
        p.loop_command()
    );
}

#[test]
fn test_loop_command_invalid_missing_keyword() {
    let mut p = make_parser("guard; do foo\nbar; baz; done");
    assert_eq!(
        Err(Unexpected(Token::Name(String::from("guard")), src(0, 1, 1))),
        p.loop_command()
    );
}

#[test]
fn test_loop_command_invalid_missing_guard() {
    // With command separator between loop and do keywords
    let mut p = make_parser("while; do foo\nbar; baz; done");
    assert_eq!(Err(Unexpected(Token::Semi, src(5, 1, 6))), p.loop_command());
    let mut p = make_parser("until; do foo\nbar; baz; done");
    assert_eq!(Err(Unexpected(Token::Semi, src(5, 1, 6))), p.loop_command());

    // Without command separator between loop and do keywords
    let mut p = make_parser("while do foo\nbar; baz; done");
    assert_eq!(
        Err(Unexpected(Token::Name(String::from("do")), src(6, 1, 7))),
        p.loop_command()
    );
    let mut p = make_parser("until do foo\nbar; baz; done");
    assert_eq!(
        Err(Unexpected(Token::Name(String::from("do")), src(6, 1, 7))),
        p.loop_command()
    );
}

#[test]
fn test_loop_command_invalid_quoted() {
    let cmds = [
        (
            "'while' guard do foo\nbar; baz; done",
            Unexpected(Token::SingleQuote, src(0, 1, 1)),
        ),
        (
            "'until' guard do foo\nbar; baz; done",
            Unexpected(Token::SingleQuote, src(0, 1, 1)),
        ),
        (
            "\"while\" guard do foo\nbar; baz; done",
            Unexpected(Token::DoubleQuote, src(0, 1, 1)),
        ),
        (
            "\"until\" guard do foo\nbar; baz; done",
            Unexpected(Token::DoubleQuote, src(0, 1, 1)),
        ),
    ];

    for (c, e) in &cmds {
        match make_parser(c).loop_command() {
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
fn test_loop_command_invalid_concat() {
    let mut p = make_parser_from_tokens(vec![
        Token::Literal(String::from("whi")),
        Token::Literal(String::from("le")),
        Token::Newline,
        Token::Literal(String::from("guard")),
        Token::Newline,
        Token::Literal(String::from("do")),
        Token::Literal(String::from("foo")),
        Token::Newline,
        Token::Literal(String::from("done")),
    ]);
    assert_eq!(
        Err(Unexpected(
            Token::Literal(String::from("whi")),
            src(0, 1, 1)
        )),
        p.loop_command()
    );
    let mut p = make_parser_from_tokens(vec![
        Token::Literal(String::from("un")),
        Token::Literal(String::from("til")),
        Token::Newline,
        Token::Literal(String::from("guard")),
        Token::Newline,
        Token::Literal(String::from("do")),
        Token::Literal(String::from("foo")),
        Token::Newline,
        Token::Literal(String::from("done")),
    ]);
    assert_eq!(
        Err(Unexpected(Token::Literal(String::from("un")), src(0, 1, 1))),
        p.loop_command()
    );
}

#[test]
fn test_loop_command_should_recognize_literals_and_names() {
    for kw in vec![
        Token::Literal(String::from("while")),
        Token::Name(String::from("while")),
        Token::Literal(String::from("until")),
        Token::Name(String::from("until")),
    ] {
        let mut p = make_parser_from_tokens(vec![
            kw,
            Token::Newline,
            Token::Literal(String::from("guard")),
            Token::Newline,
            Token::Literal(String::from("do")),
            Token::Newline,
            Token::Literal(String::from("foo")),
            Token::Newline,
            Token::Literal(String::from("done")),
        ]);
        p.loop_command().unwrap();
    }
}
