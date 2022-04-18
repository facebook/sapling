#![deny(rust_2018_idioms)]
use conch_parser::ast::Parameter::*;
use conch_parser::ast::ParameterSubstitution::*;
use conch_parser::parse::ParseError::*;

mod parse_support;
use crate::parse_support::*;

#[test]
fn test_parameter_short() {
    let words = vec![At, Star, Pound, Question, Dash, Dollar, Bang, Positional(3)];

    let mut p = make_parser("$@$*$#$?$-$$$!$3$");
    for param in words {
        assert_eq!(p.parameter().unwrap(), word_param(param));
    }

    assert_eq!(word("$"), p.parameter().unwrap());
    assert_eq!(Err(UnexpectedEOF), p.parameter()); // Stream should be exhausted
}

#[test]
fn test_parameter_short_in_curlies() {
    let words = vec![
        At,
        Star,
        Pound,
        Question,
        Dash,
        Dollar,
        Bang,
        Var(String::from("foo")),
        Positional(3),
        Positional(1000),
    ];

    let mut p = make_parser("${@}${*}${#}${?}${-}${$}${!}${foo}${3}${1000}");
    for param in words {
        assert_eq!(p.parameter().unwrap(), word_param(param));
    }

    assert_eq!(Err(UnexpectedEOF), p.parameter()); // Stream should be exhausted
}

#[test]
fn test_parameter_command_substitution() {
    let correct = word_subst(Command(vec![
        cmd_args("echo", &["hello"]),
        cmd_args("echo", &["world"]),
    ]));

    assert_eq!(
        correct,
        make_parser("$(echo hello; echo world)")
            .parameter()
            .unwrap()
    );
}

#[test]
fn test_parameter_command_substitution_valid_empty_substitution() {
    let correct = word_subst(Command(vec![]));
    assert_eq!(correct, make_parser("$()").parameter().unwrap());
    assert_eq!(correct, make_parser("$(     )").parameter().unwrap());
    assert_eq!(correct, make_parser("$(\n\n)").parameter().unwrap());
}

#[test]
fn test_parameter_literal_dollar_if_no_param() {
    let mut p = make_parser("$%asdf");
    assert_eq!(word("$"), p.parameter().unwrap());
    assert_eq!(p.word().unwrap().unwrap(), word("%asdf"));
}
