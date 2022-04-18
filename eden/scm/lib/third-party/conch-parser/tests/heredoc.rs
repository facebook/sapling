#![deny(rust_2018_idioms)]
use conch_parser::ast::ComplexWord::*;
use conch_parser::ast::Redirect::Heredoc;
use conch_parser::ast::SimpleWord::*;
use conch_parser::ast::*;
use conch_parser::parse::ParseError::*;
use conch_parser::token::Token;

mod parse_support;
use crate::parse_support::*;

fn cat_heredoc(fd: Option<u16>, body: &'static str) -> TopLevelCommand<String> {
    cmd_from_simple(SimpleCommand {
        redirects_or_env_vars: vec![],
        redirects_or_cmd_words: vec![
            RedirectOrCmdWord::CmdWord(word("cat")),
            RedirectOrCmdWord::Redirect(Heredoc(fd, word(body))),
        ],
    })
}

#[test]
fn test_heredoc_valid() {
    let correct = Some(cat_heredoc(None, "hello\n"));
    assert_eq!(
        correct,
        make_parser("cat <<eof\nhello\neof\n")
            .complete_command()
            .unwrap()
    );
}

#[test]
fn test_heredoc_valid_eof_after_delimiter_allowed() {
    let correct = Some(cat_heredoc(None, "hello\n"));
    assert_eq!(
        correct,
        make_parser("cat <<eof\nhello\neof")
            .complete_command()
            .unwrap()
    );
}

#[test]
fn test_heredoc_valid_with_empty_body() {
    let correct = Some(cat_heredoc(None, ""));
    assert_eq!(
        correct,
        make_parser("cat <<eof\neof").complete_command().unwrap()
    );
    assert_eq!(
        correct,
        make_parser("cat <<eof\n").complete_command().unwrap()
    );
    assert_eq!(
        correct,
        make_parser("cat <<eof").complete_command().unwrap()
    );
}

#[test]
fn test_heredoc_valid_eof_acceptable_as_delimeter() {
    let correct = Some(cat_heredoc(None, "hello\n"));
    assert_eq!(
        correct,
        make_parser("cat <<eof\nhello\neof")
            .complete_command()
            .unwrap()
    );
}

#[test]
fn test_heredoc_valid_does_not_lose_tokens_up_to_next_newline() {
    let mut p = make_parser("cat <<eof1; cat 3<<eof2\nhello\neof1\nworld\neof2");
    let first = Some(cat_heredoc(None, "hello\n"));
    let second = Some(cat_heredoc(Some(3), "world\n"));

    assert_eq!(first, p.complete_command().unwrap());
    assert_eq!(second, p.complete_command().unwrap());
}

#[test]
fn test_heredoc_valid_space_before_delimeter_allowed() {
    let mut p = make_parser("cat <<   eof1; cat 3<<- eof2\nhello\neof1\nworld\neof2");
    let first = Some(cat_heredoc(None, "hello\n"));
    let second = Some(cat_heredoc(Some(3), "world\n"));

    assert_eq!(first, p.complete_command().unwrap());
    assert_eq!(second, p.complete_command().unwrap());
}

#[test]
fn test_heredoc_valid_unquoted_delimeter_should_expand_body() {
    let expanded = Some(cmd_from_simple(SimpleCommand {
        redirects_or_env_vars: vec![],
        redirects_or_cmd_words: vec![
            RedirectOrCmdWord::CmdWord(word("cat")),
            RedirectOrCmdWord::Redirect(Heredoc(
                None,
                TopLevelWord(Concat(vec![
                    Word::Simple(Param(Parameter::Dollar)),
                    lit(" "),
                    subst(ParameterSubstitution::Len(Parameter::Bang)),
                    lit(" "),
                    subst(ParameterSubstitution::Command(vec![cmd("foo")])),
                    lit("\n"),
                ])),
            )),
        ],
    }));

    let literal = Some(cat_heredoc(None, "$$ ${#!} `foo`\n"));

    assert_eq!(
        expanded,
        make_parser("cat <<eof\n$$ ${#!} `foo`\neof")
            .complete_command()
            .unwrap()
    );
    assert_eq!(
        literal,
        make_parser("cat <<'eof'\n$$ ${#!} `foo`\neof")
            .complete_command()
            .unwrap()
    );
    assert_eq!(
        literal,
        make_parser("cat <<`eof`\n$$ ${#!} `foo`\n`eof`")
            .complete_command()
            .unwrap()
    );
    assert_eq!(
        literal,
        make_parser("cat <<\"eof\"\n$$ ${#!} `foo`\neof")
            .complete_command()
            .unwrap()
    );
    assert_eq!(
        literal,
        make_parser("cat <<e\\of\n$$ ${#!} `foo`\neof")
            .complete_command()
            .unwrap()
    );
}

#[test]
fn test_heredoc_valid_leading_tab_removal_works() {
    let mut p =
        make_parser("cat <<-eof1; cat 3<<-eof2\n\t\thello\n\teof1\n\t\t \t\nworld\n\t\teof2");
    let first = Some(cat_heredoc(None, "hello\n"));
    let second = Some(cat_heredoc(Some(3), " \t\nworld\n"));

    assert_eq!(first, p.complete_command().unwrap());
    assert_eq!(second, p.complete_command().unwrap());
}

#[test]
fn test_heredoc_valid_leading_tab_removal_works_if_dash_immediately_after_dless() {
    let mut p = make_parser("cat 3<< -eof\n\t\t \t\nworld\n\t\teof\n\t\t-eof\n-eof");
    let correct = Some(cat_heredoc(Some(3), "\t\t \t\nworld\n\t\teof\n\t\t-eof\n"));
    assert_eq!(correct, p.complete_command().unwrap());
}

#[test]
fn test_heredoc_valid_unquoted_backslashes_in_delimeter_disappear() {
    let correct = Some(cat_heredoc(None, "hello\n"));
    assert_eq!(
        correct,
        make_parser("cat <<e\\ f\\f\nhello\ne ff")
            .complete_command()
            .unwrap()
    );
}

#[test]
fn test_heredoc_valid_balanced_single_quotes_in_delimeter() {
    let correct = Some(cat_heredoc(None, "hello\n"));
    assert_eq!(
        correct,
        make_parser("cat <<e'o'f\nhello\neof")
            .complete_command()
            .unwrap()
    );
}

#[test]
fn test_heredoc_valid_balanced_double_quotes_in_delimeter() {
    let correct = Some(cat_heredoc(None, "hello\n"));
    assert_eq!(
        correct,
        make_parser("cat <<e\"\\o${foo}\"f\nhello\ne\\o${foo}f")
            .complete_command()
            .unwrap()
    );
}

#[test]
fn test_heredoc_valid_balanced_backticks_in_delimeter() {
    let correct = Some(cat_heredoc(None, "hello\n"));
    assert_eq!(
        correct,
        make_parser("cat <<e`\\o\\$\\`\\\\${f}`\nhello\ne`\\o$`\\${f}`")
            .complete_command()
            .unwrap()
    );
}

#[test]
fn test_heredoc_valid_balanced_parens_in_delimeter() {
    let correct = Some(cat_heredoc(None, "hello\n"));
    assert_eq!(
        correct,
        make_parser("cat <<eof(  )\nhello\neof(  )")
            .complete_command()
            .unwrap()
    );
}

#[test]
fn test_heredoc_valid_cmd_subst_in_delimeter() {
    let correct = Some(cat_heredoc(None, "hello\n"));
    assert_eq!(
        correct,
        make_parser("cat <<eof$(  )\nhello\neof$(  )")
            .complete_command()
            .unwrap()
    );
}

#[test]
fn test_heredoc_valid_param_subst_in_delimeter() {
    let correct = Some(cat_heredoc(None, "hello\n"));
    assert_eq!(
        correct,
        make_parser("cat <<eof${  }\nhello\neof${  }")
            .complete_command()
            .unwrap()
    );
}

#[test]
fn test_heredoc_valid_skip_past_newlines_in_single_quotes() {
    let correct = Some(cmd_from_simple(SimpleCommand {
        redirects_or_env_vars: vec![],
        redirects_or_cmd_words: vec![
            RedirectOrCmdWord::CmdWord(word("cat")),
            RedirectOrCmdWord::Redirect(Heredoc(None, word("here\n"))),
            RedirectOrCmdWord::CmdWord(single_quoted("\n")),
            RedirectOrCmdWord::CmdWord(word("arg")),
        ],
    }));
    assert_eq!(
        correct,
        make_parser("cat <<EOF '\n' arg\nhere\nEOF")
            .complete_command()
            .unwrap()
    );
}

#[test]
fn test_heredoc_valid_skip_past_newlines_in_double_quotes() {
    let correct = Some(cmd_from_simple(SimpleCommand {
        redirects_or_env_vars: vec![],
        redirects_or_cmd_words: vec![
            RedirectOrCmdWord::CmdWord(word("cat")),
            RedirectOrCmdWord::Redirect(Heredoc(None, word("here\n"))),
            RedirectOrCmdWord::CmdWord(double_quoted("\n")),
            RedirectOrCmdWord::CmdWord(word("arg")),
        ],
    }));
    assert_eq!(
        correct,
        make_parser("cat <<EOF \"\n\" arg\nhere\nEOF")
            .complete_command()
            .unwrap()
    );
}

#[test]
fn test_heredoc_valid_skip_past_newlines_in_backticks() {
    let correct = Some(cmd_from_simple(SimpleCommand {
        redirects_or_env_vars: vec![],
        redirects_or_cmd_words: vec![
            RedirectOrCmdWord::CmdWord(word("cat")),
            RedirectOrCmdWord::Redirect(Heredoc(None, word("here\n"))),
            RedirectOrCmdWord::CmdWord(word_subst(ParameterSubstitution::Command(vec![cmd(
                "echo",
            )]))),
            RedirectOrCmdWord::CmdWord(word("arg")),
        ],
    }));
    assert_eq!(
        correct,
        make_parser("cat <<EOF `echo \n` arg\nhere\nEOF")
            .complete_command()
            .unwrap()
    );
}

#[test]
fn test_heredoc_valid_skip_past_newlines_in_parens() {
    let correct = Some(cat_heredoc(None, "here\n"));
    assert_eq!(
        correct,
        make_parser("cat <<EOF; (foo\n); arg\nhere\nEOF")
            .complete_command()
            .unwrap()
    );
}

#[test]
fn test_heredoc_valid_skip_past_newlines_in_cmd_subst() {
    let correct = Some(cmd_from_simple(SimpleCommand {
        redirects_or_env_vars: vec![],
        redirects_or_cmd_words: vec![
            RedirectOrCmdWord::CmdWord(word("cat")),
            RedirectOrCmdWord::Redirect(Heredoc(None, word("here\n"))),
            RedirectOrCmdWord::CmdWord(word_subst(ParameterSubstitution::Command(vec![cmd(
                "foo",
            )]))),
            RedirectOrCmdWord::CmdWord(word("arg")),
        ],
    }));
    assert_eq!(
        correct,
        make_parser("cat <<EOF $(foo\n) arg\nhere\nEOF")
            .complete_command()
            .unwrap()
    );
}

#[test]
fn test_heredoc_valid_skip_past_newlines_in_param_subst() {
    let correct = Some(cmd_from_simple(SimpleCommand {
        redirects_or_env_vars: vec![],
        redirects_or_cmd_words: vec![
            RedirectOrCmdWord::CmdWord(word("cat")),
            RedirectOrCmdWord::Redirect(Heredoc(None, word("here\n"))),
            RedirectOrCmdWord::CmdWord(word_subst(ParameterSubstitution::Assign(
                false,
                Parameter::Var(String::from("foo")),
                Some(word("\n")),
            ))),
            RedirectOrCmdWord::CmdWord(word("arg")),
        ],
    }));
    assert_eq!(
        correct,
        make_parser("cat <<EOF ${foo=\n} arg\nhere\nEOF")
            .complete_command()
            .unwrap()
    );
}

#[test]
fn test_heredoc_valid_skip_past_escaped_newlines() {
    let correct = Some(cmd_from_simple(SimpleCommand {
        redirects_or_env_vars: vec![],
        redirects_or_cmd_words: vec![
            RedirectOrCmdWord::CmdWord(word("cat")),
            RedirectOrCmdWord::Redirect(Heredoc(None, word("here\n"))),
            RedirectOrCmdWord::CmdWord(word("arg")),
        ],
    }));
    assert_eq!(
        correct,
        make_parser("cat <<EOF \\\n arg\nhere\nEOF")
            .complete_command()
            .unwrap()
    );
}

#[test]
fn test_heredoc_valid_double_quoted_delim_keeps_backslashe_except_after_specials() {
    let correct = Some(cat_heredoc(None, "here\n"));
    assert_eq!(
        correct,
        make_parser("cat <<\"\\EOF\\$\\`\\\"\\\\\"\nhere\n\\EOF$`\"\\\n")
            .complete_command()
            .unwrap()
    );
}

#[test]
fn test_heredoc_valid_unquoting_only_removes_outer_quotes_and_backslashes() {
    let correct = Some(cat_heredoc(None, "here\n"));
    assert_eq!(
        correct,
        make_parser("cat <<EOF${ 'asdf'}(\"hello'\"){\\o}\nhere\nEOF${ asdf}(hello'){o}")
            .complete_command()
            .unwrap()
    );
}

#[test]
fn test_heredoc_valid_delimeter_can_be_followed_by_carriage_return_newline() {
    let correct = Some(cmd_from_simple(SimpleCommand {
        redirects_or_env_vars: vec![],
        redirects_or_cmd_words: vec![
            RedirectOrCmdWord::CmdWord(word("cat")),
            RedirectOrCmdWord::Redirect(Heredoc(None, word("here\n"))),
            RedirectOrCmdWord::CmdWord(word("arg")),
        ],
    }));
    assert_eq!(
        correct,
        make_parser("cat <<EOF arg\nhere\nEOF\r\n")
            .complete_command()
            .unwrap()
    );

    let correct = Some(cmd_from_simple(SimpleCommand {
        redirects_or_env_vars: vec![],
        redirects_or_cmd_words: vec![
            RedirectOrCmdWord::CmdWord(word("cat")),
            RedirectOrCmdWord::Redirect(Heredoc(None, word("here\r\n"))),
            RedirectOrCmdWord::CmdWord(word("arg")),
        ],
    }));
    assert_eq!(
        correct,
        make_parser("cat <<EOF arg\nhere\r\nEOF\r\n")
            .complete_command()
            .unwrap()
    );
}

#[test]
fn test_heredoc_valid_delimiter_can_start_with() {
    let correct = Some(cat_heredoc(None, "\thello\n\t\tworld\n"));
    assert_eq!(
        correct,
        make_parser("cat << -EOF\n\thello\n\t\tworld\n-EOF")
            .complete_command()
            .unwrap()
    );

    let correct = Some(cat_heredoc(None, "hello\nworld\n"));
    assert_eq!(
        correct,
        make_parser("cat <<--EOF\n\thello\n\t\tworld\n-EOF")
            .complete_command()
            .unwrap()
    );
}

#[test]
fn test_heredoc_invalid_missing_delimeter() {
    assert_eq!(
        Err(Unexpected(Token::Semi, src(7, 1, 8))),
        make_parser("cat << ;").complete_command()
    );
}

#[test]
fn test_heredoc_invalid_unbalanced_quoting() {
    assert_eq!(
        Err(Unmatched(Token::SingleQuote, src(6, 1, 7))),
        make_parser("cat <<'eof").complete_command()
    );
    assert_eq!(
        Err(Unmatched(Token::Backtick, src(6, 1, 7))),
        make_parser("cat <<`eof").complete_command()
    );
    assert_eq!(
        Err(Unmatched(Token::DoubleQuote, src(6, 1, 7))),
        make_parser("cat <<\"eof").complete_command()
    );
    assert_eq!(
        Err(Unmatched(Token::ParenOpen, src(9, 1, 10))),
        make_parser("cat <<eof(").complete_command()
    );
    assert_eq!(
        Err(Unmatched(Token::ParenOpen, src(10, 1, 11))),
        make_parser("cat <<eof$(").complete_command()
    );
    assert_eq!(
        Err(Unmatched(Token::CurlyOpen, src(10, 1, 11))),
        make_parser("cat <<eof${").complete_command()
    );
}

#[test]
fn test_heredoc_invalid_shows_right_position_of_error() {
    let mut p = make_parser("cat <<EOF\nhello\n${invalid subst\nEOF");
    assert_eq!(
        Err(BadSubst(
            Token::Whitespace(String::from(" ")),
            src(25, 3, 10)
        )),
        p.complete_command()
    );
}

#[test]
fn test_heredoc_invalid_shows_right_position_of_error_when_tabs_stripped() {
    let mut p = make_parser("cat <<-EOF\n\t\thello\n\t\t${invalid subst\n\t\t\tEOF");
    assert_eq!(
        Err(BadSubst(
            Token::Whitespace(String::from(" ")),
            src(30, 3, 12)
        )),
        p.complete_command()
    );
}

#[test]
fn test_heredoc_keeps_track_of_correct_position_after_redirect() {
    let mut p = make_parser("cat <<EOF arg ()\nhello\nEOF");
    // Grab the heredoc command
    p.complete_command().unwrap();
    // Fail on the ()
    assert_eq!(
        Err(Unexpected(Token::ParenClose, src(15, 1, 16))),
        p.complete_command()
    );
}
