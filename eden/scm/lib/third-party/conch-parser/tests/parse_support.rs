// Certain helpers may only be used by specific tests,
// suppress dead_code warnings since the compiler can't
// see our intent
#![allow(dead_code)]

use conch_parser::ast::Command::*;
use conch_parser::ast::ComplexWord::*;
use conch_parser::ast::PipeableCommand::*;
use conch_parser::ast::SimpleWord::*;
use conch_parser::ast::*;
use conch_parser::lexer::Lexer;
use conch_parser::parse::*;
use conch_parser::token::Token;

pub fn lit(s: &str) -> DefaultWord {
    Word::Simple(Literal(String::from(s)))
}

pub fn escaped(s: &str) -> DefaultWord {
    Word::Simple(Escaped(String::from(s)))
}

pub fn subst(s: DefaultParameterSubstitution) -> DefaultWord {
    Word::Simple(Subst(Box::new(s)))
}

pub fn single_quoted(s: &str) -> TopLevelWord<String> {
    TopLevelWord(Single(Word::SingleQuoted(String::from(s))))
}

pub fn double_quoted(s: &str) -> TopLevelWord<String> {
    TopLevelWord(Single(Word::DoubleQuoted(vec![Literal(String::from(s))])))
}

pub fn word(s: &str) -> TopLevelWord<String> {
    TopLevelWord(Single(lit(s)))
}

pub fn word_escaped(s: &str) -> TopLevelWord<String> {
    TopLevelWord(Single(escaped(s)))
}

pub fn word_subst(s: DefaultParameterSubstitution) -> TopLevelWord<String> {
    TopLevelWord(Single(subst(s)))
}

pub fn word_param(p: DefaultParameter) -> TopLevelWord<String> {
    TopLevelWord(Single(Word::Simple(Param(p))))
}

pub fn make_parser(src: &str) -> DefaultParser<Lexer<std::str::Chars<'_>>> {
    DefaultParser::new(Lexer::new(src.chars()))
}

pub fn make_parser_from_tokens(src: Vec<Token>) -> DefaultParser<std::vec::IntoIter<Token>> {
    DefaultParser::new(src.into_iter())
}

pub fn cmd_args_simple(cmd: &str, args: &[&str]) -> Box<DefaultSimpleCommand> {
    let mut cmd_args = Vec::with_capacity(args.len() + 1);
    cmd_args.push(RedirectOrCmdWord::CmdWord(word(cmd)));
    cmd_args.extend(args.iter().map(|&a| RedirectOrCmdWord::CmdWord(word(a))));

    Box::new(SimpleCommand {
        redirects_or_env_vars: vec![],
        redirects_or_cmd_words: cmd_args,
    })
}

pub fn cmd_simple(cmd: &str) -> Box<DefaultSimpleCommand> {
    cmd_args_simple(cmd, &[])
}

pub fn cmd_args(cmd: &str, args: &[&str]) -> TopLevelCommand<String> {
    TopLevelCommand(List(CommandList {
        first: ListableCommand::Single(Simple(cmd_args_simple(cmd, args))),
        rest: vec![],
    }))
}

pub fn cmd(cmd: &str) -> TopLevelCommand<String> {
    cmd_args(cmd, &[])
}

pub fn cmd_from_simple(cmd: DefaultSimpleCommand) -> TopLevelCommand<String> {
    TopLevelCommand(List(CommandList {
        first: ListableCommand::Single(Simple(Box::new(cmd))),
        rest: vec![],
    }))
}

pub fn src(byte: usize, line: usize, col: usize) -> SourcePos {
    SourcePos { byte, line, col }
}
