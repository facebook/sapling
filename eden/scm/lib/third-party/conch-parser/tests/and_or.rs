#![deny(rust_2018_idioms)]
use conch_parser::ast::PipeableCommand::*;
use conch_parser::ast::*;
use conch_parser::parse::ParseError::*;
use conch_parser::token::Token;

mod parse_support;
use crate::parse_support::*;

#[test]
fn test_and_or_correct_associativity() {
    let mut p = make_parser("foo || bar && baz");
    let correct = CommandList {
        first: ListableCommand::Single(Simple(cmd_simple("foo"))),
        rest: vec![
            AndOr::Or(ListableCommand::Single(Simple(cmd_simple("bar")))),
            AndOr::And(ListableCommand::Single(Simple(cmd_simple("baz")))),
        ],
    };
    assert_eq!(correct, p.and_or_list().unwrap());
}

#[test]
fn test_and_or_valid_with_newlines_after_operator() {
    let mut p = make_parser("foo ||\n\n\n\nbar && baz");
    let correct = CommandList {
        first: ListableCommand::Single(Simple(cmd_simple("foo"))),
        rest: vec![
            AndOr::Or(ListableCommand::Single(Simple(cmd_simple("bar")))),
            AndOr::And(ListableCommand::Single(Simple(cmd_simple("baz")))),
        ],
    };
    assert_eq!(correct, p.and_or_list().unwrap());
}

#[test]
fn test_and_or_invalid_with_newlines_before_operator() {
    let mut p = make_parser("foo || bar\n\n&& baz");
    p.and_or_list().unwrap(); // Successful parse Or(foo, bar)
                              // Fail to parse "&& baz" which is an error
    assert_eq!(
        Err(Unexpected(Token::AndIf, src(12, 3, 1))),
        p.complete_command()
    );
}
