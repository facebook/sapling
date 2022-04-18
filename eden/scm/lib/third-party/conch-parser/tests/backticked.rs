#![deny(rust_2018_idioms)]
use conch_parser::ast::ComplexWord::*;
use conch_parser::ast::SimpleWord::*;
use conch_parser::ast::*;
use conch_parser::parse::ParseError::*;
use conch_parser::token::Token;

mod parse_support;
use crate::parse_support::*;

#[test]
fn test_backticked_valid() {
    let correct = word_subst(ParameterSubstitution::Command(vec![cmd("foo")]));
    assert_eq!(
        correct,
        make_parser("`foo`")
            .backticked_command_substitution()
            .unwrap()
    );
}

#[test]
fn test_backticked_valid_backslashes_removed_if_before_dollar_backslash_and_backtick() {
    let correct = word_subst(ParameterSubstitution::Command(vec![cmd_from_simple(
        SimpleCommand {
            redirects_or_env_vars: vec![],
            redirects_or_cmd_words: vec![
                RedirectOrCmdWord::CmdWord(word("foo")),
                RedirectOrCmdWord::CmdWord(TopLevelWord(Concat(vec![
                    Word::Simple(Param(Parameter::Dollar)),
                    escaped("`"),
                    escaped("o"),
                ]))),
            ],
        },
    )]));
    assert_eq!(
        correct,
        make_parser("`foo \\$\\$\\\\\\`\\o`")
            .backticked_command_substitution()
            .unwrap()
    );
}

#[test]
fn test_backticked_nested_backticks() {
    let correct = word_subst(ParameterSubstitution::Command(vec![cmd_from_simple(
        SimpleCommand {
            redirects_or_env_vars: vec![],
            redirects_or_cmd_words: vec![
                RedirectOrCmdWord::CmdWord(word("foo")),
                RedirectOrCmdWord::CmdWord(word_subst(ParameterSubstitution::Command(vec![
                    cmd_from_simple(SimpleCommand {
                        redirects_or_env_vars: vec![],
                        redirects_or_cmd_words: vec![
                            RedirectOrCmdWord::CmdWord(word("bar")),
                            RedirectOrCmdWord::CmdWord(TopLevelWord(Concat(vec![
                                escaped("$"),
                                escaped("$"),
                            ]))),
                        ],
                    }),
                ]))),
            ],
        },
    )]));
    assert_eq!(
        correct,
        make_parser(r#"`foo \`bar \\\\$\\\\$\``"#)
            .backticked_command_substitution()
            .unwrap()
    );
}

#[test]
fn test_backticked_nested_backticks_x2() {
    let correct = word_subst(ParameterSubstitution::Command(vec![cmd_from_simple(
        SimpleCommand {
            redirects_or_env_vars: vec![],
            redirects_or_cmd_words: vec![
                RedirectOrCmdWord::CmdWord(word("foo")),
                RedirectOrCmdWord::CmdWord(word_subst(ParameterSubstitution::Command(vec![
                    cmd_from_simple(SimpleCommand {
                        redirects_or_env_vars: vec![],
                        redirects_or_cmd_words: vec![
                            RedirectOrCmdWord::CmdWord(word("bar")),
                            RedirectOrCmdWord::CmdWord(word_subst(ParameterSubstitution::Command(
                                vec![cmd_from_simple(SimpleCommand {
                                    redirects_or_env_vars: vec![],
                                    redirects_or_cmd_words: vec![
                                        RedirectOrCmdWord::CmdWord(word("baz")),
                                        RedirectOrCmdWord::CmdWord(TopLevelWord(Concat(vec![
                                            escaped("$"),
                                            escaped("$"),
                                        ]))),
                                    ],
                                })],
                            ))),
                        ],
                    }),
                ]))),
            ],
        },
    )]));
    assert_eq!(
        correct,
        make_parser(r#"`foo \`bar \\\`baz \\\\\\\\$\\\\\\\\$ \\\`\``"#)
            .backticked_command_substitution()
            .unwrap()
    );
}

#[test]
fn test_backticked_nested_backticks_x3() {
    let correct = word_subst(ParameterSubstitution::Command(vec![cmd_from_simple(
        SimpleCommand {
            redirects_or_env_vars: vec![],
            redirects_or_cmd_words: vec![
                RedirectOrCmdWord::CmdWord(word("foo")),
                RedirectOrCmdWord::CmdWord(word_subst(ParameterSubstitution::Command(vec![
                    cmd_from_simple(SimpleCommand {
                        redirects_or_env_vars: vec![],
                        redirects_or_cmd_words: vec![
                            RedirectOrCmdWord::CmdWord(word("bar")),
                            RedirectOrCmdWord::CmdWord(word_subst(ParameterSubstitution::Command(
                                vec![cmd_from_simple(SimpleCommand {
                                    redirects_or_env_vars: vec![],
                                    redirects_or_cmd_words: vec![
                                        RedirectOrCmdWord::CmdWord(word("baz")),
                                        RedirectOrCmdWord::CmdWord(word_subst(
                                            ParameterSubstitution::Command(vec![cmd_from_simple(
                                                SimpleCommand {
                                                    redirects_or_env_vars: vec![],
                                                    redirects_or_cmd_words: vec![
                                                        RedirectOrCmdWord::CmdWord(word("qux")),
                                                        RedirectOrCmdWord::CmdWord(TopLevelWord(
                                                            Concat(vec![
                                                                escaped("$"),
                                                                escaped("$"),
                                                            ]),
                                                        )),
                                                    ],
                                                },
                                            )]),
                                        )),
                                    ],
                                })],
                            ))),
                        ],
                    }),
                ]))),
            ],
        },
    )]));
    assert_eq!(
        correct,
        make_parser(
            r#"`foo \`bar \\\`baz \\\\\\\`qux \\\\\\\\\\\\\\\\$\\\\\\\\\\\\\\\\$ \\\\\\\` \\\`\``"#
        )
        .backticked_command_substitution()
        .unwrap()
    );
}

#[test]
fn test_backticked_invalid_missing_closing_backtick() {
    let src = [
        // Missing outermost backtick
        (r#"`foo"#, src(0, 1, 1)),
        (r#"`foo \`bar \\\\$\\\\$\`"#, src(0, 1, 1)),
        (
            r#"`foo \`bar \\\`baz \\\\\\\\$\\\\\\\\$ \\\`\`"#,
            src(0, 1, 1),
        ),
        (
            r#"`foo \`bar \\\`baz \\\\\\\`qux \\\\\\\\\\\\\\\\$ \\\\\\\\\\\\\\\\$ \\\\\\\` \\\`\`"#,
            src(0, 1, 1),
        ),
        // Missing second to last backtick
        (r#"`foo \`bar \\\\$\\\\$`"#, src(6, 1, 7)),
        (
            r#"`foo \`bar \\\`baz \\\\\\\\$\\\\\\\\$ \\\``"#,
            src(6, 1, 7),
        ),
        (
            r#"`foo \`bar \\\`baz \\\\\\\`qux \\\\\\\\\\\\\\\\$ \\\\\\\\\\\\\\\\$ \\\\\\\` \\\``"#,
            src(6, 1, 7),
        ),
        // Missing third to last backtick
        (
            r#"`foo \`bar \\\`baz \\\\\\\\$\\\\\\\\$ \``"#,
            src(14, 1, 15),
        ),
        (
            r#"`foo \`bar \\\`baz \\\\\\\`qux \\\\\\\\\\\\\\\\$ \\\\\\\\\\\\\\\\$ \\\\\\\` \``"#,
            src(14, 1, 15),
        ),
        // Missing fourth to last backtick
        (
            r#"`foo \`bar \\\`baz \\\\\\\`qux \\\\\\\\\\\\\\\\$ \\\\\\\\\\\\\\\\$ \\\`\``"#,
            src(26, 1, 27),
        ),
    ];

    for &(s, p) in &src {
        let correct = Unmatched(Token::Backtick, p);
        match make_parser(s).backticked_command_substitution() {
            Ok(w) => panic!("Unexpectedly parsed the source \"{}\" as\n{:?}", s, w),
            Err(ref err) => {
                if err != &correct {
                    panic!(
                        "Expected the source \"{}\" to return the error `{:?}`, but got `{:?}`",
                        s, correct, err
                    );
                }
            }
        }
    }
}

#[test]
fn test_backticked_invalid_maintains_accurate_source_positions() {
    let src = [
        (r#"`foo ${invalid param}`"#, src(14, 1, 15)),
        (r#"`foo \`bar ${invalid param}\``"#, src(20, 1, 21)),
        (
            r#"`foo \`bar \\\`baz ${invalid param} \\\`\``"#,
            src(28, 1, 29),
        ),
        (
            r#"`foo \`bar \\\`baz \\\\\\\`qux ${invalid param} \\\\\\\` \\\`\``"#,
            src(40, 1, 41),
        ),
    ];

    for &(s, p) in &src {
        let correct = BadSubst(Token::Whitespace(String::from(" ")), p);
        match make_parser(s).backticked_command_substitution() {
            Ok(w) => panic!("Unexpectedly parsed the source \"{}\" as\n{:?}", s, w),
            Err(ref err) => {
                if err != &correct {
                    panic!(
                        "Expected the source \"{}\" to return the error `{:?}`, but got `{:?}`",
                        s, correct, err
                    );
                }
            }
        }
    }
}

#[test]
fn test_backticked_invalid_missing_opening_backtick() {
    let mut p = make_parser("foo`");
    assert_eq!(
        Err(Unexpected(Token::Name(String::from("foo")), src(0, 1, 1))),
        p.backticked_command_substitution()
    );
}
