/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

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
///     1. Anchors such ^, \A and \z. RegexMatcher checks for a match only at the beginning
///        of the string by default, so there is no need for '^'.
///     2. Word boundary assertions such as \b and \B.
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
    pub fn new(pattern: &str) -> Result<Self> {
        let pattern = replace_eol(pattern)?;

        // The RE library doesn't support ^, we use this `Builder::anchored` to
        // make the search anchored at the beginning of the input. By default,
        // the regex will act as if the pattern started with a .*?, which enables
        // a match to appear anywhere.
        let dfa = Builder::new().anchored(true).build(&pattern)?;

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

/// Replace eol ('$') with '\0', since regex-automata doesn't support '$'.
fn replace_eol(pattern: &str) -> Result<String> {
    // Find the positions of eol ('$') by traversing the Ast tree, then replace
    // eol with '\0'
    fn traverse_ast(ast: &Ast, output: &mut String) {
        match ast {
            Ast::Group(group) => traverse_ast(&group.ast, output),
            Ast::Assertion(assertion) => {
                if assertion.kind == AssertionKind::EndLine {
                    let start = assertion.span.start.offset;
                    let end = assertion.span.end.offset;
                    assert_eq!(start + 1, end, "$ (end of line) should be 1 char");
                    output.replace_range(start..end, "\0");
                }
            }
            Ast::Repetition(repeat) => {
                traverse_ast(&repeat.ast, output);
            }
            Ast::Alternation(alternation) => {
                for t in &alternation.asts {
                    traverse_ast(t, output);
                }
            }
            Ast::Concat(concat) => {
                for t in &concat.asts {
                    traverse_ast(t, output);
                }
            }
            // Ast::Empty, Ast::Flags, Ast::Literal, Ast::Dot, Ast::Class
            _ => {}
        }
    }

    let ast = parse::Parser::new().parse(pattern)?;
    let mut new_pattern = pattern.to_string();
    traverse_ast(&ast, &mut new_pattern);
    Ok(new_pattern)
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
        let m = RegexMatcher::new("").unwrap();
        assert!(m.matches(""));
        assert!(m.matches("a"));
        assert!(m.matches("abc"));
    }

    #[test]
    fn test_re_literal_path() {
        let m = RegexMatcher::new(r"(?:a/.*|b/.*)").unwrap();
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
        let m = RegexMatcher::new(r"aa/t\d+").unwrap();
        assert_eq!(m.match_prefix(""), None);
        assert_eq!(m.match_prefix("a"), Some(false));
        assert_eq!(m.match_prefix("aa"), None);
        assert_eq!(m.match_prefix("aa/t123"), Some(true));
        assert_eq!(m.match_prefix("aa/t123/b"), Some(true));
    }

    #[test]
    fn test_re_simple_pattern() {
        let m = RegexMatcher::new(r"a/t\d+/").unwrap();
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
        let m = RegexMatcher::new(r"a/t\d+.py").unwrap();
        assert_eq!(m.match_prefix(""), None);
        assert_eq!(m.match_prefix("a"), None);
        assert_eq!(m.match_prefix("b"), Some(false));

        assert!(!m.matches("a/tt.py"));
        assert!(m.matches("a/t1.py"));
        assert!(m.matches("a/t1.pyc"));
    }

    #[test]
    fn test_re_with_eol() {
        let m = RegexMatcher::new(r"(:?a/t\d+.py$|a\d*.txt)").unwrap();
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
    fn test_re_replace_eol() {
        assert_eq!(replace_eol(r"*").unwrap_or("err".to_string()), "err");
        assert_eq!(replace_eol(r"a.py").unwrap(), "a.py");
        assert_eq!(replace_eol(r"a.py\$").unwrap(), r"a.py\$");
        assert_eq!(replace_eol(r"a.py$").unwrap(), "a.py\0");
        assert_eq!(replace_eol(r"a.py$|a.txt").unwrap(), "a.py\0|a.txt");
        assert_eq!(replace_eol(r"a.py$|a.txt$").unwrap(), "a.py\0|a.txt\0");
        assert_eq!(
            replace_eol(r"(a.py$|a.txt$)|b.py").unwrap(),
            "(a.py\0|a.txt\0)|b.py"
        );
        assert_eq!(
            replace_eol(r"(a$[b$]c$|d\$)e$").unwrap(),
            "(a\0[b$]c\0|d\\$)e\0"
        );
    }
}
