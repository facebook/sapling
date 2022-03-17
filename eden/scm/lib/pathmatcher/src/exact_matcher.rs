/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Pattern matcher that matches an exact set of paths.

use std::collections::HashSet;

use anyhow::Result;
use types::RepoPath;
use types::RepoPathBuf;

use crate::DirectoryMatch;
use crate::Matcher;

/// A [Matcher] that only matches an exact list of file paths.
#[derive(Clone, Debug)]
pub struct ExactMatcher {
    paths: HashSet<RepoPathBuf>,
}

impl ExactMatcher {
    /// Create [ExactMatcher] using an exact list of file paths.
    ///
    /// The matcher will only match files explicitly listed.
    pub fn new(paths: impl Iterator<Item = impl AsRef<RepoPath>>) -> Self {
        ExactMatcher {
            paths: paths.map(|p| p.as_ref().to_owned()).collect(),
        }
    }
}

impl Matcher for ExactMatcher {
    fn matches_directory(&self, _path: &RepoPath) -> Result<DirectoryMatch> {
        // TODO: determine which directories we can avoid traversing.
        Ok(DirectoryMatch::ShouldTraverse)
    }

    fn matches_file(&self, path: &RepoPath) -> Result<bool> {
        Ok(self.paths.contains(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic() {
        let paths = ["file1", "d1/d2", "d1/d2/file2", "d1/file3", "d2/file4"];
        let paths = paths
            .iter()
            .map(|p| RepoPath::from_str(p).unwrap().to_owned());
        let m = ExactMatcher::new(paths);

        let cases = [
            ("", false), // empty path shouldn't match
            ("file1", true),
            ("d1/d2/file2", true),
            ("d1/file3", true),
            ("d2/file4", true),
            ("bad_file", false),
            ("bad_dir/f3", false),
            ("d1/bad", false),
            ("d1/d2/bad", false),
            ("d1", false),   // regular directories shouldn't match
            ("d1/d2", true), // directories that are also files should match
            ("d1/d2/file", false),
            ("file", false), // name prefixes shouldn't match
        ];
        for (path, should_match) in cases {
            let matches = m.matches_file(RepoPath::from_str(path).unwrap()).unwrap();
            assert_eq!(should_match, matches, "Matching {:?}", path);
        }
    }
}
