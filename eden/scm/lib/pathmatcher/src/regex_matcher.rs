/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use regex_automata::dfa::dense;
use regex_automata::dfa::Automaton;
use regex_automata::dfa::StartKind;
use regex_automata::util::syntax;
use regex_automata::Anchored;
use regex_automata::Input;
use types::RepoPath;

use crate::DirectoryMatch;
use crate::Matcher;

/// Pattern matcher constructed by an regular expression.
///
/// The regular expression syntax [1] is same as regex crate with below limitations:
///   * Lacks a few features like look around and backreferences (e.g. '?!').
///   * Does not support Unicode word boundaries.
///
/// [1] https://docs.rs/regex/1.9.3/regex/#syntax
///
/// The [RegexMatcher::match_prefix] method can be used to rule out
/// unnecessary directory visit early.
#[derive(Clone)]
pub struct RegexMatcher {
    // The regular expression pattern.
    pattern: String,

    // A table-based deterministic finite automaton (DFA) constructed by regular
    // expression.
    //
    // The RegexBuilder generally constructs two DFAs, where one is responsible for
    // finding the end of a match and the other is responsible for finding the
    // start of a match. Since we only need to detect whether something matched,
    // we use a dense::Builder to construct a single DFA here, which is cheaper
    // than building two DFAs.
    dfa: dense::DFA<Vec<u32>>,
}

impl RegexMatcher {
    pub fn new(pattern: &str, case_sensitive: bool) -> Result<Self> {
        // `StartKind::Anchored` makes the dfa searching at the beginning of the
        // string. This is similar to Python's `re.match` behavior, which is
        // used in the match.py
        let dfa = dense::Builder::new()
            .configure(dense::DFA::config().start_kind(StartKind::Anchored))
            .syntax(syntax::Config::new().case_insensitive(!case_sensitive))
            .build(pattern)?;

        Ok(RegexMatcher {
            pattern: pattern.to_string(),
            dfa,
        })
    }

    /// Return `Some(bool)` if the end state of the DFA is 'match' or 'dead' state.
    /// Return `None` otherwise.
    pub fn match_prefix(&self, dir: &str) -> Option<bool> {
        // empty dir is for testing the "root" dir
        if dir.is_empty() {
            return None;
        }

        let bytes = dir.as_bytes();
        // safety: `unwrap` is okay because `start_state_forward` returns error when:
        // - explicitly set quit bytes
        // - Anchored mode does not match
        // both of them cannot happen in our use case.
        let mut state = self
            .dfa
            .start_state_forward(&Input::new(dir).anchored(Anchored::Yes))
            .unwrap();

        for b in bytes {
            state = self.dfa.next_state(state, *b);
            if self.dfa.is_dead_state(state) {
                return Some(false);
            } else if self.dfa.is_match_state(state) {
                return Some(true);
            }
        }
        // Adding trailing '/' to handle cases like: if the pattern is `aa/bb`, then
        // it should return `Some(false)` for input "a"
        if bytes.last() != Some(&b'/') {
            state = self.dfa.next_state(state, b'/');
        }

        if self.dfa.is_dead_state(state) {
            return Some(false);
        }

        state = self.dfa.next_eoi_state(state);
        if self.dfa.is_match_state(state) {
            return Some(true);
        }
        None
    }

    /// Return if `path` matches with the matcher.
    pub fn matches(&self, path: &str) -> bool {
        // empty regex pattern (""), which describes the empty language. The empty language is
        // a sub-language of every other language. So it will true without checking the input.
        if self.pattern.is_empty() {
            return true;
        }

        // safety: `unwrap` is okay because `start_state_forward` returns error when:
        // - explicitly set quit bytes
        // - Anchored mode does not match
        // both of them cannot happen in our use case.
        let mut state = self
            .dfa
            .start_state_forward(&Input::new(path).anchored(Anchored::Yes))
            .unwrap();

        for &b in path.as_bytes() {
            state = self.dfa.next_state(state, b);
            if self.dfa.is_dead_state(state) {
                return false;
            } else if self.dfa.is_match_state(state) {
                return true;
            }
        }

        state = self.dfa.next_eoi_state(state);
        self.dfa.is_match_state(state)
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

/// A matcher that supports PCRE style lookarounds and backreferences.
///
/// Not only can these regexes suffer from exponential time complexity, there's
/// no way to implement matches_directory search pruning, so this matcher will
/// force a full traversal.
///
/// This is deprecated and should not be used except to maintain backwards
/// compatibility.
pub struct SlowRegexMatcher(fancy_regex::Regex);

impl SlowRegexMatcher {
    pub fn new(pattern: &str, case_sensitive: bool) -> Result<Self> {
        let flags = if case_sensitive { "-i" } else { "i" };
        Ok(Self(fancy_regex::Regex::new(&format!(
            "^(?{flags}:{pattern})"
        ))?))
    }
}

impl Matcher for SlowRegexMatcher {
    fn matches_directory(&self, _path: &RepoPath) -> Result<DirectoryMatch> {
        Ok(DirectoryMatch::ShouldTraverse)
    }

    fn matches_file(&self, path: &RepoPath) -> Result<bool> {
        Ok(self.0.is_match(path.as_str())?)
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

    #[test]
    fn test_slow_regex() -> Result<()> {
        let m = SlowRegexMatcher::new(r".*(?<!bad)(good)\1(?=next)", true)?;

        // Always need to traverse.
        assert_eq!(
            m.matches_directory("whatever".try_into()?)?,
            DirectoryMatch::ShouldTraverse
        );

        // Sanity check fancy features.
        assert!(m.matches_file("okgoodgoodnext".try_into()?)?);
        assert!(!m.matches_file("badgoodgoodnext".try_into()?)?);
        assert!(!m.matches_file("okgoodgood".try_into()?)?);

        Ok(())
    }
}
