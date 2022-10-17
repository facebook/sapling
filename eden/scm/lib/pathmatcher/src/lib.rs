/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod error;
mod exact_matcher;
mod gitignore_matcher;
mod pattern;
mod regex_matcher;
mod tree_matcher;
mod utils;

use std::ops::Deref;
use std::sync::Arc;

use anyhow::Result;
use types::RepoPath;

/// Limits the set of files to be operated on.
pub trait Matcher {
    /// This method is intended for tree traversals of the file system.
    /// It allows for fast paths where whole subtrees are skipped.
    /// It should be noted that the DirectoryMatch::ShouldTraverse return value is always correct.
    /// Other values enable fast code paths only (performance).
    fn matches_directory(&self, path: &RepoPath) -> Result<DirectoryMatch>;

    /// Returns true when the file path should be kept in the file set and returns false when
    /// it has to be removed.
    fn matches_file(&self, path: &RepoPath) -> Result<bool>;
}

/// Allows for fast code paths when dealing with patterns selecting directories.
/// `Everything` means that all the files in the subtree of the given directory need to be part
/// of the returned file set.
/// `Nothing` means that no files in the subtree of the given directory will be part of the
/// returned file set. Recursive traversal can be stopped at this point.
/// `ShouldTraverse` is a value that is always valid. It does not provide additional information.
/// Subtrees should be traversed and the matches should continue to be asked.
#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq)]
pub enum DirectoryMatch {
    Everything,
    Nothing,
    ShouldTraverse,
}

impl<T: Matcher + ?Sized, U: Deref<Target = T>> Matcher for U {
    fn matches_directory(&self, path: &RepoPath) -> Result<DirectoryMatch> {
        T::matches_directory(self, path)
    }

    fn matches_file(&self, path: &RepoPath) -> Result<bool> {
        T::matches_file(self, path)
    }
}

#[derive(Clone)]
pub struct AlwaysMatcher {}

impl AlwaysMatcher {
    pub fn new() -> Self {
        AlwaysMatcher {}
    }
}

impl Matcher for AlwaysMatcher {
    fn matches_directory(&self, _path: &RepoPath) -> Result<DirectoryMatch> {
        Ok(DirectoryMatch::Everything)
    }
    fn matches_file(&self, _path: &RepoPath) -> Result<bool> {
        Ok(true)
    }
}

#[derive(Clone)]
pub struct NeverMatcher {}

impl NeverMatcher {
    pub fn new() -> Self {
        NeverMatcher {}
    }
}

impl Matcher for NeverMatcher {
    fn matches_directory(&self, _path: &RepoPath) -> Result<DirectoryMatch> {
        Ok(DirectoryMatch::Nothing)
    }
    fn matches_file(&self, _path: &RepoPath) -> Result<bool> {
        Ok(false)
    }
}

pub struct XorMatcher<A, B> {
    a: A,
    b: B,
}

impl<A, B> XorMatcher<A, B> {
    pub fn new(a: A, b: B) -> Self {
        XorMatcher { a, b }
    }
}

impl<A: Matcher, B: Matcher> Matcher for XorMatcher<A, B> {
    fn matches_directory(&self, path: &RepoPath) -> Result<DirectoryMatch> {
        let matches_a = self.a.matches_directory(path)?;
        let matches_b = self.b.matches_directory(path)?;
        Ok(match (matches_a, matches_b) {
            (DirectoryMatch::Everything, DirectoryMatch::Everything) => DirectoryMatch::Nothing,
            (DirectoryMatch::Nothing, DirectoryMatch::Nothing) => DirectoryMatch::Nothing,
            (DirectoryMatch::Everything, DirectoryMatch::Nothing) => DirectoryMatch::Everything,
            (DirectoryMatch::Nothing, DirectoryMatch::Everything) => DirectoryMatch::Everything,
            _ => DirectoryMatch::ShouldTraverse,
        })
    }

    fn matches_file(&self, path: &RepoPath) -> Result<bool> {
        Ok(self.a.matches_file(path)? ^ self.b.matches_file(path)?)
    }
}

pub struct DifferenceMatcher<A, B> {
    include: A,
    exclude: B,
}

impl<A, B> DifferenceMatcher<A, B> {
    pub fn new(include: A, exclude: B) -> Self {
        DifferenceMatcher { include, exclude }
    }
}

impl<A: Matcher, B: Matcher> Matcher for DifferenceMatcher<A, B> {
    fn matches_directory(&self, path: &RepoPath) -> Result<DirectoryMatch> {
        let include = self.include.matches_directory(path)?;

        // Don't execute the exclude ahead of time, since in some cases we can avoid executing it
        // entirely. This is useful when the exclude side is expensive, like in the status case
        // where the exclude side may inspect a manifest or the treestate.
        Ok(match include {
            DirectoryMatch::Nothing => DirectoryMatch::Nothing,
            DirectoryMatch::Everything => match self.exclude.matches_directory(path)? {
                DirectoryMatch::Nothing => DirectoryMatch::Everything,
                DirectoryMatch::Everything => DirectoryMatch::Nothing,
                DirectoryMatch::ShouldTraverse => DirectoryMatch::ShouldTraverse,
            },
            DirectoryMatch::ShouldTraverse => match self.exclude.matches_directory(path)? {
                DirectoryMatch::Everything => DirectoryMatch::Nothing,
                _ => DirectoryMatch::ShouldTraverse,
            },
        })
    }

    fn matches_file(&self, path: &RepoPath) -> Result<bool> {
        Ok(self.include.matches_file(path)? && !self.exclude.matches_file(path)?)
    }
}

pub struct UnionMatcher {
    matchers: Vec<Arc<dyn 'static + Matcher + Send + Sync>>,
}

impl UnionMatcher {
    pub fn new(matchers: Vec<Arc<dyn 'static + Matcher + Send + Sync>>) -> Self {
        UnionMatcher { matchers }
    }

    pub fn matches_directory<M: Matcher, I: Iterator<Item = M>>(
        matchers: I,
        path: &RepoPath,
    ) -> Result<DirectoryMatch> {
        let mut current = DirectoryMatch::Nothing;
        for matcher in matchers {
            current = match matcher.matches_directory(path)? {
                DirectoryMatch::Nothing => current,
                DirectoryMatch::Everything => {
                    return Ok(DirectoryMatch::Everything);
                }
                DirectoryMatch::ShouldTraverse => DirectoryMatch::ShouldTraverse,
            };
        }
        Ok(current)
    }

    pub fn matches_file<M: Matcher, I: Iterator<Item = M>>(
        matchers: I,
        path: &RepoPath,
    ) -> Result<bool> {
        for matcher in matchers {
            if matcher.matches_file(path)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

impl Matcher for UnionMatcher {
    fn matches_directory(&self, path: &RepoPath) -> Result<DirectoryMatch> {
        UnionMatcher::matches_directory(self.matchers.iter(), path)
    }

    fn matches_file(&self, path: &RepoPath) -> Result<bool> {
        UnionMatcher::matches_file(self.matchers.iter(), path)
    }
}

pub struct IntersectMatcher {
    matchers: Vec<Arc<dyn 'static + Matcher + Send + Sync>>,
}

impl IntersectMatcher {
    pub fn new(matchers: Vec<Arc<dyn 'static + Matcher + Send + Sync>>) -> Self {
        Self { matchers }
    }
}

impl Matcher for IntersectMatcher {
    fn matches_directory(&self, path: &RepoPath) -> Result<DirectoryMatch> {
        if self.matchers.is_empty() {
            return Ok(DirectoryMatch::Nothing);
        }

        let mut traverse = false;
        for matcher in &self.matchers {
            match matcher.matches_directory(path)? {
                DirectoryMatch::Nothing => return Ok(DirectoryMatch::Nothing),
                DirectoryMatch::ShouldTraverse => traverse = true,
                DirectoryMatch::Everything => {}
            };
        }

        if traverse {
            Ok(DirectoryMatch::ShouldTraverse)
        } else {
            Ok(DirectoryMatch::Everything)
        }
    }

    fn matches_file(&self, path: &RepoPath) -> Result<bool> {
        let mut matched = false;
        for matcher in &self.matchers {
            if !matcher.matches_file(path)? {
                return Ok(false);
            }
            matched = true;
        }
        Ok(matched)
    }
}

pub use error::Error;
pub use exact_matcher::ExactMatcher;
pub use gitignore_matcher::GitignoreMatcher;
pub use pattern::split_pattern;
pub use pattern::PatternKind;
pub use regex_matcher::RegexMatcher;
pub use tree_matcher::TreeMatcher;
pub use utils::expand_curly_brackets;
pub use utils::normalize_glob;
pub use utils::plain_to_glob;

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_intersection_matcher() -> Result<()> {
        let empty = IntersectMatcher::new(Vec::new());
        assert_eq!(
            empty.matches_directory("something".try_into()?)?,
            DirectoryMatch::Nothing
        );
        assert!(!empty.matches_file("something".try_into()?)?);

        let matcher = IntersectMatcher::new(vec![
            Arc::new(ExactMatcher::new(
                [RepoPath::from_str("both/both")?, RepoPath::from_str("a/a")?].iter(),
                true,
            )),
            Arc::new(ExactMatcher::new(
                [RepoPath::from_str("both/both")?, RepoPath::from_str("b/b")?].iter(),
                true,
            )),
        ]);

        assert_eq!(
            matcher.matches_directory("both".try_into()?)?,
            DirectoryMatch::ShouldTraverse
        );
        assert_eq!(
            matcher.matches_directory("neither".try_into()?)?,
            DirectoryMatch::Nothing
        );
        assert_eq!(
            matcher.matches_directory("a".try_into()?)?,
            DirectoryMatch::Nothing
        );

        assert!(matcher.matches_file("both/both".try_into()?)?);
        assert!(!matcher.matches_file("neither".try_into()?)?);
        assert!(!matcher.matches_file("a/a".try_into()?)?);

        Ok(())
    }
}
