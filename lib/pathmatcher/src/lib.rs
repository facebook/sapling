// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

mod gitignore_matcher;
mod tree_matcher;
mod utils;

use std::ops::Deref;

use types::RepoPath;

/// Limits the set of files to be operated on.
pub trait Matcher {
    /// This method is intended for tree traversals of the file system.
    /// It allows for fast paths where whole subtrees are skipped.
    /// It should be noted that the DirectoryMatch::ShouldTraverse return value is always correct.
    /// Other values enable fast code paths only (performance).
    fn matches_directory(&self, path: &RepoPath) -> DirectoryMatch;

    /// Returns true when the file path should be kept in the file set and returns false when
    /// it has to be removed.
    fn matches_file(&self, path: &RepoPath) -> bool;
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
    fn matches_directory(&self, path: &RepoPath) -> DirectoryMatch {
        T::matches_directory(self, path)
    }

    fn matches_file(&self, path: &RepoPath) -> bool {
        T::matches_file(self, path)
    }
}

pub struct AlwaysMatcher {}

impl AlwaysMatcher {
    pub fn new() -> Self {
        AlwaysMatcher {}
    }
}

impl Matcher for AlwaysMatcher {
    fn matches_directory(&self, _path: &RepoPath) -> DirectoryMatch {
        DirectoryMatch::Everything
    }
    fn matches_file(&self, _path: &RepoPath) -> bool {
        true
    }
}

pub struct NeverMatcher {}

impl NeverMatcher {
    pub fn new() -> Self {
        NeverMatcher {}
    }
}

impl Matcher for NeverMatcher {
    fn matches_directory(&self, _path: &RepoPath) -> DirectoryMatch {
        DirectoryMatch::Nothing
    }
    fn matches_file(&self, _path: &RepoPath) -> bool {
        false
    }
}

pub use gitignore_matcher::GitignoreMatcher;
pub use tree_matcher::TreeMatcher;
pub use utils::expand_curly_brackets;
