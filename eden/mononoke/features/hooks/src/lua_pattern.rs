/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

enum TranslationOutput {
    Literal(String),
    LuaSpecial(String),
    CharClass(String),
}

fn lua_special_to_regex(s: &str) -> String {
    // see https://www.lua.org/pil/20.2.html
    // NOTE: If you add to this, you need to cross-check pattern_to_regex to ensure it
    // correctly records a special, not a literal
    match s {
        "." => ".".to_string(),
        "%a" => "[[:alpha:]]".to_string(),
        "%A" => "[[:^alpha:]]".to_string(),
        "%c" => "[[:cntrl:]]".to_string(),
        "%C" => "[[:^cntrl:]]".to_string(),
        "%d" => "[[:digit:]]".to_string(),
        "%D" => "[[:^digit:]]".to_string(),
        "%l" => "[[:lower:]]".to_string(),
        "%L" => "[[:^lower:]]".to_string(),
        "%p" => "[[:punct:]]".to_string(),
        "%P" => "[[:^punct:]]".to_string(),
        "%s" => "[[:space:]]".to_string(),
        "%S" => "[[:^space:]]".to_string(),
        "%u" => "[[:upper:]]".to_string(),
        "%U" => "[[:^upper:]]".to_string(),
        "%w" => "[[:word:]]".to_string(),
        "%W" => "[[:^word:]]".to_string(),
        "%x" => "[[:xdigit:]]".to_string(),
        "%X" => "[[:^xdigit:]]".to_string(),
        "%z" => "\\0".to_string(),
        "(" => "(".to_string(),
        ")" => ")".to_string(),
        "+" => "+".to_string(),
        "*" => "*".to_string(),
        "-" => "*?".to_string(),
        "?" => "?".to_string(),
        "^" => "^".to_string(),
        "$" => "$".to_string(),
        _ if s.starts_with('%') && s.len() > 1 => regex::escape(&s[1..]),
        _ => panic!("Untranslated pattern component"),
    }
}

#[derive(Clone, Copy)]
enum ParseState {
    Normal,
    LastPercent,
    CharClass,
    CharClassLiteral,
}

impl ParseState {
    fn is_char_class(&self) -> bool {
        use ParseState::*;
        match self {
            Normal => false,
            LastPercent => false,
            CharClass => true,
            CharClassLiteral => true,
        }
    }
}

fn pattern_to_regex(other: &str) -> String {
    let mut output = Vec::new();
    let mut current_acc = String::new();
    let mut state = ParseState::Normal;

    fn flush_acc(acc: &mut String, output: &mut Vec<TranslationOutput>) {
        let acc = std::mem::take(acc);
        if !acc.is_empty() {
            output.push(TranslationOutput::Literal(acc));
        }
    }

    fn flush_class(acc: &mut String, output: &mut Vec<TranslationOutput>) {
        let acc = std::mem::take(acc);
        output.push(TranslationOutput::CharClass(acc));
    }

    for c in other.chars() {
        match (state, c) {
            (ParseState::CharClass, ']') => {
                flush_class(&mut current_acc, &mut output);
                state = ParseState::Normal;
            }
            (ParseState::CharClass, '\\') => {
                state = ParseState::CharClassLiteral;
                current_acc.push(c);
            }
            (ParseState::CharClass, _) => current_acc.push(c),
            (ParseState::CharClassLiteral, _) => {
                state = ParseState::CharClass;
                current_acc.push(c);
            }
            (ParseState::LastPercent, _) => {
                state = ParseState::Normal;
                flush_acc(&mut current_acc, &mut output);
                let mut special = String::with_capacity(2);
                special.push('%');
                special.push(c);
                output.push(TranslationOutput::LuaSpecial(special));
            }
            (ParseState::Normal, '[') => {
                flush_acc(&mut current_acc, &mut output);
                state = ParseState::CharClass;
            }
            (ParseState::Normal, '%') => {
                state = ParseState::LastPercent;
            }
            (ParseState::Normal, _) if r".()+*-?^$".contains(c) => {
                flush_acc(&mut current_acc, &mut output);
                output.push(TranslationOutput::LuaSpecial(c.to_string()));
            }
            (ParseState::Normal, _) => current_acc.push(c),
        };
    }

    if state.is_char_class() {
        current_acc.insert(0, '[');
    }
    flush_acc(&mut current_acc, &mut output);

    output
        .into_iter()
        .map(|item| match item {
            TranslationOutput::Literal(s) => regex::escape(&s),
            TranslationOutput::LuaSpecial(s) => lua_special_to_regex(&s),
            TranslationOutput::CharClass(s) => format!("[{s}]"),
        })
        .collect()
}

/// Translates a path pattern from the typed hook config into a regex. A pattern prefixed with
/// `re:` is already a regex; any other pattern is a Lua pattern.
pub(crate) fn lua_pattern_to_regex(pattern: &str) -> String {
    match pattern.strip_prefix("re:") {
        Some(regex) => regex.to_owned(),
        None => pattern_to_regex(pattern),
    }
}

#[cfg(test)]
mod tests {
    use fancy_regex::Regex;
    use mononoke_macros::mononoke;

    // All tests represent something that went wrong in translation during manual testing
    use super::*;

    #[mononoke::test]
    fn test_literal() {
        assert_eq!("hello", pattern_to_regex("hello"));
    }

    #[mononoke::test]
    fn test_anchors() {
        assert_eq!("^str$", pattern_to_regex("^str$"));
        assert_eq!("^buck\\-out/", pattern_to_regex("^buck%-out/"));
    }

    #[mononoke::test]
    fn test_escaped() {
        assert_eq!("/buck\\-out/", pattern_to_regex("/buck%-out/"));
    }

    #[mononoke::test]
    fn test_class() {
        assert_eq!("[[:alpha:]]", pattern_to_regex("%a"));
        assert_eq!("[[:^digit:]]", pattern_to_regex("%D"));
        assert_eq!("[abc]", pattern_to_regex("[abc]"));
        assert_eq!("/[.]git/", pattern_to_regex("/[.]git/"));
        assert_eq!("^[.]git/", pattern_to_regex("^[.]git/"));
        assert_eq!("[ab\\]]", pattern_to_regex("[ab\\]]"));
    }

    fn compile(pattern: &str) -> Regex {
        Regex::new(&lua_pattern_to_regex(pattern)).expect("Could not map pattern to regex")
    }

    #[mononoke::test]
    fn test_matching() {
        let pattern = compile(r"^[.]git/");
        assert!(pattern.is_match(".git/foo").unwrap());
        assert!(!pattern.is_match("./git/foo").unwrap());

        let pattern = compile(r"^buck%-out/");
        assert!(pattern.is_match("buck-out/file").unwrap());
        assert!(!pattern.is_match("/buck-out/file").unwrap());
    }

    #[mononoke::test]
    fn test_re_matching() {
        let pattern = compile(r"re:^[.]git/");
        assert!(pattern.is_match(".git/foo").unwrap());
        assert!(!pattern.is_match("./git/foo").unwrap());

        let pattern = compile(r"re:^buck-out/");
        assert!(pattern.is_match("buck-out/file").unwrap());
        assert!(!pattern.is_match("/buck-out/file").unwrap());
    }

    #[mononoke::test]
    fn test_fancy_re_matching() {
        let pattern = compile(r"re:^www/(?!html/(xplat-react|megarepo))");
        assert!(pattern.is_match("www/html/foo").unwrap());
        assert!(!pattern.is_match("www/html/xplat-react").unwrap());
    }
}
