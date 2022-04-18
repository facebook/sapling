#![deny(rust_2018_idioms)]
use conch_parser::ast::ComplexWord::*;
use conch_parser::ast::PipeableCommand::*;
use conch_parser::ast::SimpleWord::*;
use conch_parser::ast::*;
use conch_parser::parse::ParseError::*;
use conch_parser::token::Token;

mod parse_support;
use crate::parse_support::*;

fn simple_command_with_redirect(cmd: &str, redirect: DefaultRedirect) -> DefaultPipeableCommand {
    Simple(Box::new(SimpleCommand {
        redirects_or_env_vars: vec![],
        redirects_or_cmd_words: vec![
            RedirectOrCmdWord::CmdWord(word(cmd)),
            RedirectOrCmdWord::Redirect(redirect),
        ],
    }))
}

#[test]
fn test_redirect_valid_close_without_whitespace() {
    let mut p = make_parser(">&-");
    assert_eq!(
        Some(Ok(Redirect::DupWrite(None, word("-")))),
        p.redirect().unwrap()
    );
}

#[test]
fn test_redirect_valid_close_with_whitespace() {
    let mut p = make_parser("<&       -");
    assert_eq!(
        Some(Ok(Redirect::DupRead(None, word("-")))),
        p.redirect().unwrap()
    );
}

#[test]
fn test_redirect_valid_start_with_dash_if_not_dup() {
    let path = word("-test");
    let cases = vec![
        ("4<-test", Redirect::Read(Some(4), path.clone())),
        ("4>-test", Redirect::Write(Some(4), path.clone())),
        ("4<>-test", Redirect::ReadWrite(Some(4), path.clone())),
        ("4>>-test", Redirect::Append(Some(4), path.clone())),
        ("4>|-test", Redirect::Clobber(Some(4), path)),
    ];

    for (s, correct) in cases.into_iter() {
        match make_parser(s).redirect() {
            Ok(Some(Ok(ref r))) if *r == correct => {}
            Ok(r) => panic!(
                "Unexpectedly parsed the source \"{}\" as\n{:?} instead of\n{:?}",
                s, r, correct
            ),
            Err(err) => panic!("Failed to parse the source \"{}\": {}", s, err),
        }
    }
}

#[test]
fn test_redirect_valid_return_word_if_no_redirect() {
    let mut p = make_parser("hello");
    assert_eq!(Some(Err(word("hello"))), p.redirect().unwrap());
}

#[test]
fn test_redirect_valid_return_word_if_src_fd_is_definitely_non_numeric() {
    let mut p = make_parser("123$$'foo'>out");
    let correct = TopLevelWord(Concat(vec![
        lit("123"),
        Word::Simple(Param(Parameter::Dollar)),
        Word::SingleQuoted(String::from("foo")),
    ]));
    assert_eq!(Some(Err(correct)), p.redirect().unwrap());
}

#[test]
fn test_redirect_valid_return_word_if_src_fd_has_escaped_numerics() {
    let mut p = make_parser("\\2>");
    let correct = word_escaped("2");
    assert_eq!(Some(Err(correct)), p.redirect().unwrap());
}

#[test]
fn test_redirect_valid_dst_fd_can_have_escaped_numerics() {
    let mut p = make_parser(">\\2");
    let correct = Redirect::Write(None, word_escaped("2"));
    assert_eq!(Some(Ok(correct)), p.redirect().unwrap());
}

#[test]
fn test_redirect_invalid_dup_if_dst_fd_is_definitely_non_numeric() {
    assert_eq!(
        Err(BadFd(src(2, 1, 3), src(12, 1, 13))),
        make_parser(">&123$$'foo'").redirect()
    );
}

#[test]
fn test_redirect_valid_dup_return_redirect_if_dst_fd_is_possibly_numeric() {
    let mut p = make_parser(">&123$$$(echo 2)`echo bar`");
    let correct = Redirect::DupWrite(
        None,
        TopLevelWord(Concat(vec![
            lit("123"),
            Word::Simple(Param(Parameter::Dollar)),
            subst(ParameterSubstitution::Command(vec![cmd_args(
                "echo",
                &["2"],
            )])),
            subst(ParameterSubstitution::Command(vec![cmd_args(
                "echo",
                &["bar"],
            )])),
        ])),
    );
    assert_eq!(Some(Ok(correct)), p.redirect().unwrap());
}

#[test]
fn test_redirect_invalid_close_without_whitespace() {
    assert_eq!(
        Err(BadFd(src(2, 1, 3), src(7, 1, 8))),
        make_parser(">&-asdf").redirect()
    );
}

#[test]
fn test_redirect_invalid_close_with_whitespace() {
    assert_eq!(
        Err(BadFd(src(9, 1, 10), src(14, 1, 15))),
        make_parser("<&       -asdf").redirect()
    );
}

#[test]
fn test_redirect_fd_immediately_preceeding_redirection() {
    let mut p = make_parser("foo 1>>out");
    let cmd = p.simple_command().unwrap();
    assert_eq!(
        cmd,
        simple_command_with_redirect("foo", Redirect::Append(Some(1), word("out")))
    );
}

#[test]
fn test_redirect_fd_must_immediately_preceed_redirection() {
    let correct = Simple(Box::new(SimpleCommand {
        redirects_or_env_vars: vec![],
        redirects_or_cmd_words: vec![
            RedirectOrCmdWord::CmdWord(word("foo")),
            RedirectOrCmdWord::CmdWord(word("1")),
            RedirectOrCmdWord::Redirect(Redirect::ReadWrite(None, word("out"))),
        ],
    }));

    let mut p = make_parser("foo 1 <>out");
    assert_eq!(p.simple_command().unwrap(), correct);
}

#[test]
fn test_redirect_valid_dup_with_fd() {
    let mut p = make_parser("foo 1>&2");
    let cmd = p.simple_command().unwrap();
    assert_eq!(
        cmd,
        simple_command_with_redirect("foo", Redirect::DupWrite(Some(1), word("2")))
    );
}

#[test]
fn test_redirect_valid_dup_without_fd() {
    let correct = Simple(Box::new(SimpleCommand {
        redirects_or_env_vars: vec![],
        redirects_or_cmd_words: vec![
            RedirectOrCmdWord::CmdWord(word("foo")),
            RedirectOrCmdWord::CmdWord(word("1")),
            RedirectOrCmdWord::Redirect(Redirect::DupRead(None, word("2"))),
        ],
    }));

    let mut p = make_parser("foo 1 <&2");
    assert_eq!(p.simple_command().unwrap(), correct);
}

#[test]
fn test_redirect_valid_dup_with_whitespace() {
    let mut p = make_parser("foo 1<& 2");
    let cmd = p.simple_command().unwrap();
    assert_eq!(
        cmd,
        simple_command_with_redirect("foo", Redirect::DupRead(Some(1), word("2")))
    );
}

#[test]
fn test_redirect_valid_single_quoted_dup_fd() {
    let correct = Redirect::DupWrite(Some(1), single_quoted("2"));
    assert_eq!(Some(Ok(correct)), make_parser("1>&'2'").redirect().unwrap());
}

#[test]
fn test_redirect_valid_double_quoted_dup_fd() {
    let correct = Redirect::DupWrite(None, double_quoted("2"));
    assert_eq!(
        Some(Ok(correct)),
        make_parser(">&\"2\"").redirect().unwrap()
    );
}

#[test]
fn test_redirect_src_fd_need_not_be_single_token() {
    let mut p = make_parser_from_tokens(vec![
        Token::Literal(String::from("foo")),
        Token::Whitespace(String::from(" ")),
        Token::Literal(String::from("12")),
        Token::Literal(String::from("34")),
        Token::LessAnd,
        Token::Dash,
    ]);

    let cmd = p.simple_command().unwrap();
    assert_eq!(
        cmd,
        simple_command_with_redirect("foo", Redirect::DupRead(Some(1234), word("-")))
    );
}

#[test]
fn test_redirect_dst_fd_need_not_be_single_token() {
    let mut p = make_parser_from_tokens(vec![
        Token::GreatAnd,
        Token::Literal(String::from("12")),
        Token::Literal(String::from("34")),
    ]);

    let correct = Redirect::DupWrite(None, word("1234"));
    assert_eq!(Some(Ok(correct)), p.redirect().unwrap());
}

#[test]
fn test_redirect_accept_literal_and_name_tokens() {
    let mut p = make_parser_from_tokens(vec![Token::GreatAnd, Token::Literal(String::from("12"))]);
    assert_eq!(
        Some(Ok(Redirect::DupWrite(None, word("12")))),
        p.redirect().unwrap()
    );

    let mut p = make_parser_from_tokens(vec![Token::GreatAnd, Token::Name(String::from("12"))]);
    assert_eq!(
        Some(Ok(Redirect::DupWrite(None, word("12")))),
        p.redirect().unwrap()
    );
}

#[test]
fn test_redirect_list_valid() {
    let mut p = make_parser("1>>out <& 2 2>&-");
    let io = p.redirect_list().unwrap();
    assert_eq!(
        io,
        vec!(
            Redirect::Append(Some(1), word("out")),
            Redirect::DupRead(None, word("2")),
            Redirect::DupWrite(Some(2), word("-")),
        )
    );
}

#[test]
fn test_redirect_list_rejects_non_fd_words() {
    assert_eq!(
        Err(BadFd(src(16, 1, 17), src(19, 1, 20))),
        make_parser("1>>out <in 2>&- abc").redirect_list()
    );
    assert_eq!(
        Err(BadFd(src(7, 1, 8), src(10, 1, 11))),
        make_parser("1>>out abc<in 2>&-").redirect_list()
    );
    assert_eq!(
        Err(BadFd(src(7, 1, 8), src(10, 1, 11))),
        make_parser("1>>out abc <in 2>&-").redirect_list()
    );
}
