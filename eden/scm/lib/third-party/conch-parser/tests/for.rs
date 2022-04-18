#![deny(rust_2018_idioms)]
use conch_parser::ast::builder::*;
use conch_parser::parse::ParseError::*;
use conch_parser::token::Token;

mod parse_support;
use crate::parse_support::*;

#[test]
fn test_for_command_valid_with_words() {
    let mut p = make_parser(
        "\
    for var #var comment
    #prew1
    #prew2
    in one two three #word comment
    #precmd1
    #precmd2
    do echo;
    #body_comment
    done
    ",
    );
    assert_eq!(
        p.for_command(),
        Ok(ForFragments {
            var: "var".into(),
            var_comment: Some(Newline(Some("#var comment".into()))),
            words: Some((
                vec!(
                    Newline(Some("#prew1".into())),
                    Newline(Some("#prew2".into())),
                ),
                vec!(word("one"), word("two"), word("three"),),
                Some(Newline(Some("#word comment".into())))
            )),
            pre_body_comments: vec!(
                Newline(Some("#precmd1".into())),
                Newline(Some("#precmd2".into())),
            ),
            body: CommandGroup {
                commands: vec!(cmd("echo")),
                trailing_comments: vec!(Newline(Some("#body_comment".into()))),
            },
        })
    );
}

#[test]
fn test_for_command_valid_without_words() {
    let mut p = make_parser(
        "\
    for var #var comment
    #w1
    #w2
    do echo;
    #body_comment
    done
    ",
    );
    assert_eq!(
        p.for_command(),
        Ok(ForFragments {
            var: "var".into(),
            var_comment: Some(Newline(Some("#var comment".into()))),
            words: None,
            pre_body_comments: vec!(Newline(Some("#w1".into())), Newline(Some("#w2".into())),),
            body: CommandGroup {
                commands: vec!(cmd("echo")),
                trailing_comments: vec!(Newline(Some("#body_comment".into()))),
            },
        })
    );
}

#[test]
fn test_for_command_valid_separators() {
    let cases = vec![
        "for var                 do body; done",
        "for var             ;   do body; done",
        "for var             ;\n do body; done",
        "for var\n               do body; done",
        "for var\n in        ;   do body; done",
        "for var\n in        ;\n do body; done",
        "for var\n in         \n do body; done",
        "for var   in        ;   do body; done",
        "for var   in        ;\n do body; done",
        "for var   in         \n do body; done",
        "for var\n in one two;   do body; done",
        "for var\n in one two;\n do body; done",
        "for var\n in one two \n do body; done",
        "for var   in one two;   do body; done",
        "for var   in one two;\n do body; done",
        "for var   in one two \n do body; done",
    ];

    for src in cases {
        match make_parser(src).for_command() {
            Ok(_) => {}
            e @ Err(_) => panic!("expected `{}` to parse successfully, but got: {:?}", src, e),
        }
    }
}

#[test]
fn test_for_command_valid_with_separator() {
    let mut p = make_parser("for var in one two three\ndo echo $var; done");
    p.for_command().unwrap();
    let mut p = make_parser("for var in one two three;do echo $var; done");
    p.for_command().unwrap();
}

#[test]
fn test_for_command_invalid_with_in_no_words_no_with_separator() {
    let mut p = make_parser("for var in do echo $var; done");
    assert_eq!(
        Err(IncompleteCmd("for", src(0, 1, 1), "do", src(25, 1, 26))),
        p.for_command()
    );
}

#[test]
fn test_for_command_invalid_missing_separator() {
    let mut p = make_parser("for var in one two three do echo $var; done");
    assert_eq!(
        Err(IncompleteCmd("for", src(0, 1, 1), "do", src(39, 1, 40))),
        p.for_command()
    );
}

#[test]
fn test_for_command_invalid_amp_not_valid_separator() {
    let mut p = make_parser("for var in one two three& do echo $var; done");
    assert_eq!(Err(Unexpected(Token::Amp, src(24, 1, 25))), p.for_command());
}

#[test]
fn test_for_command_invalid_missing_keyword() {
    let mut p = make_parser("var in one two three\ndo echo $var; done");
    assert_eq!(
        Err(Unexpected(Token::Name(String::from("var")), src(0, 1, 1))),
        p.for_command()
    );
}

#[test]
fn test_for_command_invalid_missing_var() {
    let mut p = make_parser("for in one two three\ndo echo $var; done");
    assert_eq!(
        Err(IncompleteCmd("for", src(0, 1, 1), "in", src(7, 1, 8))),
        p.for_command()
    );
}

#[test]
fn test_for_command_invalid_missing_body() {
    let mut p = make_parser("for var in one two three\n");
    assert_eq!(
        Err(IncompleteCmd("for", src(0, 1, 1), "do", src(25, 2, 1))),
        p.for_command()
    );
}

#[test]
fn test_for_command_invalid_quoted() {
    let cmds = [
        (
            "'for' var in one two three\ndo echo $var; done",
            Unexpected(Token::SingleQuote, src(0, 1, 1)),
        ),
        (
            "for var 'in' one two three\ndo echo $var; done",
            IncompleteCmd("for", src(0, 1, 1), "in", src(8, 1, 9)),
        ),
        (
            "\"for\" var in one two three\ndo echo $var; done",
            Unexpected(Token::DoubleQuote, src(0, 1, 1)),
        ),
        (
            "for var \"in\" one two three\ndo echo $var; done",
            IncompleteCmd("for", src(0, 1, 1), "in", src(8, 1, 9)),
        ),
    ];

    for (c, e) in &cmds {
        match make_parser(c).for_command() {
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
fn test_for_command_invalid_var_must_be_name() {
    let mut p = make_parser("for 123var in one two three\ndo echo $var; done");
    assert_eq!(
        Err(BadIdent(String::from("123var"), src(4, 1, 5))),
        p.for_command()
    );
    let mut p = make_parser("for 'var' in one two three\ndo echo $var; done");
    assert_eq!(
        Err(Unexpected(Token::SingleQuote, src(4, 1, 5))),
        p.for_command()
    );
    let mut p = make_parser("for \"var\" in one two three\ndo echo $var; done");
    assert_eq!(
        Err(Unexpected(Token::DoubleQuote, src(4, 1, 5))),
        p.for_command()
    );
    let mut p = make_parser("for var*% in one two three\ndo echo $var; done");
    assert_eq!(
        Err(IncompleteCmd("for", src(0, 1, 1), "in", src(7, 1, 8))),
        p.for_command()
    );
}

#[test]
fn test_for_command_invalid_concat() {
    let mut p = make_parser_from_tokens(vec![
        Token::Literal(String::from("fo")),
        Token::Literal(String::from("r")),
        Token::Whitespace(String::from(" ")),
        Token::Name(String::from("var")),
        Token::Whitespace(String::from(" ")),
        Token::Literal(String::from("in")),
        Token::Literal(String::from("one")),
        Token::Whitespace(String::from(" ")),
        Token::Literal(String::from("two")),
        Token::Whitespace(String::from(" ")),
        Token::Literal(String::from("three")),
        Token::Whitespace(String::from(" ")),
        Token::Newline,
        Token::Literal(String::from("do")),
        Token::Whitespace(String::from(" ")),
        Token::Literal(String::from("echo")),
        Token::Whitespace(String::from(" ")),
        Token::Dollar,
        Token::Literal(String::from("var")),
        Token::Newline,
        Token::Literal(String::from("done")),
    ]);
    assert_eq!(
        Err(Unexpected(Token::Literal(String::from("fo")), src(0, 1, 1))),
        p.for_command()
    );

    let mut p = make_parser_from_tokens(vec![
        Token::Literal(String::from("for")),
        Token::Whitespace(String::from(" ")),
        Token::Name(String::from("var")),
        Token::Whitespace(String::from(" ")),
        Token::Literal(String::from("i")),
        Token::Literal(String::from("n")),
        Token::Literal(String::from("one")),
        Token::Whitespace(String::from(" ")),
        Token::Literal(String::from("two")),
        Token::Whitespace(String::from(" ")),
        Token::Literal(String::from("three")),
        Token::Whitespace(String::from(" ")),
        Token::Newline,
        Token::Literal(String::from("do")),
        Token::Whitespace(String::from(" ")),
        Token::Literal(String::from("echo")),
        Token::Whitespace(String::from(" ")),
        Token::Dollar,
        Token::Literal(String::from("var")),
        Token::Newline,
        Token::Literal(String::from("done")),
    ]);
    assert_eq!(
        Err(IncompleteCmd("for", src(0, 1, 1), "in", src(8, 1, 9))),
        p.for_command()
    );
}

#[test]
fn test_for_command_should_recognize_literals_and_names() {
    for for_tok in vec![
        Token::Literal(String::from("for")),
        Token::Name(String::from("for")),
    ] {
        for in_tok in vec![
            Token::Literal(String::from("in")),
            Token::Name(String::from("in")),
        ] {
            let mut p = make_parser_from_tokens(vec![
                for_tok.clone(),
                Token::Whitespace(String::from(" ")),
                Token::Name(String::from("var")),
                Token::Whitespace(String::from(" ")),
                in_tok.clone(),
                Token::Whitespace(String::from(" ")),
                Token::Literal(String::from("one")),
                Token::Whitespace(String::from(" ")),
                Token::Literal(String::from("two")),
                Token::Whitespace(String::from(" ")),
                Token::Literal(String::from("three")),
                Token::Whitespace(String::from(" ")),
                Token::Newline,
                Token::Literal(String::from("do")),
                Token::Whitespace(String::from(" ")),
                Token::Literal(String::from("echo")),
                Token::Whitespace(String::from(" ")),
                Token::Dollar,
                Token::Name(String::from("var")),
                Token::Newline,
                Token::Literal(String::from("done")),
            ]);
            p.for_command().unwrap();
        }
    }
}
