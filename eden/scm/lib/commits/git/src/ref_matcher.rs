/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;
use pathmatcher_types::DirectoryMatch;
use pathmatcher_types::Matcher;
use types::RepoPath;

/// Decide whether a Git ref should be read/parsed from disk.
///
/// Implemented as a `Matcher`. Happens before `GitRefMetaLogFilter`.
///
/// Basically, select `refs/{heads/,remotes/,remotetags/,visibleheads/,HEAD}`,
/// and ignore others like `refs/{notes/,tags/,FETCH_HEAD}`.
///
/// `GitRefMetaLogFilter` will further filter refs out.
pub(crate) struct GitRefPreliminaryMatcher;

impl GitRefPreliminaryMatcher {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl Matcher for GitRefPreliminaryMatcher {
    fn matches_directory(&self, path: &RepoPath) -> Result<DirectoryMatch> {
        let mut components = path.components();
        let first_component = components.next().map_or("", |c| c.as_str());
        let result = match first_component {
            "HEAD" => DirectoryMatch::ShouldTraverse,
            "refs" => {
                let second_component = components.next().map_or("", |c| c.as_str());
                match second_component {
                    "" => DirectoryMatch::ShouldTraverse,
                    // refs/heads map to local bookmarks.
                    // refs/remotes/origin/HEAD can be used to detect the "main" branch of
                    // "origin".
                    // refs/visibleheads/.. can be added by autopull running `git fetch`.
                    "heads" | "remotes" | "remotetags" | "visibleheads" => {
                        DirectoryMatch::Everything
                    }
                    _ => DirectoryMatch::Nothing,
                }
            }
            _ => DirectoryMatch::Nothing,
        };
        Ok(result)
    }

    fn matches_file(&self, path: &RepoPath) -> Result<bool> {
        match self.matches_directory(path)? {
            DirectoryMatch::Nothing => Ok(false),
            _ => Ok(true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic() -> Result<()> {
        let m = GitRefPreliminaryMatcher::new();
        assert!(m.matches_file(p("refs/heads/foo/bar"))?);
        assert!(m.matches_file(p("refs/remotes/foo/bar"))?);
        assert!(m.matches_file(p("HEAD"))?);
        assert!(!m.matches_file(p("FETCH_HEAD"))?);
        assert!(m.matches_file(p("refs/visibleheads/123"))?);
        assert!(!m.matches_file(p("refs/notes/foo/bar"))?);
        assert_ne!(m.matches_directory(p("refs"))?, DirectoryMatch::Nothing);

        Ok(())
    }

    fn p(s: &str) -> &RepoPath {
        RepoPath::from_str(s).unwrap()
    }
}
