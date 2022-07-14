/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

use anyhow::Error;
use regex::Regex;

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
            TranslationOutput::CharClass(s) => format!("[{}]", s),
        })
        .collect()
}

pub struct LuaPattern {
    original: String,
    regex: Regex,
}

impl LuaPattern {
    pub fn is_match(&self, s: &str) -> bool {
        self.regex.is_match(s)
    }

    #[allow(dead_code)]
    pub fn get_regex(&self) -> &Regex {
        &self.regex
    }
}

impl TryFrom<&str> for LuaPattern {
    type Error = Error;
    fn try_from(other: &str) -> Result<Self, Error> {
        let regex = Regex::new(&pattern_to_regex(other))?;
        Ok(Self {
            original: other.to_string(),
            regex,
        })
    }
}

impl TryFrom<String> for LuaPattern {
    type Error = Error;
    fn try_from(other: String) -> Result<Self, Error> {
        let regex = Regex::new(&pattern_to_regex(&other))?;
        Ok(Self {
            original: other,
            regex,
        })
    }
}

impl fmt::Display for LuaPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        self.original.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    // All tests represent something that went wrong in translation during manual testing
    use super::*;

    #[test]
    fn test_literal() {
        assert_eq!("hello", pattern_to_regex("hello"));
    }

    #[test]
    fn test_anchors() {
        assert_eq!("^str$", pattern_to_regex("^str$"));
        assert_eq!("^buck\\-out/", pattern_to_regex("^buck%-out/"));
    }

    #[test]
    fn test_escaped() {
        assert_eq!("/buck\\-out/", pattern_to_regex("/buck%-out/"));
    }

    #[test]
    fn test_class() {
        assert_eq!("[[:alpha:]]", pattern_to_regex("%a"));
        assert_eq!("[[:^digit:]]", pattern_to_regex("%D"));
        assert_eq!("[abc]", pattern_to_regex("[abc]"));
        assert_eq!("/[.]git/", pattern_to_regex("/[.]git/"));
        assert_eq!("^[.]git/", pattern_to_regex("^[.]git/"));
        assert_eq!("[ab\\]]", pattern_to_regex("[ab\\]]"));
    }

    #[test]
    fn test_matching() {
        let pattern: LuaPattern = "^[.]git/"
            .try_into()
            .expect("Could not map pattern to regex");
        assert!(pattern.is_match(".git/foo"));
        assert!(!pattern.is_match("./git/foo"));

        let pattern: LuaPattern = "^buck%-out/"
            .try_into()
            .expect("Could not map pattern to regex");
        assert!(pattern.is_match("buck-out/file"));
        assert!(!pattern.is_match("/buck-out/file"));
    }
}
