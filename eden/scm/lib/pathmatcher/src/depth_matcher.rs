/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Pattern matcher that matches based on path depth.

use anyhow::Result;
use types::RepoPath;

use crate::DirectoryMatch;
use crate::Matcher;

/// A [Matcher] that matches based on min/max path depth.
/// A depth of zero corresponds to files directly in the root directory.
#[derive(Clone, Debug)]
pub struct DepthMatcher {
    min_depth: Option<usize>,
    max_depth: Option<usize>,
}

impl DepthMatcher {
    /// Construct a new DepthMatcher. None means "no limit".
    pub fn new(min_depth: Option<usize>, max_depth: Option<usize>) -> Self {
        Self {
            min_depth,
            max_depth,
        }
    }
}

impl Matcher for DepthMatcher {
    fn matches_directory(&self, path: &RepoPath) -> Result<DirectoryMatch> {
        let path_depth = path.depth();

        Ok(match (self.min_depth, self.max_depth) {
            (None, None) => DirectoryMatch::Everything,
            (Some(min), None) if path_depth >= min => DirectoryMatch::Everything,
            (_, Some(max)) if path_depth > max => DirectoryMatch::Nothing,
            (Some(min), Some(max)) if max < min => DirectoryMatch::Nothing,
            _ => DirectoryMatch::ShouldTraverse,
        })
    }

    fn matches_file(&self, path: &RepoPath) -> Result<bool> {
        let path_depth = path.depth().saturating_sub(1);

        Ok(self.min_depth.is_none_or(|min| path_depth >= min)
            && self.max_depth.is_none_or(|max| path_depth <= max))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn p(s: &'static str) -> &'static RepoPath {
        RepoPath::from_str(s).unwrap()
    }

    #[test]
    fn test_both_limits() {
        let m = DepthMatcher::new(Some(1), Some(2));
        assert_eq!(
            m.matches_directory(p("")).unwrap(),
            DirectoryMatch::ShouldTraverse
        );
        assert!(!m.matches_file(p("foo")).unwrap());

        assert_eq!(
            m.matches_directory(p("foo/bar")).unwrap(),
            DirectoryMatch::ShouldTraverse
        );
        assert!(m.matches_file(p("foo/bar/baz")).unwrap());

        assert_eq!(
            m.matches_directory(p("foo/bar/baz")).unwrap(),
            DirectoryMatch::Nothing
        );
        assert!(!m.matches_file(p("foo/bar/baz/qux")).unwrap());
    }

    #[test]
    fn test_min_only() {
        let m = DepthMatcher::new(Some(1), None);
        assert_eq!(
            m.matches_directory(p("")).unwrap(),
            DirectoryMatch::ShouldTraverse
        );
        assert!(!m.matches_file(p("foo")).unwrap());

        assert_eq!(
            m.matches_directory(p("foo")).unwrap(),
            DirectoryMatch::Everything
        );
        assert!(m.matches_file(p("foo/bar")).unwrap());

        assert_eq!(
            m.matches_directory(p("foo/bar")).unwrap(),
            DirectoryMatch::Everything
        );
        assert!(m.matches_file(p("foo/bar/baz")).unwrap());
    }

    #[test]
    fn test_max_only() {
        let m = DepthMatcher::new(None, Some(1));
        assert_eq!(
            m.matches_directory(p("")).unwrap(),
            DirectoryMatch::ShouldTraverse
        );
        assert!(m.matches_file(p("foo")).unwrap());

        assert_eq!(
            m.matches_directory(p("foo")).unwrap(),
            DirectoryMatch::ShouldTraverse
        );
        assert!(m.matches_file(p("foo/bar")).unwrap());

        assert_eq!(
            m.matches_directory(p("foo/bar")).unwrap(),
            DirectoryMatch::Nothing
        );
        assert!(!m.matches_file(p("foo/bar/baz")).unwrap());
    }

    #[test]
    fn test_unlimited_matcher() {
        // No limits specified.
        let m = DepthMatcher::new(None, None);
        assert_eq!(
            m.matches_directory(p("")).unwrap(),
            DirectoryMatch::Everything
        );
        assert!(m.matches_file(p("foo")).unwrap());

        assert_eq!(
            m.matches_directory(p("foo/bar")).unwrap(),
            DirectoryMatch::Everything
        );
        assert!(m.matches_file(p("foo/bar/baz")).unwrap());
    }

    #[test]
    fn test_null_matcher() {
        // Backwards limits.
        let m = DepthMatcher::new(Some(2), Some(1));
        assert_eq!(m.matches_directory(p("")).unwrap(), DirectoryMatch::Nothing);
        assert!(!m.matches_file(p("foo")).unwrap());

        assert_eq!(
            m.matches_directory(p("foo/bar")).unwrap(),
            DirectoryMatch::Nothing
        );
        assert!(!m.matches_file(p("foo/bar/baz")).unwrap());

        assert_eq!(
            m.matches_directory(p("foo/bar/baz/qux")).unwrap(),
            DirectoryMatch::Nothing
        );
        assert!(!m.matches_file(p("foo/bar/baz/qux/file")).unwrap());
    }
}
