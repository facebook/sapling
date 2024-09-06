/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use pathmatcher_types::DirectoryMatch;
use pathmatcher_types::Matcher;
use types::RepoPath;

/// Git reference matcher to select what references to read when syncing
/// between Git (references) and Sapling (metalog).
///
/// Note: The references to read is a super set of references to sync to
/// metalog. For example, we read "refs/remotes/origin/HEAD" to figure out the
/// main branch. If both "refs/heads/x" with "refs/remotes/origin/x" exist,
/// treat "refs/heads/x" as a visiblehead instead of a bookmark.
/// "refs/remotes/origin/x" is read, but might not get imported to metalog.
pub(crate) struct GitRefMatcher;

impl GitRefMatcher {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl Matcher for GitRefMatcher {
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
        let m = GitRefMatcher::new();
        assert!(m.matches_file(p("refs/heads/foo/bar"))?);
        assert!(m.matches_file(p("refs/remotes/foo/bar"))?);
        assert!(m.matches_file(p("HEAD"))?);
        assert!(m.matches_file(p("refs/visibleheads/123"))?);
        assert!(!m.matches_file(p("refs/notes/foo/bar"))?);
        assert_ne!(m.matches_directory(p("refs"))?, DirectoryMatch::Nothing);

        Ok(())
    }

    fn p(s: &str) -> &RepoPath {
        RepoPath::from_str(s).unwrap()
    }
}
