#![deny(rust_2018_idioms)]
use conch_parser::ast::builder::*;
use conch_parser::parse::ParseError::*;
use conch_parser::token::Token;

mod parse_support;
use crate::parse_support::*;

#[test]
fn test_if_command_valid_with_else() {
    let guard1 = cmd("guard1");
    let guard2 = cmd("guard2");
    let guard3 = cmd("guard3");

    let body1 = cmd("body1");
    let body2 = cmd("body2");

    let els = cmd("else");

    let correct = IfFragments {
        conditionals: vec![
            GuardBodyPairGroup {
                guard: CommandGroup {
                    commands: vec![guard1, guard2],
                    trailing_comments: vec![Newline(Some("#guard_comment_a".into()))],
                },
                body: CommandGroup {
                    commands: vec![body1],
                    trailing_comments: vec![Newline(Some("#body_comment_a".into()))],
                },
            },
            GuardBodyPairGroup {
                guard: CommandGroup {
                    commands: vec![guard3],
                    trailing_comments: vec![Newline(Some("#guard_comment_b".into()))],
                },
                body: CommandGroup {
                    commands: vec![body2],
                    trailing_comments: vec![Newline(Some("#body_comment_b".into()))],
                },
            },
        ],
        else_branch: Some(CommandGroup {
            commands: vec![els],
            trailing_comments: vec![Newline(Some("#else_comment".into()))],
        }),
    };
    let mut p = make_parser(
        "\
        if guard1; guard2;
        #guard_comment_a
        then body1
        #body_comment_a
        elif guard3;
        #guard_comment_b
        then body2;
        #body_comment_b
        else else;
        #else_comment
        fi
    ",
    );
    assert_eq!(correct, p.if_command().unwrap());
}

#[test]
fn test_if_command_valid_without_else() {
    let guard1 = cmd("guard1");
    let guard2 = cmd("guard2");
    let guard3 = cmd("guard3");

    let body1 = cmd("body1");
    let body2 = cmd("body2");

    let correct = IfFragments {
        conditionals: vec![
            GuardBodyPairGroup {
                guard: CommandGroup {
                    commands: vec![guard1, guard2],
                    trailing_comments: vec![Newline(Some("#guard_comment_a".into()))],
                },
                body: CommandGroup {
                    commands: vec![body1],
                    trailing_comments: vec![Newline(Some("#body_comment_a".into()))],
                },
            },
            GuardBodyPairGroup {
                guard: CommandGroup {
                    commands: vec![guard3],
                    trailing_comments: vec![Newline(Some("#guard_comment_b".into()))],
                },
                body: CommandGroup {
                    commands: vec![body2],
                    trailing_comments: vec![Newline(Some("#body_comment_b".into()))],
                },
            },
        ],
        else_branch: None,
    };
    let mut p = make_parser(
        "\
        if guard1; guard2;
        #guard_comment_a
        then body1
        #body_comment_a
        elif guard3;
        #guard_comment_b
        then body2;
        #body_comment_b
        fi
    ",
    );
    assert_eq!(correct, p.if_command().unwrap());
}

#[test]
fn test_if_command_invalid_missing_separator() {
    let mut p = make_parser("if guard; then body1; elif guard2; then body2; else else fi");
    assert_eq!(
        Err(IncompleteCmd("if", src(0, 1, 1), "fi", src(59, 1, 60))),
        p.if_command()
    );
}

#[test]
fn test_if_command_invalid_missing_keyword() {
    let mut p = make_parser("guard1; then body1; elif guard2; then body2; else else; fi");
    assert_eq!(
        Err(Unexpected(
            Token::Name(String::from("guard1")),
            src(0, 1, 1)
        )),
        p.if_command()
    );
    let mut p = make_parser("if guard1; then body1; elif guard2; then body2; else else;");
    assert_eq!(
        Err(IncompleteCmd("if", src(0, 1, 1), "fi", src(58, 1, 59))),
        p.if_command()
    );
}

#[test]
fn test_if_command_invalid_missing_guard() {
    let mut p = make_parser("if; then body1; elif guard2; then body2; else else; fi");
    assert_eq!(Err(Unexpected(Token::Semi, src(2, 1, 3))), p.if_command());
}

#[test]
fn test_if_command_invalid_missing_body() {
    let mut p = make_parser("if guard; then; elif guard2; then body2; else else; fi");
    assert_eq!(Err(Unexpected(Token::Semi, src(14, 1, 15))), p.if_command());
    let mut p = make_parser("if guard1; then body1; elif; then body2; else else; fi");
    assert_eq!(Err(Unexpected(Token::Semi, src(27, 1, 28))), p.if_command());
    let mut p = make_parser("if guard1; then body1; elif guard2; then body2; else; fi");
    assert_eq!(Err(Unexpected(Token::Semi, src(52, 1, 53))), p.if_command());
}

#[test]
fn test_if_command_invalid_quoted() {
    let cmds = [
        (
            "'if' guard1; then body1; elif guard2; then body2; else else; fi",
            Unexpected(Token::SingleQuote, src(0, 1, 1)),
        ),
        (
            "if guard1; then body1; elif guard2; then body2; else else; 'fi'",
            IncompleteCmd("if", src(0, 1, 1), "fi", src(63, 1, 64)),
        ),
        (
            "\"if\" guard1; then body1; elif guard2; then body2; else else; fi",
            Unexpected(Token::DoubleQuote, src(0, 1, 1)),
        ),
        (
            "if guard1; then body1; elif guard2; then body2; else else; \"fi\"",
            IncompleteCmd("if", src(0, 1, 1), "fi", src(63, 1, 64)),
        ),
    ];

    for (s, e) in &cmds {
        match make_parser(s).if_command() {
            Ok(result) => panic!("Unexpectedly parsed \"{}\" as\n{:#?}", s, result),
            Err(ref err) => {
                if err != e {
                    panic!(
                        "Expected the source \"{}\" to return the error `{:?}`, but got `{:?}`",
                        s, e, err
                    );
                }
            }
        }
    }
}

#[test]
fn test_if_command_invalid_concat() {
    let mut p = make_parser_from_tokens(vec![
        Token::Literal(String::from("i")),
        Token::Literal(String::from("f")),
        Token::Newline,
        Token::Literal(String::from("guard1")),
        Token::Newline,
        Token::Literal(String::from("then")),
        Token::Newline,
        Token::Literal(String::from("body1")),
        Token::Newline,
        Token::Literal(String::from("elif")),
        Token::Newline,
        Token::Literal(String::from("guard2")),
        Token::Newline,
        Token::Literal(String::from("then")),
        Token::Newline,
        Token::Literal(String::from("body2")),
        Token::Newline,
        Token::Literal(String::from("else")),
        Token::Newline,
        Token::Literal(String::from("else part")),
        Token::Newline,
        Token::Literal(String::from("fi")),
    ]);
    assert_eq!(
        Err(Unexpected(Token::Literal(String::from("i")), src(0, 1, 1))),
        p.if_command()
    );

    // Splitting up `then`, `elif`, and `else` tokens makes them
    // get interpreted as arguments, so an explicit error may not be raised
    // although the command will be malformed

    let mut p = make_parser_from_tokens(vec![
        Token::Literal(String::from("if")),
        Token::Newline,
        Token::Literal(String::from("guard1")),
        Token::Newline,
        Token::Literal(String::from("then")),
        Token::Newline,
        Token::Literal(String::from("body1")),
        Token::Newline,
        Token::Literal(String::from("elif")),
        Token::Newline,
        Token::Literal(String::from("guard2")),
        Token::Newline,
        Token::Literal(String::from("then")),
        Token::Newline,
        Token::Literal(String::from("body2")),
        Token::Newline,
        Token::Literal(String::from("else")),
        Token::Newline,
        Token::Literal(String::from("else part")),
        Token::Newline,
        Token::Literal(String::from("f")),
        Token::Literal(String::from("i")),
    ]);
    assert_eq!(
        Err(IncompleteCmd("if", src(0, 1, 1), "fi", src(61, 11, 3))),
        p.if_command()
    );
}

#[test]
fn test_if_command_should_recognize_literals_and_names() {
    for if_tok in vec![
        Token::Literal(String::from("if")),
        Token::Name(String::from("if")),
    ] {
        for then_tok in vec![
            Token::Literal(String::from("then")),
            Token::Name(String::from("then")),
        ] {
            for elif_tok in vec![
                Token::Literal(String::from("elif")),
                Token::Name(String::from("elif")),
            ] {
                for else_tok in vec![
                    Token::Literal(String::from("else")),
                    Token::Name(String::from("else")),
                ] {
                    for fi_tok in vec![
                        Token::Literal(String::from("fi")),
                        Token::Name(String::from("fi")),
                    ] {
                        let mut p = make_parser_from_tokens(vec![
                            if_tok.clone(),
                            Token::Whitespace(String::from(" ")),
                            Token::Literal(String::from("guard1")),
                            Token::Newline,
                            then_tok.clone(),
                            Token::Newline,
                            Token::Literal(String::from("body1")),
                            elif_tok.clone(),
                            Token::Whitespace(String::from(" ")),
                            Token::Literal(String::from("guard2")),
                            Token::Newline,
                            then_tok.clone(),
                            Token::Whitespace(String::from(" ")),
                            Token::Literal(String::from("body2")),
                            else_tok.clone(),
                            Token::Whitespace(String::from(" ")),
                            Token::Whitespace(String::from(" ")),
                            Token::Literal(String::from("else part")),
                            Token::Newline,
                            fi_tok,
                        ]);
                        p.if_command().unwrap();
                    }
                }
            }
        }
    }
}
