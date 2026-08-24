/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Pattern matcher that matches files by basename.

use std::collections::HashSet;

use anyhow::Result;
use types::RepoPath;

use crate::DirectoryMatch;
use crate::Matcher;

/// A [Matcher] that matches files whose basename is in a set of names,
/// in any directory.
///
/// Directory decisions are always [DirectoryMatch::ShouldTraverse]; combine
/// with another matcher (ex. via [crate::IntersectMatcher]) to also limit
/// which directories are visited.
#[derive(Clone, Debug)]
pub struct BasenameMatcher {
    names: HashSet<String>,
    case_sensitive: bool,
}

impl BasenameMatcher {
    pub fn new(names: impl IntoIterator<Item = impl AsRef<str>>, case_sensitive: bool) -> Self {
        let names = names
            .into_iter()
            .map(|name| {
                let name = name.as_ref();
                if case_sensitive {
                    name.to_string()
                } else {
                    name.to_lowercase()
                }
            })
            .collect();
        Self {
            names,
            case_sensitive,
        }
    }
}

impl Matcher for BasenameMatcher {
    fn matches_directory(&self, _path: &RepoPath) -> Result<DirectoryMatch> {
        Ok(DirectoryMatch::ShouldTraverse)
    }

    fn matches_file(&self, path: &RepoPath) -> Result<bool> {
        Ok(path.last_component().is_some_and(|name| {
            if self.case_sensitive {
                self.names.contains(name.as_str())
            } else {
                self.names.contains(&name.as_str().to_lowercase())
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_basenames_in_any_directory() -> Result<()> {
        let m = BasenameMatcher::new([".arcconfig", "BUCK"], true);
        assert!(m.matches_file(RepoPath::from_str(".arcconfig")?)?);
        assert!(m.matches_file(RepoPath::from_str("a/b/BUCK")?)?);
        assert!(!m.matches_file(RepoPath::from_str("a/BUCK.bak")?)?);
        assert!(!m.matches_file(RepoPath::from_str("a/buck")?)?);
        assert_eq!(
            m.matches_directory(RepoPath::from_str("any/dir")?)?,
            DirectoryMatch::ShouldTraverse
        );
        Ok(())
    }

    #[test]
    fn test_case_insensitive_matches_any_case() -> Result<()> {
        let m = BasenameMatcher::new([".arcconfig", "BUCK"], false);
        assert!(m.matches_file(RepoPath::from_str("a/buck")?)?);
        assert!(m.matches_file(RepoPath::from_str("a/.ArcConfig")?)?);
        assert!(!m.matches_file(RepoPath::from_str("a/BUCK.bak")?)?);
        Ok(())
    }
}
