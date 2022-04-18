#![deny(rust_2018_idioms)]
use conch_parser::ast::PipeableCommand::*;
use conch_parser::ast::*;
use conch_parser::parse::ParseError::*;
use conch_parser::token::Token;

mod parse_support;
use crate::parse_support::*;

#[test]
fn test_pipeline_valid_bang() {
    let mut p = make_parser("! foo | bar | baz");
    let correct = CommandList {
        first: ListableCommand::Pipe(
            true,
            vec![
                Simple(cmd_simple("foo")),
                Simple(cmd_simple("bar")),
                Simple(cmd_simple("baz")),
            ],
        ),
        rest: vec![],
    };
    assert_eq!(correct, p.and_or_list().unwrap());
}

#[test]
fn test_pipeline_valid_bangs_in_and_or() {
    let mut p = make_parser("! foo | bar || ! baz && ! foobar");
    let correct = CommandList {
        first: ListableCommand::Pipe(
            true,
            vec![Simple(cmd_simple("foo")), Simple(cmd_simple("bar"))],
        ),
        rest: vec![
            AndOr::Or(ListableCommand::Pipe(true, vec![Simple(cmd_simple("baz"))])),
            AndOr::And(ListableCommand::Pipe(
                true,
                vec![Simple(cmd_simple("foobar"))],
            )),
        ],
    };
    assert_eq!(correct, p.and_or_list().unwrap());
}

#[test]
fn test_pipeline_no_bang_single_cmd_optimize_wrapper_out() {
    let mut p = make_parser("foo");
    let parse = p.pipeline().unwrap();
    if let ListableCommand::Pipe(..) = parse {
        panic!("Parser::pipeline should not create a wrapper if no ! present and only a single command");
    }
}

#[test]
fn test_pipeline_invalid_multiple_bangs_in_same_pipeline() {
    let mut p = make_parser("! foo | bar | ! baz");
    assert_eq!(Err(Unexpected(Token::Bang, src(14, 1, 15))), p.pipeline());
}
