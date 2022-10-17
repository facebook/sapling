/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use anyhow::Result;
use regex_automata::dense::Builder;
use regex_automata::DenseDFA;
use regex_automata::DFA;
use regex_syntax::ast::parse;
use regex_syntax::ast::AssertionKind;
use regex_syntax::ast::Ast;
use types::RepoPath;

use crate::DirectoryMatch;
use crate::Matcher;

/// Pattern matcher constructed by an regular expression.
///
/// The regular expression syntax is same as regex crate with below limitations
/// due to the underlying RE lib (regex_automata):
///     1. Anchors \A, \z. RegexMatcher checks for a match only at the beginning
///        of the string by default, so '^' is not required.
///     2. Word boundary assertions such as \b and \B.
///     3. Lacks a few features like look around and backreferences (e.g. '?!').
///
/// The [RegexMatcher::match_prefix] method can be used to rule out
/// unnecessary directory visit early.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct RegexMatcher {
    // Transformed regular expression pattern after replacing '$' (End-Of-Line) with '\0'.
    //
    // The underlying regex engine doesn't support '$' (EOL), we workaround this limitation
    // by replacing '$' with '\0' (which cannot occur in a path) in the pattern. Then have
    // `matches` API add a '\0' for testing when needed.
    pattern: String,

    // A table-based deterministic finite automaton (DFA) constructed by regular
    // expression.
    //
    // The RegexBuilder generally constructs two DFAs, where one is responsible for
    // finding the end of a match and the other is responsible for finding the
    // start of a match. Since we only need to detect whether something matched,
    // we use a dense::Builder to construct a single DFA here, which is cheaper
    // than building two DFAs.
    dfa: DenseDFA<Vec<usize>, usize>,
}

impl RegexMatcher {
    pub fn new(pattern: &str, case_sensitive: bool) -> Result<Self> {
        let pattern = handle_sol_eol(pattern)?;

        // The RE library doesn't support ^, we use this `Builder::anchored` to
        // make the search anchored at the beginning of the input. By default,
        // the regex will act as if the pattern started with a .*?, which enables
        // a match to appear anywhere.
        let dfa = Builder::new()
            .anchored(true)
            .case_insensitive(!case_sensitive)
            .build(&pattern)?;

        Ok(RegexMatcher { pattern, dfa })
    }

    /// Return `Some(bool)` if the end state of the DFA is 'match' or 'dead' state.
    /// Return `None` otherwise.
    pub fn match_prefix(&self, dir: &str) -> Option<bool> {
        // empty dir is for testing the "root" dir
        if dir.is_empty() {
            return None;
        }
        let bytes = dir.as_bytes();
        let mut state = self.dfa.start_state();

        for b in bytes {
            state = self.dfa.next_state(state, *b);
            if self.dfa.is_match_or_dead_state(state) {
                break;
            }
        }
        // Adding trailing '/' to handle cases like: if the pattern is `aa/bb`, then
        // it should return `Some(false)` for input "a"
        if !self.dfa.is_match_or_dead_state(state) && bytes.last() != Some(&b'/') {
            state = self.dfa.next_state(state, b'/');
        }

        if self.dfa.is_dead_state(state) {
            return Some(false);
        } else if self.dfa.is_match_state(state) {
            return Some(true);
        }
        None
    }

    /// Return if `path` matches with the matcher.
    pub fn matches(&self, path: &str) -> bool {
        let bytes = path.as_bytes();
        let mut state = self.dfa.start_state();

        // This handles two special cases:
        // 1. empty regex pattern (""), which describes the empty language. The empty language is
        //    a sub-language of every other language. So it will true without checking the input.
        // 2. it is possible to write a regex that is the opposite of the empty set, i.e., one that
        //    will not match anything. You could write it like so: [a&&b] -- intersection of a and b,
        //    which is empty. Currently though, such patterns won't compile in Rust Regex parser.
        if self.dfa.is_match_or_dead_state(state) {
            return self.dfa.is_match_state(state);
        }

        for &b in bytes {
            // We are using `next_state_unchecked` method here for speed, this follows
            // the implementation of DFA.is_match_at API:
            // https://github.com/BurntSushi/regex-automata/blob/0.1.10/src/dfa.rs#L220
            state = unsafe { self.dfa.next_state_unchecked(state, b) };
            if self.dfa.is_match_or_dead_state(state) {
                return self.dfa.is_match_state(state);
            }
        }

        // At this point, it means we are not in 'match' or 'dead' state, then
        // we add '\0' to check if the pattern is expecting EOL. Check the
        // comment of [RegexMatcher.pattern] for more details.
        state = unsafe { self.dfa.next_state_unchecked(state, b'\0') };
        self.dfa.is_match_state(state)
    }
}

/// Handle sol (start-of-line) and eol (end-of-line) since regex-automata doesn't support '^' and '$'.
///   1. sol ('^'), we just remove it, RegexMatcher will only match at the beginning of the string.
///   2. eol ('$'), we replace it with '\0'
fn handle_sol_eol(pattern: &str) -> Result<String> {
    fn traverse_ast(ast: &Ast, pattern: &mut String, sol_indices: &mut HashSet<usize>) {
        match ast {
            Ast::Group(group) => traverse_ast(&group.ast, pattern, sol_indices),
            Ast::Assertion(assertion) => {
                if assertion.kind == AssertionKind::EndLine {
                    let start = assertion.span.start.offset;
                    let end = assertion.span.end.offset;
                    assert_eq!(start + 1, end, "$ (end of line) should be 1 char");
                    pattern.replace_range(start..end, "\0");
                } else if assertion.kind == AssertionKind::StartLine {
                    let start = assertion.span.start.offset;
                    let end = assertion.span.end.offset;
                    assert_eq!(start + 1, end, "^ (start of line) should be 1 char");
                    sol_indices.insert(start);
                }
            }
            Ast::Repetition(repeat) => {
                traverse_ast(&repeat.ast, pattern, sol_indices);
            }
            Ast::Alternation(alternation) => {
                for t in &alternation.asts {
                    traverse_ast(t, pattern, sol_indices);
                }
            }
            Ast::Concat(concat) => {
                for t in &concat.asts {
                    traverse_ast(t, pattern, sol_indices);
                }
            }
            // Ast::Empty, Ast::Flags, Ast::Literal, Ast::Dot, Ast::Class
            _ => {}
        }
    }

    let ast = parse::Parser::new().parse(pattern)?;
    let mut new_pattern = pattern.to_string();
    let mut sol_indices = HashSet::new();

    traverse_ast(&ast, &mut new_pattern, &mut sol_indices);

    if sol_indices.is_empty() {
        Ok(new_pattern)
    } else {
        let bytes = new_pattern
            .as_bytes()
            .iter()
            .enumerate()
            .filter(|(i, _)| !sol_indices.contains(i))
            .map(|(_, e)| *e)
            .collect::<Vec<u8>>();
        Ok(String::from_utf8(bytes)?)
    }
}

impl Matcher for RegexMatcher {
    fn matches_directory(&self, path: &RepoPath) -> Result<DirectoryMatch> {
        let dm = match self.match_prefix(path.as_str()) {
            Some(true) => DirectoryMatch::Everything,
            Some(false) => DirectoryMatch::Nothing,
            None => DirectoryMatch::ShouldTraverse,
        };
        Ok(dm)
    }

    fn matches_file(&self, path: &RepoPath) -> Result<bool> {
        Ok(self.matches(path.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_re_empty_pattern() {
        let m = RegexMatcher::new("", true).unwrap();
        assert!(m.matches(""));
        assert!(m.matches("a"));
        assert!(m.matches("abc"));
    }

    #[test]
    fn test_re_literal_path() {
        let m = RegexMatcher::new(r"(?:a/.*|b/.*)", true).unwrap();
        assert_eq!(m.match_prefix(""), None);
        assert_eq!(m.match_prefix("a"), Some(true));
        assert_eq!(m.match_prefix("a/"), Some(true));
        assert_eq!(m.match_prefix("b"), Some(true));
        assert_eq!(m.match_prefix("b/"), Some(true));
        assert_eq!(m.match_prefix("c"), Some(false));

        assert!(m.matches("a/c"));
        assert!(m.matches("a/b/c"));
        assert!(m.matches("b/c"));
        assert!(!m.matches("c/a"));
    }

    #[test]
    fn test_re_dir_boundary() {
        let m = RegexMatcher::new(r"aa/t\d+", true).unwrap();
        assert_eq!(m.match_prefix(""), None);
        assert_eq!(m.match_prefix("a"), Some(false));
        assert_eq!(m.match_prefix("aa"), None);
        assert_eq!(m.match_prefix("aa/t123"), Some(true));
        assert_eq!(m.match_prefix("aa/t123/b"), Some(true));
    }

    #[test]
    fn test_re_simple_pattern() {
        let m = RegexMatcher::new(r"a/t\d+/", true).unwrap();
        assert_eq!(m.match_prefix(""), None);
        assert_eq!(m.match_prefix("a"), None);
        assert_eq!(m.match_prefix("a/t123"), Some(true));
        assert_eq!(m.match_prefix("a/t123/"), Some(true));
        assert_eq!(m.match_prefix("a/test"), Some(false));

        assert!(!m.matches("b"));
        assert!(!m.matches("a/t"));
        assert!(!m.matches("a/tt"));
        assert!(!m.matches("a/c"));
        assert!(m.matches("a/t1/"));
        assert!(m.matches("a/t123/"));
    }

    #[test]
    fn test_re_without_eol() {
        let m = RegexMatcher::new(r"a/t\d+.py", true).unwrap();
        assert_eq!(m.match_prefix(""), None);
        assert_eq!(m.match_prefix("a"), None);
        assert_eq!(m.match_prefix("b"), Some(false));

        assert!(!m.matches("a/tt.py"));
        assert!(m.matches("a/t1.py"));
        assert!(m.matches("a/t1.pyc"));
    }

    #[test]
    fn test_re_with_eol() {
        let m = RegexMatcher::new(r"(:?a/t\d+.py$|a\d*.txt)", true).unwrap();
        assert_eq!(m.match_prefix(""), None);
        assert_eq!(m.match_prefix("a"), None);
        assert_eq!(m.match_prefix("b"), Some(false));

        assert!(m.matches("a/t123.py"));
        assert!(m.matches("a.txt"));
        assert!(m.matches("a1.txt"));
        assert!(m.matches("a1.txt1"));
        assert!(!m.matches("a/tt.py"));
        assert!(!m.matches("a/t123.pyc"));
    }

    #[test]
    fn test_re_with_sol() {
        let m = RegexMatcher::new(r"(:?^a/t.py$|^a\^.txt)", true).unwrap();
        assert_eq!(m.match_prefix(""), None);
        assert_eq!(m.match_prefix("a"), None);
        assert_eq!(m.match_prefix("b"), Some(false));

        assert!(m.matches("a/t.py"));
        assert!(!m.matches("ba/t.py"));
        assert!(!m.matches("a/t.pyc"));
        assert!(m.matches(r"a^.txt"));
        assert!(m.matches(r"a^.txt1"));
    }

    #[test]
    fn test_re_handle_sol_eol() {
        assert!(handle_sol_eol(r"*").is_err());
        assert_eq!(handle_sol_eol(r"a.py").unwrap(), "a.py");
        assert_eq!(handle_sol_eol(r"a.py\$").unwrap(), r"a.py\$");
        assert_eq!(handle_sol_eol(r"a.py$").unwrap(), "a.py\0");
        assert_eq!(handle_sol_eol(r"a.py$|a.txt").unwrap(), "a.py\0|a.txt");
        assert_eq!(handle_sol_eol(r"a.py$|a.txt$").unwrap(), "a.py\0|a.txt\0");
        assert_eq!(
            handle_sol_eol(r"(a.py$|a.txt$)|b.py").unwrap(),
            "(a.py\0|a.txt\0)|b.py"
        );
        assert_eq!(
            handle_sol_eol(r"(a$[b$]c$|d\$)e$").unwrap(),
            "(a\0[b$]c\0|d\\$)e\0"
        );

        assert_eq!(
            handle_sol_eol(r"(?:^a[^xyz]\^b$|^abc)").unwrap(),
            "(?:a[^xyz]\\^b\0|abc)"
        );
        // this is the edge case of current implementation of '^' support, which
        // should be rare since the regular expression is actually "wrong".
        assert_eq!(handle_sol_eol(r"^ab^c").unwrap(), "abc");
    }

    #[test]
    fn test_case_insensitive() {
        let case_sensitive = [true, false];
        for sensitive in case_sensitive {
            let m = RegexMatcher::new(r"(?:a/.*|b/.*)", sensitive).unwrap();
            assert_eq!(m.match_prefix(""), None);
            assert_eq!(m.match_prefix("A"), Some(!sensitive));
            assert_eq!(m.match_prefix("A/"), Some(!sensitive));
            assert_eq!(m.match_prefix("B"), Some(!sensitive));
            assert_eq!(m.match_prefix("B/"), Some(!sensitive));

            assert_eq!(m.matches("A/c"), !sensitive);
            assert_eq!(m.matches("A/b/c"), !sensitive);
            assert_eq!(m.matches("B/c"), !sensitive);
        }
    }
}
