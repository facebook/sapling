#![deny(rust_2018_idioms)]
use conch_parser::ast::PipeableCommand::*;
use conch_parser::ast::Redirect::*;
use conch_parser::ast::*;

mod parse_support;
use crate::parse_support::*;

#[test]
fn test_simple_command_valid_assignments_at_start_of_command() {
    let mut p = make_parser("var=val ENV=true BLANK= foo bar baz");
    let correct = Simple(Box::new(SimpleCommand {
        redirects_or_env_vars: vec![
            RedirectOrEnvVar::EnvVar("var".to_owned(), Some(word("val"))),
            RedirectOrEnvVar::EnvVar("ENV".to_owned(), Some(word("true"))),
            RedirectOrEnvVar::EnvVar("BLANK".to_owned(), None),
        ],
        redirects_or_cmd_words: vec![
            RedirectOrCmdWord::CmdWord(word("foo")),
            RedirectOrCmdWord::CmdWord(word("bar")),
            RedirectOrCmdWord::CmdWord(word("baz")),
        ],
    }));
    assert_eq!(correct, p.simple_command().unwrap());
}

#[test]
fn test_simple_command_assignments_after_start_of_command_should_be_args() {
    let mut p = make_parser("var=val ENV=true BLANK= foo var2=val2 bar baz var3=val3");
    let correct = Simple(Box::new(SimpleCommand {
        redirects_or_env_vars: vec![
            RedirectOrEnvVar::EnvVar("var".to_owned(), Some(word("val"))),
            RedirectOrEnvVar::EnvVar("ENV".to_owned(), Some(word("true"))),
            RedirectOrEnvVar::EnvVar("BLANK".to_owned(), None),
        ],
        redirects_or_cmd_words: vec![
            RedirectOrCmdWord::CmdWord(word("foo")),
            RedirectOrCmdWord::CmdWord(word("var2=val2")),
            RedirectOrCmdWord::CmdWord(word("bar")),
            RedirectOrCmdWord::CmdWord(word("baz")),
            RedirectOrCmdWord::CmdWord(word("var3=val3")),
        ],
    }));
    assert_eq!(correct, p.simple_command().unwrap());
}

#[test]
fn test_simple_command_redirections_at_start_of_command() {
    let mut p = make_parser("2>|clob 3<>rw <in var=val ENV=true BLANK= foo bar baz");
    let correct = Simple(Box::new(SimpleCommand {
        redirects_or_env_vars: vec![
            RedirectOrEnvVar::Redirect(Clobber(Some(2), word("clob"))),
            RedirectOrEnvVar::Redirect(ReadWrite(Some(3), word("rw"))),
            RedirectOrEnvVar::Redirect(Read(None, word("in"))),
            RedirectOrEnvVar::EnvVar("var".to_owned(), Some(word("val"))),
            RedirectOrEnvVar::EnvVar("ENV".to_owned(), Some(word("true"))),
            RedirectOrEnvVar::EnvVar("BLANK".to_owned(), None),
        ],
        redirects_or_cmd_words: vec![
            RedirectOrCmdWord::CmdWord(word("foo")),
            RedirectOrCmdWord::CmdWord(word("bar")),
            RedirectOrCmdWord::CmdWord(word("baz")),
        ],
    }));
    assert_eq!(correct, p.simple_command().unwrap());
}

#[test]
fn test_simple_command_redirections_at_end_of_command() {
    let mut p = make_parser("var=val ENV=true BLANK= foo bar baz 2>|clob 3<>rw <in");
    let correct = Simple(Box::new(SimpleCommand {
        redirects_or_env_vars: vec![
            RedirectOrEnvVar::EnvVar("var".to_owned(), Some(word("val"))),
            RedirectOrEnvVar::EnvVar("ENV".to_owned(), Some(word("true"))),
            RedirectOrEnvVar::EnvVar("BLANK".to_owned(), None),
        ],
        redirects_or_cmd_words: vec![
            RedirectOrCmdWord::CmdWord(word("foo")),
            RedirectOrCmdWord::CmdWord(word("bar")),
            RedirectOrCmdWord::CmdWord(word("baz")),
            RedirectOrCmdWord::Redirect(Clobber(Some(2), word("clob"))),
            RedirectOrCmdWord::Redirect(ReadWrite(Some(3), word("rw"))),
            RedirectOrCmdWord::Redirect(Read(None, word("in"))),
        ],
    }));
    assert_eq!(correct, p.simple_command().unwrap());
}

#[test]
fn test_simple_command_redirections_throughout_the_command() {
    let mut p = make_parser("2>|clob var=val 3<>rw ENV=true BLANK= foo bar <in baz 4>&-");
    let correct = Simple(Box::new(SimpleCommand {
        redirects_or_env_vars: vec![
            RedirectOrEnvVar::Redirect(Clobber(Some(2), word("clob"))),
            RedirectOrEnvVar::EnvVar("var".to_owned(), Some(word("val"))),
            RedirectOrEnvVar::Redirect(ReadWrite(Some(3), word("rw"))),
            RedirectOrEnvVar::EnvVar("ENV".to_owned(), Some(word("true"))),
            RedirectOrEnvVar::EnvVar("BLANK".to_owned(), None),
        ],
        redirects_or_cmd_words: vec![
            RedirectOrCmdWord::CmdWord(word("foo")),
            RedirectOrCmdWord::CmdWord(word("bar")),
            RedirectOrCmdWord::Redirect(Read(None, word("in"))),
            RedirectOrCmdWord::CmdWord(word("baz")),
            RedirectOrCmdWord::Redirect(DupWrite(Some(4), word("-"))),
        ],
    }));

    assert_eq!(correct, p.simple_command().unwrap());
}
