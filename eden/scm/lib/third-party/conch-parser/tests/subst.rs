#![deny(rust_2018_idioms)]
use conch_parser::ast::ComplexWord::*;
use conch_parser::ast::Parameter::*;
use conch_parser::ast::ParameterSubstitution::*;
use conch_parser::ast::{RedirectOrCmdWord, SimpleCommand, SimpleWord, TopLevelWord, Word};
use conch_parser::parse::ParseError::*;
use conch_parser::token::Token;

mod parse_support;
use crate::parse_support::*;

#[test]
fn test_parameter_substitution() {
    let words = vec![
        Len(At),
        Len(Star),
        Len(Pound),
        Len(Question),
        Len(Dash),
        Len(Dollar),
        Len(Bang),
        Len(Var(String::from("foo"))),
        Len(Positional(3)),
        Len(Positional(1000)),
        Command(vec![cmd_args("echo", &["foo"])]),
    ];

    let mut p = make_parser("${#@}${#*}${##}${#?}${#-}${#$}${#!}${#foo}${#3}${#1000}$(echo foo)");
    for param in words {
        let correct = word_subst(param);
        assert_eq!(correct, p.parameter().unwrap());
    }

    assert_eq!(Err(UnexpectedEOF), p.parameter()); // Stream should be exhausted
}

#[test]
fn test_parameter_substitution_smallest_suffix() {
    let word = word("foo");

    let substs = vec![
        RemoveSmallestSuffix(At, Some(word.clone())),
        RemoveSmallestSuffix(Star, Some(word.clone())),
        RemoveSmallestSuffix(Pound, Some(word.clone())),
        RemoveSmallestSuffix(Question, Some(word.clone())),
        RemoveSmallestSuffix(Dash, Some(word.clone())),
        RemoveSmallestSuffix(Dollar, Some(word.clone())),
        RemoveSmallestSuffix(Bang, Some(word.clone())),
        RemoveSmallestSuffix(Positional(0), Some(word.clone())),
        RemoveSmallestSuffix(Positional(10), Some(word.clone())),
        RemoveSmallestSuffix(Positional(100), Some(word.clone())),
        RemoveSmallestSuffix(Var(String::from("foo_bar123")), Some(word)),
        RemoveSmallestSuffix(At, None),
        RemoveSmallestSuffix(Star, None),
        RemoveSmallestSuffix(Pound, None),
        RemoveSmallestSuffix(Question, None),
        RemoveSmallestSuffix(Dash, None),
        RemoveSmallestSuffix(Dollar, None),
        RemoveSmallestSuffix(Bang, None),
        RemoveSmallestSuffix(Positional(0), None),
        RemoveSmallestSuffix(Positional(10), None),
        RemoveSmallestSuffix(Positional(100), None),
        RemoveSmallestSuffix(Var(String::from("foo_bar123")), None),
    ];

    let src = "${@%foo}${*%foo}${#%foo}${?%foo}${-%foo}${$%foo}${!%foo}${0%foo}${10%foo}${100%foo}${foo_bar123%foo}${@%}${*%}${#%}${?%}${-%}${$%}${!%}${0%}${10%}${100%}${foo_bar123%}";
    let mut p = make_parser(src);

    for s in substs {
        assert_eq!(word_subst(s), p.parameter().unwrap());
    }

    assert_eq!(Err(UnexpectedEOF), p.parameter()); // Stream should be exhausted
}

#[test]
fn test_parameter_substitution_largest_suffix() {
    let word = word("foo");

    let substs = vec![
        RemoveLargestSuffix(At, Some(word.clone())),
        RemoveLargestSuffix(Star, Some(word.clone())),
        RemoveLargestSuffix(Pound, Some(word.clone())),
        RemoveLargestSuffix(Question, Some(word.clone())),
        RemoveLargestSuffix(Dash, Some(word.clone())),
        RemoveLargestSuffix(Dollar, Some(word.clone())),
        RemoveLargestSuffix(Bang, Some(word.clone())),
        RemoveLargestSuffix(Positional(0), Some(word.clone())),
        RemoveLargestSuffix(Positional(10), Some(word.clone())),
        RemoveLargestSuffix(Positional(100), Some(word.clone())),
        RemoveLargestSuffix(Var(String::from("foo_bar123")), Some(word)),
        RemoveLargestSuffix(At, None),
        RemoveLargestSuffix(Star, None),
        RemoveLargestSuffix(Pound, None),
        RemoveLargestSuffix(Question, None),
        RemoveLargestSuffix(Dash, None),
        RemoveLargestSuffix(Dollar, None),
        RemoveLargestSuffix(Bang, None),
        RemoveLargestSuffix(Positional(0), None),
        RemoveLargestSuffix(Positional(10), None),
        RemoveLargestSuffix(Positional(100), None),
        RemoveLargestSuffix(Var(String::from("foo_bar123")), None),
    ];

    let src = "${@%%foo}${*%%foo}${#%%foo}${?%%foo}${-%%foo}${$%%foo}${!%%foo}${0%%foo}${10%%foo}${100%%foo}${foo_bar123%%foo}${@%%}${*%%}${#%%}${?%%}${-%%}${$%%}${!%%}${0%%}${10%%}${100%%}${foo_bar123%%}";
    let mut p = make_parser(src);

    for s in substs {
        assert_eq!(word_subst(s), p.parameter().unwrap());
    }

    assert_eq!(Err(UnexpectedEOF), p.parameter()); // Stream should be exhausted
}

#[test]
fn test_parameter_substitution_smallest_prefix() {
    let word = word("foo");

    let substs = vec![
        RemoveSmallestPrefix(At, Some(word.clone())),
        RemoveSmallestPrefix(Star, Some(word.clone())),
        RemoveSmallestPrefix(Pound, Some(word.clone())),
        RemoveSmallestPrefix(Question, Some(word.clone())),
        RemoveSmallestPrefix(Dash, Some(word.clone())),
        RemoveSmallestPrefix(Dollar, Some(word.clone())),
        RemoveSmallestPrefix(Bang, Some(word.clone())),
        RemoveSmallestPrefix(Positional(0), Some(word.clone())),
        RemoveSmallestPrefix(Positional(10), Some(word.clone())),
        RemoveSmallestPrefix(Positional(100), Some(word.clone())),
        RemoveSmallestPrefix(Var(String::from("foo_bar123")), Some(word)),
        RemoveSmallestPrefix(At, None),
        RemoveSmallestPrefix(Star, None),
        //RemoveSmallestPrefix(Pound, None), // ${##} should parse as Len(#)
        RemoveSmallestPrefix(Question, None),
        RemoveSmallestPrefix(Dash, None),
        RemoveSmallestPrefix(Dollar, None),
        RemoveSmallestPrefix(Bang, None),
        RemoveSmallestPrefix(Positional(0), None),
        RemoveSmallestPrefix(Positional(10), None),
        RemoveSmallestPrefix(Positional(100), None),
        RemoveSmallestPrefix(Var(String::from("foo_bar123")), None),
    ];

    let src = "${@#foo}${*#foo}${##foo}${?#foo}${-#foo}${$#foo}${!#foo}${0#foo}${10#foo}${100#foo}${foo_bar123#foo}${@#}${*#}${?#}${-#}${$#}${!#}${0#}${10#}${100#}${foo_bar123#}";
    let mut p = make_parser(src);

    for s in substs {
        assert_eq!(word_subst(s), p.parameter().unwrap());
    }

    assert_eq!(Err(UnexpectedEOF), p.parameter()); // Stream should be exhausted
}

#[test]
fn test_parameter_substitution_largest_prefix() {
    let word = word("foo");

    let substs = vec![
        RemoveLargestPrefix(At, Some(word.clone())),
        RemoveLargestPrefix(Star, Some(word.clone())),
        RemoveLargestPrefix(Pound, Some(word.clone())),
        RemoveLargestPrefix(Question, Some(word.clone())),
        RemoveLargestPrefix(Dash, Some(word.clone())),
        RemoveLargestPrefix(Dollar, Some(word.clone())),
        RemoveLargestPrefix(Bang, Some(word.clone())),
        RemoveLargestPrefix(Positional(0), Some(word.clone())),
        RemoveLargestPrefix(Positional(10), Some(word.clone())),
        RemoveLargestPrefix(Positional(100), Some(word.clone())),
        RemoveLargestPrefix(Var(String::from("foo_bar123")), Some(word)),
        RemoveLargestPrefix(At, None),
        RemoveLargestPrefix(Star, None),
        RemoveLargestPrefix(Pound, None),
        RemoveLargestPrefix(Question, None),
        RemoveLargestPrefix(Dash, None),
        RemoveLargestPrefix(Dollar, None),
        RemoveLargestPrefix(Bang, None),
        RemoveLargestPrefix(Positional(0), None),
        RemoveLargestPrefix(Positional(10), None),
        RemoveLargestPrefix(Positional(100), None),
        RemoveLargestPrefix(Var(String::from("foo_bar123")), None),
    ];

    let src = "${@##foo}${*##foo}${###foo}${?##foo}${-##foo}${$##foo}${!##foo}${0##foo}${10##foo}${100##foo}${foo_bar123##foo}${@##}${*##}${###}${?##}${-##}${$##}${!##}${0##}${10##}${100##}${foo_bar123##}";
    let mut p = make_parser(src);

    for s in substs {
        assert_eq!(word_subst(s), p.parameter().unwrap());
    }

    assert_eq!(Err(UnexpectedEOF), p.parameter()); // Stream should be exhausted
}

#[test]
fn test_parameter_substitution_default() {
    let word = word("foo");

    let substs = vec![
        Default(true, At, Some(word.clone())),
        Default(true, Star, Some(word.clone())),
        Default(true, Pound, Some(word.clone())),
        Default(true, Question, Some(word.clone())),
        Default(true, Dash, Some(word.clone())),
        Default(true, Dollar, Some(word.clone())),
        Default(true, Bang, Some(word.clone())),
        Default(true, Positional(0), Some(word.clone())),
        Default(true, Positional(10), Some(word.clone())),
        Default(true, Positional(100), Some(word.clone())),
        Default(true, Var(String::from("foo_bar123")), Some(word.clone())),
        Default(true, At, None),
        Default(true, Star, None),
        Default(true, Pound, None),
        Default(true, Question, None),
        Default(true, Dash, None),
        Default(true, Dollar, None),
        Default(true, Bang, None),
        Default(true, Positional(0), None),
        Default(true, Positional(10), None),
        Default(true, Positional(100), None),
        Default(true, Var(String::from("foo_bar123")), None),
    ];

    let src = "${@:-foo}${*:-foo}${#:-foo}${?:-foo}${-:-foo}${$:-foo}${!:-foo}${0:-foo}${10:-foo}${100:-foo}${foo_bar123:-foo}${@:-}${*:-}${#:-}${?:-}${-:-}${$:-}${!:-}${0:-}${10:-}${100:-}${foo_bar123:-}";
    let mut p = make_parser(src);
    for s in substs {
        assert_eq!(word_subst(s), p.parameter().unwrap());
    }
    assert_eq!(Err(UnexpectedEOF), p.parameter()); // Stream should be exhausted

    let substs = vec![
        Default(false, At, Some(word.clone())),
        Default(false, Star, Some(word.clone())),
        Default(false, Pound, Some(word.clone())),
        Default(false, Question, Some(word.clone())),
        Default(false, Dash, Some(word.clone())),
        Default(false, Dollar, Some(word.clone())),
        Default(false, Bang, Some(word.clone())),
        Default(false, Positional(0), Some(word.clone())),
        Default(false, Positional(10), Some(word.clone())),
        Default(false, Positional(100), Some(word.clone())),
        Default(false, Var(String::from("foo_bar123")), Some(word)),
        Default(false, At, None),
        Default(false, Star, None),
        //Default(false, Pound, None), // ${#-} should be a length check of the `-` parameter
        Default(false, Question, None),
        Default(false, Dash, None),
        Default(false, Dollar, None),
        Default(false, Bang, None),
        Default(false, Positional(0), None),
        Default(false, Positional(10), None),
        Default(false, Positional(100), None),
        Default(false, Var(String::from("foo_bar123")), None),
    ];

    let src = "${@-foo}${*-foo}${#-foo}${?-foo}${--foo}${$-foo}${!-foo}${0-foo}${10-foo}${100-foo}${foo_bar123-foo}${@-}${*-}${?-}${--}${$-}${!-}${0-}${10-}${100-}${foo_bar123-}";
    let mut p = make_parser(src);
    for s in substs {
        assert_eq!(word_subst(s), p.parameter().unwrap());
    }
    assert_eq!(Err(UnexpectedEOF), p.parameter()); // Stream should be exhausted
}

#[test]
fn test_parameter_substitution_error() {
    let word = word("foo");

    let substs = vec![
        Error(true, At, Some(word.clone())),
        Error(true, Star, Some(word.clone())),
        Error(true, Pound, Some(word.clone())),
        Error(true, Question, Some(word.clone())),
        Error(true, Dash, Some(word.clone())),
        Error(true, Dollar, Some(word.clone())),
        Error(true, Bang, Some(word.clone())),
        Error(true, Positional(0), Some(word.clone())),
        Error(true, Positional(10), Some(word.clone())),
        Error(true, Positional(100), Some(word.clone())),
        Error(true, Var(String::from("foo_bar123")), Some(word.clone())),
        Error(true, At, None),
        Error(true, Star, None),
        Error(true, Pound, None),
        Error(true, Question, None),
        Error(true, Dash, None),
        Error(true, Dollar, None),
        Error(true, Bang, None),
        Error(true, Positional(0), None),
        Error(true, Positional(10), None),
        Error(true, Positional(100), None),
        Error(true, Var(String::from("foo_bar123")), None),
    ];

    let src = "${@:?foo}${*:?foo}${#:?foo}${?:?foo}${-:?foo}${$:?foo}${!:?foo}${0:?foo}${10:?foo}${100:?foo}${foo_bar123:?foo}${@:?}${*:?}${#:?}${?:?}${-:?}${$:?}${!:?}${0:?}${10:?}${100:?}${foo_bar123:?}";
    let mut p = make_parser(src);
    for s in substs {
        assert_eq!(word_subst(s), p.parameter().unwrap());
    }
    assert_eq!(Err(UnexpectedEOF), p.parameter()); // Stream should be exhausted

    let substs = vec![
        Error(false, At, Some(word.clone())),
        Error(false, Star, Some(word.clone())),
        Error(false, Pound, Some(word.clone())),
        Error(false, Question, Some(word.clone())),
        Error(false, Dash, Some(word.clone())),
        Error(false, Dollar, Some(word.clone())),
        Error(false, Bang, Some(word.clone())),
        Error(false, Positional(0), Some(word.clone())),
        Error(false, Positional(10), Some(word.clone())),
        Error(false, Positional(100), Some(word.clone())),
        Error(false, Var(String::from("foo_bar123")), Some(word)),
        Error(false, At, None),
        Error(false, Star, None),
        //Error(false, Pound, None), // ${#?} should be a length check of the `?` parameter
        Error(false, Question, None),
        Error(false, Dash, None),
        Error(false, Dollar, None),
        Error(false, Bang, None),
        Error(false, Positional(0), None),
        Error(false, Positional(10), None),
        Error(false, Positional(100), None),
        Error(false, Var(String::from("foo_bar123")), None),
    ];

    let src = "${@?foo}${*?foo}${#?foo}${??foo}${-?foo}${$?foo}${!?foo}${0?foo}${10?foo}${100?foo}${foo_bar123?foo}${@?}${*?}${??}${-?}${$?}${!?}${0?}${10?}${100?}${foo_bar123?}";
    let mut p = make_parser(src);
    for s in substs {
        assert_eq!(word_subst(s), p.parameter().unwrap());
    }
    assert_eq!(Err(UnexpectedEOF), p.parameter()); // Stream should be exhausted
}

#[test]
fn test_parameter_substitution_assign() {
    let word = word("foo");

    let substs = vec![
        Assign(true, At, Some(word.clone())),
        Assign(true, Star, Some(word.clone())),
        Assign(true, Pound, Some(word.clone())),
        Assign(true, Question, Some(word.clone())),
        Assign(true, Dash, Some(word.clone())),
        Assign(true, Dollar, Some(word.clone())),
        Assign(true, Bang, Some(word.clone())),
        Assign(true, Positional(0), Some(word.clone())),
        Assign(true, Positional(10), Some(word.clone())),
        Assign(true, Positional(100), Some(word.clone())),
        Assign(true, Var(String::from("foo_bar123")), Some(word.clone())),
        Assign(true, At, None),
        Assign(true, Star, None),
        Assign(true, Pound, None),
        Assign(true, Question, None),
        Assign(true, Dash, None),
        Assign(true, Dollar, None),
        Assign(true, Bang, None),
        Assign(true, Positional(0), None),
        Assign(true, Positional(10), None),
        Assign(true, Positional(100), None),
        Assign(true, Var(String::from("foo_bar123")), None),
    ];

    let src = "${@:=foo}${*:=foo}${#:=foo}${?:=foo}${-:=foo}${$:=foo}${!:=foo}${0:=foo}${10:=foo}${100:=foo}${foo_bar123:=foo}${@:=}${*:=}${#:=}${?:=}${-:=}${$:=}${!:=}${0:=}${10:=}${100:=}${foo_bar123:=}";
    let mut p = make_parser(src);
    for s in substs {
        assert_eq!(word_subst(s), p.parameter().unwrap());
    }
    assert_eq!(Err(UnexpectedEOF), p.parameter()); // Stream should be exhausted

    let substs = vec![
        Assign(false, At, Some(word.clone())),
        Assign(false, Star, Some(word.clone())),
        Assign(false, Pound, Some(word.clone())),
        Assign(false, Question, Some(word.clone())),
        Assign(false, Dash, Some(word.clone())),
        Assign(false, Dollar, Some(word.clone())),
        Assign(false, Bang, Some(word.clone())),
        Assign(false, Positional(0), Some(word.clone())),
        Assign(false, Positional(10), Some(word.clone())),
        Assign(false, Positional(100), Some(word.clone())),
        Assign(false, Var(String::from("foo_bar123")), Some(word)),
        Assign(false, At, None),
        Assign(false, Star, None),
        Assign(false, Pound, None),
        Assign(false, Question, None),
        Assign(false, Dash, None),
        Assign(false, Dollar, None),
        Assign(false, Bang, None),
        Assign(false, Positional(0), None),
        Assign(false, Positional(10), None),
        Assign(false, Positional(100), None),
        Assign(false, Var(String::from("foo_bar123")), None),
    ];

    let src = "${@=foo}${*=foo}${#=foo}${?=foo}${-=foo}${$=foo}${!=foo}${0=foo}${10=foo}${100=foo}${foo_bar123=foo}${@=}${*=}${#=}${?=}${-=}${$=}${!=}${0=}${10=}${100=}${foo_bar123=}";
    let mut p = make_parser(src);
    for s in substs {
        assert_eq!(word_subst(s), p.parameter().unwrap());
    }
    assert_eq!(Err(UnexpectedEOF), p.parameter()); // Stream should be exhausted
}

#[test]
fn test_parameter_substitution_alternative() {
    let word = word("foo");

    let substs = vec![
        Alternative(true, At, Some(word.clone())),
        Alternative(true, Star, Some(word.clone())),
        Alternative(true, Pound, Some(word.clone())),
        Alternative(true, Question, Some(word.clone())),
        Alternative(true, Dash, Some(word.clone())),
        Alternative(true, Dollar, Some(word.clone())),
        Alternative(true, Bang, Some(word.clone())),
        Alternative(true, Positional(0), Some(word.clone())),
        Alternative(true, Positional(10), Some(word.clone())),
        Alternative(true, Positional(100), Some(word.clone())),
        Alternative(true, Var(String::from("foo_bar123")), Some(word.clone())),
        Alternative(true, At, None),
        Alternative(true, Star, None),
        Alternative(true, Pound, None),
        Alternative(true, Question, None),
        Alternative(true, Dash, None),
        Alternative(true, Dollar, None),
        Alternative(true, Bang, None),
        Alternative(true, Positional(0), None),
        Alternative(true, Positional(10), None),
        Alternative(true, Positional(100), None),
        Alternative(true, Var(String::from("foo_bar123")), None),
    ];

    let src = "${@:+foo}${*:+foo}${#:+foo}${?:+foo}${-:+foo}${$:+foo}${!:+foo}${0:+foo}${10:+foo}${100:+foo}${foo_bar123:+foo}${@:+}${*:+}${#:+}${?:+}${-:+}${$:+}${!:+}${0:+}${10:+}${100:+}${foo_bar123:+}";
    let mut p = make_parser(src);
    for s in substs {
        assert_eq!(word_subst(s), p.parameter().unwrap());
    }
    assert_eq!(Err(UnexpectedEOF), p.parameter()); // Stream should be exhausted

    let substs = vec![
        Alternative(false, At, Some(word.clone())),
        Alternative(false, Star, Some(word.clone())),
        Alternative(false, Pound, Some(word.clone())),
        Alternative(false, Question, Some(word.clone())),
        Alternative(false, Dash, Some(word.clone())),
        Alternative(false, Dollar, Some(word.clone())),
        Alternative(false, Bang, Some(word.clone())),
        Alternative(false, Positional(0), Some(word.clone())),
        Alternative(false, Positional(10), Some(word.clone())),
        Alternative(false, Positional(100), Some(word.clone())),
        Alternative(false, Var(String::from("foo_bar123")), Some(word)),
        Alternative(false, At, None),
        Alternative(false, Star, None),
        Alternative(false, Pound, None),
        Alternative(false, Question, None),
        Alternative(false, Dash, None),
        Alternative(false, Dollar, None),
        Alternative(false, Bang, None),
        Alternative(false, Positional(0), None),
        Alternative(false, Positional(10), None),
        Alternative(false, Positional(100), None),
        Alternative(false, Var(String::from("foo_bar123")), None),
    ];

    let src = "${@+foo}${*+foo}${#+foo}${?+foo}${-+foo}${$+foo}${!+foo}${0+foo}${10+foo}${100+foo}${foo_bar123+foo}${@+}${*+}${#+}${?+}${-+}${$+}${!+}${0+}${10+}${100+}${foo_bar123+}";
    let mut p = make_parser(src);
    for s in substs {
        assert_eq!(word_subst(s), p.parameter().unwrap());
    }
    assert_eq!(Err(UnexpectedEOF), p.parameter()); // Stream should be exhausted
}

#[test]
fn test_parameter_substitution_words_can_have_spaces_and_escaped_curlies() {
    let var = Var(String::from("foo_bar123"));
    let word = TopLevelWord(Concat(vec![
        lit("foo{"),
        escaped("}"),
        lit(" \t\r "),
        escaped("\n"),
        lit("bar \t\r "),
    ]));

    let substs = vec![
        RemoveSmallestSuffix(var.clone(), Some(word.clone())),
        RemoveLargestSuffix(var.clone(), Some(word.clone())),
        RemoveSmallestPrefix(var.clone(), Some(word.clone())),
        RemoveLargestPrefix(var.clone(), Some(word.clone())),
        Default(true, var.clone(), Some(word.clone())),
        Default(false, var.clone(), Some(word.clone())),
        Assign(true, var.clone(), Some(word.clone())),
        Assign(false, var.clone(), Some(word.clone())),
        Error(true, var.clone(), Some(word.clone())),
        Error(false, var.clone(), Some(word.clone())),
        Alternative(true, var.clone(), Some(word.clone())),
        Alternative(false, var, Some(word)),
    ];

    let src = vec![
        "%", "%%", "#", "##", ":-", "-", ":=", "=", ":?", "?", ":+", "+",
    ];

    for (i, s) in substs.into_iter().enumerate() {
        let src = format!("${{foo_bar123{}foo{{\\}} \t\r \\\nbar \t\r }}", src[i]);
        let mut p = make_parser(&src);
        assert_eq!(word_subst(s), p.parameter().unwrap());
        assert_eq!(Err(UnexpectedEOF), p.parameter()); // Stream should be exhausted
    }
}

#[test]
fn test_parameter_substitution_words_can_start_with_pound() {
    let var = Var(String::from("foo_bar123"));
    let word = TopLevelWord(Concat(vec![
        lit("#foo{"),
        escaped("}"),
        lit(" \t\r "),
        escaped("\n"),
        lit("bar \t\r "),
    ]));

    let substs = vec![
        RemoveSmallestSuffix(var.clone(), Some(word.clone())),
        RemoveLargestSuffix(var.clone(), Some(word.clone())),
        //RemoveSmallestPrefix(var.clone(), Some(word.clone())),
        RemoveLargestPrefix(var.clone(), Some(word.clone())),
        Default(true, var.clone(), Some(word.clone())),
        Default(false, var.clone(), Some(word.clone())),
        Assign(true, var.clone(), Some(word.clone())),
        Assign(false, var.clone(), Some(word.clone())),
        Error(true, var.clone(), Some(word.clone())),
        Error(false, var.clone(), Some(word.clone())),
        Alternative(true, var.clone(), Some(word.clone())),
        Alternative(false, var, Some(word)),
    ];

    let src = vec![
        "%", "%%", //"#", // Let's not confuse the pound in the word with a substitution
        "##", ":-", "-", ":=", "=", ":?", "?", ":+", "+",
    ];

    for (i, s) in substs.into_iter().enumerate() {
        let src = format!("${{foo_bar123{}#foo{{\\}} \t\r \\\nbar \t\r }}", src[i]);
        let mut p = make_parser(&src);
        assert_eq!(word_subst(s), p.parameter().unwrap());
        assert_eq!(Err(UnexpectedEOF), p.parameter()); // Stream should be exhausted
    }
}

#[test]
fn test_parameter_substitution_words_can_be_parameters_or_substitutions_as_well() {
    let var = Var(String::from("foo_bar123"));
    let word = TopLevelWord(Concat(vec![
        Word::Simple(SimpleWord::Param(At)),
        subst(RemoveLargestPrefix(
            Var(String::from("foo")),
            Some(word("bar")),
        )),
    ]));

    let substs = vec![
        RemoveSmallestSuffix(var.clone(), Some(word.clone())),
        RemoveLargestSuffix(var.clone(), Some(word.clone())),
        RemoveSmallestPrefix(var.clone(), Some(word.clone())),
        RemoveLargestPrefix(var.clone(), Some(word.clone())),
        Default(true, var.clone(), Some(word.clone())),
        Default(false, var.clone(), Some(word.clone())),
        Assign(true, var.clone(), Some(word.clone())),
        Assign(false, var.clone(), Some(word.clone())),
        Error(true, var.clone(), Some(word.clone())),
        Error(false, var.clone(), Some(word.clone())),
        Alternative(true, var.clone(), Some(word.clone())),
        Alternative(false, var, Some(word)),
    ];

    let src = vec![
        "%", "%%", "#", "##", ":-", "-", ":=", "=", ":?", "?", ":+", "+",
    ];

    for (i, s) in substs.into_iter().enumerate() {
        let src = format!("${{foo_bar123{}$@${{foo##bar}}}}", src[i]);
        let mut p = make_parser(&src);
        assert_eq!(word_subst(s), p.parameter().unwrap());
        assert_eq!(Err(UnexpectedEOF), p.parameter()); // Stream should be exhausted
    }
}

#[test]
fn test_parameter_substitution_command_close_paren_need_not_be_followed_by_word_delimeter() {
    let correct = Some(cmd_from_simple(SimpleCommand {
        redirects_or_env_vars: vec![],
        redirects_or_cmd_words: vec![
            RedirectOrCmdWord::CmdWord(word("foo")),
            RedirectOrCmdWord::CmdWord(TopLevelWord(Single(Word::DoubleQuoted(vec![
                SimpleWord::Subst(Box::new(Command(vec![cmd("bar")]))),
            ])))),
        ],
    }));

    assert_eq!(
        correct,
        make_parser("foo \"$(bar)\"").complete_command().unwrap()
    );
}

#[test]
fn test_parameter_substitution_invalid() {
    let cases = vec![
        ("$(( x", UnexpectedEOF),
        ("${foo", Unmatched(Token::CurlyOpen, src(1, 1, 2))),
        (
            "${ foo}",
            BadSubst(Token::Whitespace(String::from(" ")), src(2, 1, 3)),
        ),
        (
            "${foo }",
            BadSubst(Token::Whitespace(String::from(" ")), src(5, 1, 6)),
        ),
        (
            "${foo -}",
            BadSubst(Token::Whitespace(String::from(" ")), src(5, 1, 6)),
        ),
        (
            "${foo =}",
            BadSubst(Token::Whitespace(String::from(" ")), src(5, 1, 6)),
        ),
        (
            "${foo ?}",
            BadSubst(Token::Whitespace(String::from(" ")), src(5, 1, 6)),
        ),
        (
            "${foo +}",
            BadSubst(Token::Whitespace(String::from(" ")), src(5, 1, 6)),
        ),
        (
            "${foo :-}",
            BadSubst(Token::Whitespace(String::from(" ")), src(5, 1, 6)),
        ),
        (
            "${foo :=}",
            BadSubst(Token::Whitespace(String::from(" ")), src(5, 1, 6)),
        ),
        (
            "${foo :?}",
            BadSubst(Token::Whitespace(String::from(" ")), src(5, 1, 6)),
        ),
        (
            "${foo :+}",
            BadSubst(Token::Whitespace(String::from(" ")), src(5, 1, 6)),
        ),
        (
            "${foo: -}",
            BadSubst(Token::Whitespace(String::from(" ")), src(6, 1, 7)),
        ),
        (
            "${foo: =}",
            BadSubst(Token::Whitespace(String::from(" ")), src(6, 1, 7)),
        ),
        (
            "${foo: ?}",
            BadSubst(Token::Whitespace(String::from(" ")), src(6, 1, 7)),
        ),
        (
            "${foo: +}",
            BadSubst(Token::Whitespace(String::from(" ")), src(6, 1, 7)),
        ),
        (
            "${foo: %}",
            BadSubst(Token::Whitespace(String::from(" ")), src(6, 1, 7)),
        ),
        (
            "${foo: #}",
            BadSubst(Token::Whitespace(String::from(" ")), src(6, 1, 7)),
        ),
        ("${foo-bar", Unmatched(Token::CurlyOpen, src(1, 1, 2))),
        ("${'foo'}", BadSubst(Token::SingleQuote, src(2, 1, 3))),
        ("${\"foo\"}", BadSubst(Token::DoubleQuote, src(2, 1, 3))),
        ("${`foo`}", BadSubst(Token::Backtick, src(2, 1, 3))),
    ];

    for (s, correct) in cases.into_iter() {
        match make_parser(s).parameter() {
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
fn test_parameter_substitution_nested_quoted() {
    let param = Var("foo".to_owned());
    let cases = vec![
        (
            "${foo:+'bar'}",
            Alternative(true, param.clone(), Some(single_quoted("bar"))),
        ),
        (
            "${foo:+\"bar\"}",
            Alternative(true, param.clone(), Some(double_quoted("bar"))),
        ),
        (
            "${foo:+`bar`}",
            Alternative(true, param, Some(word_subst(Command(vec![cmd("bar")])))),
        ),
    ];

    for (src, subst) in cases.into_iter() {
        let correct = word_subst(subst);
        let parsed = make_parser(src).parameter();
        if parsed.as_ref() != Ok(&correct) {
            panic!(
                "Expected \"{}\" to parse as `{:?}`, but got `{:?}`",
                src, correct, parsed
            );
        }
    }
}

#[test]
fn test_parameter_substitution_can_have_nested_substitution_and_parameter() {
    let param_foo = Var("foo".to_owned());
    let param_bar = Var("bar".to_owned());
    let correct = word_subst(Alternative(
        true,
        param_foo,
        Some(word_subst(Alternative(
            true,
            param_bar,
            Some(word_param(Dollar)),
        ))),
    ));

    let mut p = make_parser("${foo:+${bar:+$$}}");
    assert_eq!(Ok(correct), p.parameter());
}

#[test]
fn test_parameter_substitution_special_tokens_in_words_become_literals() {
    let correct = word_subst(Default(
        true,
        Var("foo".to_owned()),
        Some(TopLevelWord(Concat(vec![
            lit("#(bar);&|&&||;; << >> <& >& <<- "),
            escaped("\n"),
            lit("\n\t"),
        ]))),
    ));

    let mut p = make_parser("${foo:-#(bar);&|&&||;; << >> <& >& <<- \\\n\n\t}");
    assert_eq!(Ok(correct), p.parameter());
}
