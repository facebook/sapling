/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Tree-aware pattern matcher
//!
//! [TreeMatcher] is the main structure.

use bitflags::bitflags;
use globset::{Glob, GlobBuilder, GlobSet, GlobSetBuilder};
use std::path::Path;

use types::RepoPath;

use crate::{DirectoryMatch, Matcher};

bitflags! {
    struct RuleFlags: u8 {
        // A negative rule.
        const NEGATIVE = 1;

        // Auto-generated rule because the user specified a subpath.
        const PARENT = 2;

        // Mark a rule as "recursive" (ex. ending with "/**").
        const RECURSIVE = 4;
    }
}

/// Pattern matcher constructed by an ordered list of positive and negative
/// glob patterns. Negative patterns are prefixed with `!`.
///
/// The syntax is quite similar to gitignore, with some difference to avoid
/// inefficient uses. See [`TreeMatcher::from_rules`] for details about the
/// patterns.
///
/// The [TreeMatcher::match_recursive] method can be used to rule out
/// unnecessary directory visit early.
#[derive(Clone, Debug)]
pub struct TreeMatcher {
    // The [GlobSet] takes care of many algorithm stuff.  It can match a path
    // against multiple patterns and return the pattern indexes.
    glob_set: GlobSet,

    // Flags (ex. negative rule or is it a parent directory) for additional
    // information matching the pattern indexes.
    rule_flags: Vec<RuleFlags>,
}

impl TreeMatcher {
    /// Create [TreeMatcher] using an ordered list of patterns.
    ///
    /// The patterns are glob patterns supported by the `globset` crate.
    /// Like gitignore, negative patterns are supported. They are prefixed
    /// with `!`. Special characters can be escaped by prefixing `\\`.
    ///
    /// Patterns are ordered. A later pattern always overrides an earlier
    /// pattern. Invalid patterns are ignored.
    ///
    /// Unlike gitignore, all patterns are treated as using absolute paths.
    /// That is, `*.c` is treated the same as `/*.c` and does not match `a/b.c`.
    /// Similarly, `!*.c` will be treated as `!/*.c`, in gitignore's sense.
    /// Use `**/*.c` to match files recursively. Note the `**` in the middle
    /// of a pattern effectively disable fast paths provided by `match_recursive`.
    ///
    /// Patterns do not match recursively.
    ///
    /// For example, both `/a/b` and `/a*/b*` do NOT match `/a/b/c/d`. Append
    /// `/**` to make rules recursive. The matcher works best if all rules end
    /// with `**`.
    pub fn from_rules(
        rules: impl Iterator<Item = impl AsRef<str>>,
    ) -> Result<Self, globset::Error> {
        let mut builder = GlobSetBuilder::new();
        let mut rule_flags = Vec::new();

        for rule in rules {
            let rule = rule.as_ref();
            let (negative, rule) = if rule.starts_with("!") {
                (true, &rule[1..])
            } else {
                (false, rule)
            };

            // Strip a leading "/". More friendly to gitignore users.
            let rule = if rule.starts_with("/") {
                &rule[1..]
            } else {
                rule
            };

            // "{", "}" do not have special meaning in gitignore, while
            // globset treats them differently.
            //
            // For now, workaround it by escaping. In the future, this can
            // possibly be done by tweaking a GlobBuilder option in
            // build_glob().
            //
            // See https://github.com/BurntSushi/ripgrep/issues/1183.
            let rule = escape_curly_brackets(&rule);

            // Add flags to the rule_id
            let mut flag = if negative {
                RuleFlags::NEGATIVE
            } else {
                RuleFlags::empty()
            };

            // Insert "parent" rules so match_recursive won't return "None"
            // incorrectly.
            let mut sep_index = 0;
            let rule_bytes = rule.as_ref();
            while let Some(index) = next_path_separator(rule_bytes, sep_index) {
                if index > 0 && index < rule_bytes.len() - 1 {
                    let parent_rule = &rule[..index];
                    let glob = build_glob(parent_rule)?;
                    builder.add(glob);
                    rule_flags.push(flag | RuleFlags::PARENT);
                }
                sep_index = index + 1;
            }
            // Insert the rule.
            // NOTE: This crate depends on the fact that "a/**" matches "a", although
            // the documentation of globset might say otherwise.
            let glob = build_glob(&rule)?;
            builder.add(glob);
            // Mark the rule as recursive so fast paths (i.e. claim everything
            // matches or nothing matches) can be used.
            if rule.ends_with("/**") || rule.ends_with("**/*") || rule == "**" {
                flag |= RuleFlags::RECURSIVE;
            }
            rule_flags.push(flag);
        }

        let glob_set = builder.build()?;
        let matcher = Self {
            glob_set,
            rule_flags,
        };
        Ok(matcher)
    }

    /// Create [TreeMatcher] that matches nothing.
    pub fn never() -> Self {
        let rules: [&str; 0] = [];
        TreeMatcher::from_rules(rules.iter()).unwrap()
    }

    /// Create [TreeMatcher] that matches everything.
    pub fn always() -> Self {
        let rules: [&str; 1] = ["**"];
        TreeMatcher::from_rules(rules.iter()).unwrap()
    }

    /// Return `Some(bool)` if for all path inside the given `dir`,
    /// `matches(path)` will return `bool`.
    ///
    /// Return `None` if there is no fast path.
    ///
    /// `/` should be used as the path separator, regardless of system.
    pub fn match_recursive(&self, dir: impl AsRef<Path>) -> Option<bool> {
        let dir = dir.as_ref();
        // A subpath may match - cannot return Some(false)
        let mut subpath_may_match = false;
        // A subpath may mismatch - cannot return Some(true)
        let mut subpath_may_mismatch = false;
        for id in self.glob_set.matches(dir).into_iter().rev() {
            let flag = self.rule_flags[id];
            if flag.contains(RuleFlags::PARENT) {
                // An auto-generated parent rule matches.
                if flag.contains(RuleFlags::NEGATIVE) {
                    subpath_may_mismatch = true;
                } else {
                    subpath_may_match = true;
                }
            } else {
                // If it is not RECURSIVE, then fast paths (i.e. claim everything
                // matches, or nothing matches) cannot be used.
                if !flag.contains(RuleFlags::RECURSIVE) {
                    subpath_may_match = true;
                    subpath_may_mismatch = true;
                }
                // A non-parent rule matches.
                if flag.contains(RuleFlags::NEGATIVE) {
                    if subpath_may_match {
                        return None;
                    } else {
                        return Some(false);
                    }
                } else {
                    if subpath_may_mismatch {
                        return None;
                    } else {
                        return Some(true);
                    }
                }
            }
        }

        if subpath_may_match {
            None
        } else if !self.rule_flags.is_empty() && dir.to_str() == Some("") {
            // Special case: empty dir
            None
        } else {
            Some(false)
        }
    }

    /// Return if `path` matches with the matcher.
    ///
    /// `/` should be used as the path separator, regardless of system.
    pub fn matches(&self, path: impl AsRef<Path>) -> bool {
        for id in self.glob_set.matches(path).into_iter().rev() {
            let flag = self.rule_flags[id];
            if flag.contains(RuleFlags::PARENT) {
                // For full path matches, parent rules do not count.
                continue;
            } else if flag.contains(RuleFlags::NEGATIVE) {
                // Not matched.
                return false;
            } else {
                // Matched.
                return true;
            }
        }
        // No rule matches
        false
    }
}

impl Matcher for TreeMatcher {
    fn matches_directory(&self, path: &RepoPath) -> DirectoryMatch {
        match self.match_recursive(path.as_str()) {
            Some(true) => DirectoryMatch::Everything,
            Some(false) => DirectoryMatch::Nothing,
            None => DirectoryMatch::ShouldTraverse,
        }
    }

    fn matches_file(&self, path: &RepoPath) -> bool {
        self.matches(path.as_str())
    }
}

fn build_glob(pat: &str) -> Result<Glob, globset::Error> {
    GlobBuilder::new(pat)
        .literal_separator(true) // `*` or `?` should not match `/`
        .backslash_escape(true)
        .build()
}

/// Find the next path separator in a pattern. Respect escaping rules.
/// Return the index (>= `start`), or None if there are no remaining path separator.
fn next_path_separator(pat: &[u8], start: usize) -> Option<usize> {
    let mut in_box_brackets = false;
    let mut escaped = false;

    for (i, ch) in pat.iter().skip(start).enumerate() {
        if escaped {
            match ch {
                _ => escaped = false,
            }
        } else if in_box_brackets {
            match ch {
                b']' => in_box_brackets = false,
                _ => (),
            }
        } else {
            match ch {
                b'\\' => escaped = true,
                b'[' => in_box_brackets = true,
                b'/' => return Some(i + start),
                _ => (),
            }
        }
    }

    None
}

/// Escape `{` and `}` so they no longer have special meanings to `globset`.
fn escape_curly_brackets(pat: &str) -> String {
    if pat.contains('{') || pat.contains('}') {
        let mut result = String::with_capacity(pat.len() * 2);
        for ch in pat.chars() {
            match ch {
                '{' => result.push_str("\\{"),
                '}' => result.push_str("\\}"),
                ch => result.push(ch),
            }
        }
        result
    } else {
        // No escaping is needed
        pat.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_never_matcher() {
        let m = TreeMatcher::never();
        assert_eq!(m.match_recursive(""), Some(false));
        assert_eq!(m.match_recursive("a"), Some(false));
        assert_eq!(m.match_recursive("a/b"), Some(false));
        assert_eq!(m.matches(""), false);
        assert_eq!(m.matches("a/b"), false);
    }

    #[test]
    fn test_always_matcher() {
        let m = TreeMatcher::always();
        assert_eq!(m.match_recursive(""), Some(true));
        assert_eq!(m.match_recursive("a"), Some(true));
        assert_eq!(m.match_recursive("a/b"), Some(true));
        assert_eq!(m.matches(""), true);
        assert_eq!(m.matches("a/b"), true);
    }

    #[test]
    fn test_literal_paths() {
        let m = TreeMatcher::from_rules(["/a/**", "b/c/d/**", "\\e/\\f/**"].iter()).unwrap();
        assert_eq!(m.match_recursive(""), None);
        assert_eq!(m.match_recursive("a"), Some(true));
        assert_eq!(m.match_recursive("a/b"), Some(true));
        assert_eq!(m.match_recursive("b"), None);
        assert_eq!(m.match_recursive("b/c"), None);
        assert_eq!(m.match_recursive("b/c/d"), Some(true));
        assert_eq!(m.match_recursive("b/c/d/e"), Some(true));
        assert_eq!(m.match_recursive("e/f/g"), Some(true));
        assert_eq!(m.match_recursive("c"), Some(false));
        assert_eq!(m.match_recursive("c/a"), Some(false));
        assert_eq!(m.matches(""), false);
        assert_eq!(m.matches("a/b"), true);
        assert_eq!(m.matches("b/x"), false);
        assert_eq!(m.matches("b/c/d/e"), true);
        assert_eq!(m.matches("e"), false);
        assert_eq!(m.matches("e/f1"), false);
        assert_eq!(m.matches("e/f/g"), true);
    }

    #[test]
    fn test_simple_glob() {
        let m = TreeMatcher::from_rules(["a/*[cd][ef]/**"].iter()).unwrap();
        assert_eq!(m.match_recursive("a"), None);
        assert_eq!(m.match_recursive("b"), Some(false));
        assert_eq!(m.match_recursive("a/x"), Some(false));
        assert_eq!(m.match_recursive("a/xde"), Some(true));
        assert_eq!(m.match_recursive("a/xde/x"), Some(true));
        assert_eq!(m.matches("a/12df"), true);
        assert_eq!(m.matches("a/12df/12df"), true);
    }

    #[test]
    fn test_complex_glob() {
        let m = TreeMatcher::from_rules(["a/v/**/*.c/**", "a/**/w/*.c/**"].iter()).unwrap();
        assert_eq!(m.match_recursive("a"), None);
        assert_eq!(m.match_recursive("b"), Some(false));
        assert_eq!(m.match_recursive("a/v/.c"), Some(true));
        assert_eq!(m.match_recursive("a/v/.c/z"), Some(true));
        assert_eq!(m.match_recursive("a/z"), None);
        assert_eq!(m.matches("v/.c"), false);
        assert_eq!(m.matches("a/v/.c"), true);
        assert_eq!(m.matches("a/w/.c"), true);
        assert_eq!(m.matches("a/v/c/v/c/v/c/v/c/v.c"), true);
        assert_eq!(m.matches("a/c/c/c/c/w/w.c"), true);
        assert_eq!(m.matches("a/w/v/w.c"), false);

        // "{" has no special meaning
        let m = TreeMatcher::from_rules(["a/{b,c/d}/**"].iter()).unwrap();
        assert_eq!(m.match_recursive("a/{b,c/d}"), Some(true));
        assert_eq!(m.match_recursive("a/{b,c"), None);
        assert_eq!(m.match_recursive("a/{b,d"), Some(false));
    }

    #[test]
    fn test_mixed_literal_and_simple_glob() {
        let m = TreeMatcher::from_rules(["b/c/d/**", "b/*c/**", "b/1c/**"].iter()).unwrap();
        assert_eq!(m.match_recursive(""), None);
        assert_eq!(m.match_recursive("b/c/d/e"), Some(true));
        assert_eq!(m.match_recursive("b/1c"), Some(true));
        assert_eq!(m.match_recursive("b/xc/yc"), Some(true));
        assert_eq!(m.match_recursive("b/xc"), Some(true));
        assert_eq!(m.match_recursive("b/d"), Some(false));
        assert_eq!(m.matches("b/c/d/e/f"), true);
        assert_eq!(m.matches("b/fc"), true);
        assert_eq!(m.matches("b/ce"), false);
        assert_eq!(m.matches("b/c/e"), true);
    }

    #[test]
    fn test_mixed_literal_and_complex_glob() {
        let m = TreeMatcher::from_rules(["b/c/d/**", "b/**/c/**"].iter()).unwrap();
        assert_eq!(m.match_recursive("b/c/d/e"), Some(true));
        assert_eq!(m.match_recursive("b/d"), None);
        assert_eq!(m.match_recursive("b/c"), Some(true));
        assert_eq!(m.match_recursive("b/x/c/y"), Some(true));
        assert_eq!(m.matches("b/c/d/e/f"), true);
        assert_eq!(m.matches("b/c/d"), true);
        assert_eq!(m.matches("b/c"), true);
        assert_eq!(m.matches("b"), false);
        assert_eq!(m.matches("b/x/y/c/x/y"), true);
    }

    #[test]
    fn test_empty_negative() {
        let m = TreeMatcher::from_rules(["!a/**"].iter()).unwrap();
        assert_eq!(m.match_recursive(""), None); // better answer is Some(false)
        assert_eq!(m.match_recursive("a"), Some(false));
        assert_eq!(m.match_recursive("a/b"), Some(false));
        assert_eq!(m.matches(""), false);
        assert_eq!(m.matches("a/b"), false);
    }

    #[test]
    fn test_literal_negative() {
        let m = TreeMatcher::from_rules(["a/**", "!a/b/**", "a/b/c/**"].iter()).unwrap();
        assert_eq!(m.match_recursive("a"), None);
        assert_eq!(m.match_recursive("a/c"), Some(true));
        assert_eq!(m.match_recursive("a/b"), None);
        assert_eq!(m.match_recursive("a/b/d"), Some(false));
        assert_eq!(m.match_recursive("a/b/c"), Some(true));
        assert_eq!(m.matches("a"), true);
        assert_eq!(m.matches("a/b"), false);
        assert_eq!(m.matches("a/b/c/d"), true);
        assert_eq!(m.matches("a/b/d"), false);
        assert_eq!(m.matches("a/c"), true);
        assert_eq!(m.matches("z"), false);
    }

    #[test]
    fn test_negative_override() {
        let m = TreeMatcher::from_rules(["a/**", "!a/**", "!b/**", "b/**"].iter()).unwrap();
        assert_eq!(m.match_recursive("a/b"), Some(false));
        assert_eq!(m.match_recursive("b/c"), Some(true));
        assert_eq!(m.matches("a"), false);
        assert_eq!(m.matches("b"), true);
    }

    #[test]
    fn test_mixed_negative_literal_simple_glob() {
        let m =
            TreeMatcher::from_rules(["a*/**", "!a1/**", "a1/a/**", "!a1/a*c/**"].iter()).unwrap();
        assert_eq!(m.match_recursive("b"), Some(false));
        assert_eq!(m.match_recursive("a1/a"), Some(true));
        assert_eq!(m.matches("a"), true);
        assert_eq!(m.matches("a1"), false);
        assert_eq!(m.matches("a1/a"), true);
        assert_eq!(m.matches("a1/b"), false);
        assert_eq!(m.matches("a1/a1c"), false);
        assert_eq!(m.matches("a2"), true);
        assert_eq!(m.matches("b"), false);
    }

    #[test]
    fn test_fast_paths() {
        // Some interesting fast paths
        let m = TreeMatcher::from_rules(["a/**/b/**"].iter()).unwrap();
        assert_eq!(m.match_recursive("a/b"), Some(true));
        assert_eq!(m.match_recursive("a/1/2/3/b"), Some(true));

        let m = TreeMatcher::from_rules(["a/**/b/**", "!a/**/b/*/**"].iter()).unwrap();
        assert_eq!(m.match_recursive("a/b/1"), Some(false));
        assert_eq!(m.match_recursive("a/1/2/3/b/2"), Some(false));
    }

    #[test]
    fn test_non_recursive_patterns() {
        let m = TreeMatcher::from_rules(["a/*"].iter()).unwrap();
        assert!(m.matches("a/a"));
        assert!(!m.matches("b/a"));
        assert!(!m.matches("a"));
        assert_eq!(m.match_recursive("a"), None);
        assert_eq!(m.match_recursive("a/b"), None);
        assert_eq!(m.match_recursive("a/b/c"), Some(false));
        assert_eq!(m.match_recursive("b"), Some(false));

        let m = TreeMatcher::from_rules(["*a"].iter()).unwrap();
        assert!(m.matches("aa"));
        assert!(!m.matches("aa/b"));
        assert!(!m.matches("b"));
        assert_eq!(m.match_recursive("aa"), None);
        assert_eq!(m.match_recursive("a/a"), Some(false));
        assert_eq!(m.match_recursive("b"), Some(false));

        let m = TreeMatcher::from_rules(["b*/**/*a"].iter()).unwrap();
        assert!(m.matches("b1/aa"));
        assert!(!m.matches("c/aa"));
        assert!(!m.matches("b1/aa/11"));
        assert!(!m.matches("b/a/b"));
        assert!(m.matches("b/a/b/a"));
        assert_eq!(m.match_recursive("aa"), Some(false));
        assert_eq!(m.match_recursive("b/a/b"), None);
        assert_eq!(m.match_recursive("b/a/b/a"), None);
    }

    #[test]
    fn test_next_path_separator() {
        assert_eq!(next_path_separator(b"/a/b", 0), Some(0));
        assert_eq!(next_path_separator(b"/a/b", 1), Some(2));
        assert_eq!(next_path_separator(b"/a/b", 2), Some(2));
        assert_eq!(next_path_separator(b"/a/b", 3), None);
        assert_eq!(next_path_separator(b"[/]a\\/b", 0), None);
        assert_eq!(next_path_separator(b"\\[/]a", 0), Some(2));
    }
}
