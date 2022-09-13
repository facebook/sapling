/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use regex_automata::dense::Builder;
use regex_automata::DenseDFA;
use regex_automata::Error;
use regex_automata::DFA;
use types::RepoPath;

use crate::DirectoryMatch;
use crate::Matcher;

/// Pattern matcher constructed by an regular expression.
///
/// The regular expression syntax is same as regex crate with below limitations
/// due to the underlying RE lib (regex_automata):
///     1. Anchors such ^, &, \A and \Z
///     2. Word boundary assertions such as \b and \B
///
/// The [RegexMatcher::match_prefix] method can be used to rule out
/// unnecessary directory visit early.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct RegexMatcher {
    // Original regular expression.
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
    pub fn new(pattern: &str) -> Result<Self, Error> {
        // The RE library doesn't support ^, we use this `Builder::anchored` to
        // make the search anchored at the beginning of the input. By default,
        // the regex will act as if the pattern started with a .*?, which enables
        // a match to appear anywhere.
        let dfa = Builder::new().anchored(true).build(pattern)?;

        Ok(RegexMatcher {
            pattern: pattern.to_string(),
            dfa,
        })
    }

    /// Return `Some(bool)` if the end state of the DFA is 'match' or 'dead' state.
    /// Return `None` otherwise.
    pub fn match_prefix(&self, dir: &str) -> Option<bool> {
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
        self.dfa.is_match(path.as_bytes())
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
    fn test_re_literal_path() {
        let m = RegexMatcher::new(r"(?:a/.*|b/.*)").unwrap();
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
        let m = RegexMatcher::new(r"aa/t\d+").unwrap();
        assert_eq!(m.match_prefix("a"), Some(false));
        assert_eq!(m.match_prefix("aa"), None);
        assert_eq!(m.match_prefix("aa/t123"), Some(true));
        assert_eq!(m.match_prefix("aa/t123/b"), Some(true));
    }

    #[test]
    fn test_re_simple_pattern() {
        let m = RegexMatcher::new(r"a/t\d+/").unwrap();
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
    fn test_re_ending() {
        let m = RegexMatcher::new(r"a/t\d+.py").unwrap();
        assert_eq!(m.match_prefix("a"), None);
        assert_eq!(m.match_prefix("b"), Some(false));

        assert!(!m.matches("a/tt.py"));
        assert!(m.matches("a/t1.py"));
        assert!(m.matches("a/t1.pyc"));
    }
}
