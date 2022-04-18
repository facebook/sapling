#![deny(rust_2018_idioms)]
use conch_parser::ast::builder::*;
use conch_parser::parse::ParseError::*;
use conch_parser::token::Token;

mod parse_support;
use crate::parse_support::*;

#[test]
fn test_case_command_valid() {
    let correct = CaseFragments {
        word: word("foo"),
        post_word_comments: vec![],
        in_comment: None,
        arms: vec![
            CaseArm {
                patterns: CasePatternFragments {
                    pre_pattern_comments: vec![],
                    pattern_alternatives: vec![word("hello"), word("goodbye")],
                    pattern_comment: None,
                },
                body: CommandGroup {
                    commands: vec![cmd_args("echo", &["greeting"])],
                    trailing_comments: vec![],
                },
                arm_comment: None,
            },
            CaseArm {
                patterns: CasePatternFragments {
                    pre_pattern_comments: vec![],
                    pattern_alternatives: vec![word("world")],
                    pattern_comment: None,
                },
                body: CommandGroup {
                    commands: vec![cmd_args("echo", &["noun"])],
                    trailing_comments: vec![],
                },
                arm_comment: None,
            },
        ],
        post_arms_comments: vec![],
    };

    let cases = vec![
        // `(` before pattern is optional
        "case foo in  hello | goodbye) echo greeting;;  world) echo noun;; esac",
        "case foo in (hello | goodbye) echo greeting;;  world) echo noun;; esac",
        "case foo in (hello | goodbye) echo greeting;; (world) echo noun;; esac",
        // Final `;;` is optional as long as last command is complete
        "case foo in hello | goodbye) echo greeting;; world) echo noun\nesac",
        "case foo in hello | goodbye) echo greeting;; world) echo noun; esac",
    ];

    for src in cases {
        assert_eq!(correct, make_parser(src).case_command().unwrap());
    }
}

#[test]
fn test_case_command_valid_with_comments() {
    let correct = CaseFragments {
        word: word("foo"),
        post_word_comments: vec![
            Newline(Some(String::from("#word_comment"))),
            Newline(Some(String::from("#post_word_a"))),
            Newline(None),
            Newline(Some(String::from("#post_word_b"))),
        ],
        in_comment: Some(Newline(Some(String::from("#in_comment")))),
        arms: vec![
            CaseArm {
                patterns: CasePatternFragments {
                    pre_pattern_comments: vec![
                        Newline(None),
                        Newline(Some(String::from("#pre_pat_a"))),
                    ],
                    pattern_alternatives: vec![word("hello"), word("goodbye")],
                    pattern_comment: Some(Newline(Some(String::from("#pat_a")))),
                },
                body: CommandGroup {
                    commands: vec![cmd_args("echo", &["greeting"])],
                    trailing_comments: vec![
                        Newline(None),
                        Newline(Some(String::from("#post_body_a"))),
                    ],
                },
                arm_comment: Some(Newline(Some(String::from("#arm_a")))),
            },
            CaseArm {
                patterns: CasePatternFragments {
                    pre_pattern_comments: vec![
                        Newline(None),
                        Newline(Some(String::from("#pre_pat_b"))),
                    ],
                    pattern_alternatives: vec![word("world")],
                    pattern_comment: Some(Newline(Some(String::from("#pat_b")))),
                },
                body: CommandGroup {
                    commands: vec![cmd_args("echo", &["noun"])],
                    trailing_comments: vec![],
                },
                arm_comment: Some(Newline(Some(String::from("#arm_b")))),
            },
        ],
        post_arms_comments: vec![Newline(None), Newline(Some(String::from("#post_arms")))],
    };

    // Various newlines and comments allowed within the command
    let cmd = "case foo #word_comment
        #post_word_a

        #post_word_b
        in #in_comment

        #pre_pat_a
        (hello | goodbye) #pat_a

        #cmd_leading
        echo greeting #within_body

        #post_body_a
        ;; #arm_a

        #pre_pat_b
        world) #pat_b

        #cmd_leading
        echo noun
        ;; #arm_b

        #post_arms
        esac";

    assert_eq!(Ok(correct), make_parser(cmd).case_command());
}

#[test]
fn test_case_command_valid_with_comments_no_body() {
    let correct = CaseFragments {
        word: word("foo"),
        post_word_comments: vec![
            Newline(Some(String::from("#word_comment"))),
            Newline(Some(String::from("#post_word_a"))),
            Newline(None),
            Newline(Some(String::from("#post_word_b"))),
        ],
        in_comment: Some(Newline(Some(String::from("#in_comment")))),
        arms: vec![],
        post_arms_comments: vec![Newline(None), Newline(Some(String::from("#post_arms")))],
    };

    // Various newlines and comments allowed within the command
    let cmd = "case foo #word_comment
        #post_word_a

        #post_word_b
        in #in_comment

        #post_arms
        esac #case_comment";

    assert_eq!(correct, make_parser(cmd).case_command().unwrap());
}

#[test]
fn test_case_command_word_need_not_be_simple_literal() {
    let mut p = make_parser("case 'foo'bar$$ in foo) echo foo;; esac");
    p.case_command().unwrap();
}

#[test]
fn test_case_command_valid_with_no_arms() {
    let mut p = make_parser("case foo in esac");
    p.case_command().unwrap();
}

#[test]
fn test_case_command_valid_branch_with_no_command() {
    let mut p = make_parser("case foo in pat)\nesac");
    p.case_command().unwrap();
    let mut p = make_parser("case foo in pat);;esac");
    p.case_command().unwrap();
}

#[test]
fn test_case_command_invalid_missing_keyword() {
    let mut p = make_parser("foo in foo) echo foo;; bar) echo bar;; esac");
    assert_eq!(
        Err(Unexpected(Token::Name(String::from("foo")), src(0, 1, 1))),
        p.case_command()
    );
    let mut p = make_parser("case foo foo) echo foo;; bar) echo bar;; esac");
    assert_eq!(
        Err(IncompleteCmd("case", src(0, 1, 1), "in", src(9, 1, 10))),
        p.case_command()
    );
    let mut p = make_parser("case foo in foo) echo foo;; bar) echo bar;;");
    assert_eq!(
        Err(IncompleteCmd("case", src(0, 1, 1), "esac", src(43, 1, 44))),
        p.case_command()
    );
}

#[test]
fn test_case_command_invalid_missing_word() {
    let mut p = make_parser("case in foo) echo foo;; bar) echo bar;; esac");
    assert_eq!(
        Err(IncompleteCmd("case", src(0, 1, 1), "in", src(8, 1, 9))),
        p.case_command()
    );
}

#[test]
fn test_case_command_invalid_quoted() {
    let cmds = [
        (
            "'case' foo in foo) echo foo;; bar) echo bar;; esac",
            Unexpected(Token::SingleQuote, src(0, 1, 1)),
        ),
        (
            "case foo 'in' foo) echo foo;; bar) echo bar;; esac",
            IncompleteCmd("case", src(0, 1, 1), "in", src(9, 1, 10)),
        ),
        (
            "case foo in foo) echo foo;; bar')' echo bar;; esac",
            Unexpected(Token::Name(String::from("echo")), src(35, 1, 36)),
        ),
        (
            "case foo in foo) echo foo;; bar) echo bar;; 'esac'",
            IncompleteCmd("case", src(0, 1, 1), "esac", src(50, 1, 51)),
        ),
        (
            "\"case\" foo in foo) echo foo;; bar) echo bar;; esac",
            Unexpected(Token::DoubleQuote, src(0, 1, 1)),
        ),
        (
            "case foo \"in\" foo) echo foo;; bar) echo bar;; esac",
            IncompleteCmd("case", src(0, 1, 1), "in", src(9, 1, 10)),
        ),
        (
            "case foo in foo) echo foo;; bar\")\" echo bar;; esac",
            Unexpected(Token::Name(String::from("echo")), src(35, 1, 36)),
        ),
        (
            "case foo in foo) echo foo;; bar) echo bar;; \"esac\"",
            IncompleteCmd("case", src(0, 1, 1), "esac", src(50, 1, 51)),
        ),
    ];

    for (c, e) in &cmds {
        match make_parser(c).case_command() {
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
fn test_case_command_invalid_newline_after_case() {
    let mut p = make_parser("case\nfoo in foo) echo foo;; bar) echo bar;; esac");
    assert_eq!(
        Err(Unexpected(Token::Newline, src(4, 1, 5))),
        p.case_command()
    );
}

#[test]
fn test_case_command_invalid_concat() {
    let mut p = make_parser_from_tokens(vec![
        Token::Literal(String::from("ca")),
        Token::Literal(String::from("se")),
        Token::Whitespace(String::from(" ")),
        Token::Literal(String::from("foo")),
        Token::Literal(String::from("bar")),
        Token::Whitespace(String::from(" ")),
        Token::Literal(String::from("in")),
        Token::Literal(String::from("foo")),
        Token::ParenClose,
        Token::Newline,
        Token::Literal(String::from("echo")),
        Token::Whitespace(String::from(" ")),
        Token::Literal(String::from("foo")),
        Token::Newline,
        Token::Newline,
        Token::DSemi,
        Token::Literal(String::from("esac")),
    ]);
    assert_eq!(
        Err(Unexpected(Token::Literal(String::from("ca")), src(0, 1, 1))),
        p.case_command()
    );

    let mut p = make_parser_from_tokens(vec![
        Token::Literal(String::from("case")),
        Token::Whitespace(String::from(" ")),
        Token::Literal(String::from("foo")),
        Token::Literal(String::from("bar")),
        Token::Whitespace(String::from(" ")),
        Token::Literal(String::from("i")),
        Token::Literal(String::from("n")),
        Token::Literal(String::from("foo")),
        Token::ParenClose,
        Token::Newline,
        Token::Literal(String::from("echo")),
        Token::Whitespace(String::from(" ")),
        Token::Literal(String::from("foo")),
        Token::Newline,
        Token::Newline,
        Token::DSemi,
        Token::Literal(String::from("esac")),
    ]);
    assert_eq!(
        Err(IncompleteCmd("case", src(0, 1, 1), "in", src(12, 1, 13))),
        p.case_command()
    );

    let mut p = make_parser_from_tokens(vec![
        Token::Literal(String::from("case")),
        Token::Whitespace(String::from(" ")),
        Token::Literal(String::from("foo")),
        Token::Literal(String::from("bar")),
        Token::Whitespace(String::from(" ")),
        Token::Literal(String::from("in")),
        Token::Whitespace(String::from(" ")),
        Token::Literal(String::from("foo")),
        Token::ParenClose,
        Token::Newline,
        Token::Literal(String::from("echo")),
        Token::Whitespace(String::from(" ")),
        Token::Literal(String::from("foo")),
        Token::Newline,
        Token::Newline,
        Token::DSemi,
        Token::Literal(String::from("es")),
        Token::Literal(String::from("ac")),
    ]);
    assert_eq!(
        Err(IncompleteCmd("case", src(0, 1, 1), "esac", src(36, 4, 7))),
        p.case_command()
    );
}

#[test]
fn test_case_command_should_recognize_literals_and_names() {
    let case_str = String::from("case");
    let in_str = String::from("in");
    let esac_str = String::from("esac");
    for case_tok in vec![Token::Literal(case_str.clone()), Token::Name(case_str)] {
        for in_tok in vec![Token::Literal(in_str.clone()), Token::Name(in_str.clone())] {
            for esac_tok in vec![
                Token::Literal(esac_str.clone()),
                Token::Name(esac_str.clone()),
            ] {
                let mut p = make_parser_from_tokens(vec![
                    case_tok.clone(),
                    Token::Whitespace(String::from(" ")),
                    Token::Literal(String::from("foo")),
                    Token::Literal(String::from("bar")),
                    Token::Whitespace(String::from(" ")),
                    in_tok.clone(),
                    Token::Whitespace(String::from(" ")),
                    Token::Literal(String::from("foo")),
                    Token::ParenClose,
                    Token::Newline,
                    Token::Literal(String::from("echo")),
                    Token::Whitespace(String::from(" ")),
                    Token::Literal(String::from("foo")),
                    Token::Newline,
                    Token::Newline,
                    Token::DSemi,
                    esac_tok,
                ]);
                p.case_command().unwrap();
            }
        }
    }
}
