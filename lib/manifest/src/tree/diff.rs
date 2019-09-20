// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::cmp::Ordering;

use failure::Fallible;

use pathmatcher::{DirectoryMatch, Matcher};
use types::{RepoPath, RepoPathBuf};

use super::cursor::{Cursor, Step};
use super::link::{Durable, Leaf};
pub use super::store::TreeStore;
use crate::tree::Tree;
use crate::FileMetadata;

/// An iterator over all the differences between two [`Tree`]s. Keeping in mind that
/// manifests operate over files, the difference space is limited to three cases described by
/// [`DiffType`]:
///  * a file may be present only in the left tree manifest
///  * a file may be present only in the right tree manifest
///  * a file may have different file_metadata between the two tree manifests
///
/// For the case where we have the the file "foo" in the `left` tree manifest and we have the "foo"
/// directory in the `right` tree manifest, the differences returned will be:
///  1. DiffEntry("foo", LeftOnly(_))
///  2. DiffEntry(file, RightOnly(_)) for all `file`s under the "foo" directory
pub struct Diff<'a, M> {
    left: Cursor<'a>,
    step_left: bool,
    right: Cursor<'a>,
    step_right: bool,
    matcher: &'a M,
}

impl<'a, M> Diff<'a, M> {
    pub fn new(left: &'a Tree, right: &'a Tree, matcher: &'a M) -> Self {
        Self {
            left: left.root_cursor(),
            step_left: false,
            right: right.root_cursor(),
            step_right: false,
            matcher,
        }
    }
}

/// Represents a file that is different between two tree manifests.
#[derive(Clone, Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct DiffEntry {
    pub path: RepoPathBuf,
    pub diff_type: DiffType,
}

#[derive(Clone, Copy, Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub enum DiffType {
    LeftOnly(FileMetadata),
    RightOnly(FileMetadata),
    Changed(FileMetadata, FileMetadata),
}

impl DiffEntry {
    pub(crate) fn new(path: RepoPathBuf, diff_type: DiffType) -> Self {
        DiffEntry { path, diff_type }
    }
}

impl DiffType {
    /// Returns the metadata of the file in the left manifest when it exists.
    pub fn left(&self) -> Option<FileMetadata> {
        match self {
            DiffType::LeftOnly(left_metadata) => Some(*left_metadata),
            DiffType::RightOnly(_) => None,
            DiffType::Changed(left_metadata, _) => Some(*left_metadata),
        }
    }

    /// Returns the metadata of the file in the right manifest when it exists.
    pub fn right(&self) -> Option<FileMetadata> {
        match self {
            DiffType::LeftOnly(_) => None,
            DiffType::RightOnly(right_metadata) => Some(*right_metadata),
            DiffType::Changed(_, right_metadata) => Some(*right_metadata),
        }
    }
}

impl<'a, M> Iterator for Diff<'a, M>
where
    M: Matcher,
{
    type Item = Fallible<DiffEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        // This is the standard algorithm for returning the differences in two lists but adjusted
        // to have the iterator interface and to evaluate the tree lazily.

        fn diff_entry(path: &RepoPath, diff_type: DiffType) -> Option<Fallible<DiffEntry>> {
            Some(Ok(DiffEntry::new(path.to_owned(), diff_type)))
        }
        fn compare<'a>(left: &Cursor<'a>, right: &Cursor<'a>) -> Option<Ordering> {
            // TODO: cache ordering state so we compare last components at most
            match (left.finished(), right.finished()) {
                (true, true) => None,
                (false, true) => Some(Ordering::Less),
                (true, false) => Some(Ordering::Greater),
                (false, false) => Some(left.path().cmp(right.path())),
            }
        }
        fn evaluate_cursor<M: Matcher>(
            cursor: &mut Cursor<'_>,
            matcher: &M,
        ) -> Option<FileMetadata> {
            if let Leaf(file_metadata) = cursor.link() {
                if matcher.matches_file(cursor.path()) {
                    return Some(*file_metadata);
                }
            }
            try_skipping(cursor, matcher);
            None
        }

        fn try_skipping<M: Matcher>(cursor: &mut Cursor<'_>, matcher: &M) {
            if matcher.matches_directory(cursor.path()) == DirectoryMatch::Nothing {
                cursor.skip_subtree();
            }
        }
        loop {
            if self.step_left {
                if let Step::Err(error) = self.left.step() {
                    return Some(Err(error));
                }
                self.step_left = false;
            }
            if self.step_right {
                if let Step::Err(error) = self.right.step() {
                    return Some(Err(error));
                }
                self.step_right = false;
            }
            match compare(&self.left, &self.right) {
                None => return None,
                Some(Ordering::Less) => {
                    self.step_left = true;
                    if let Some(file_metadata) = evaluate_cursor(&mut self.left, &self.matcher) {
                        return diff_entry(self.left.path(), DiffType::LeftOnly(file_metadata));
                    }
                }
                Some(Ordering::Greater) => {
                    self.step_right = true;
                    if let Some(file_metadata) = evaluate_cursor(&mut self.right, &self.matcher) {
                        return diff_entry(self.right.path(), DiffType::RightOnly(file_metadata));
                    }
                }
                Some(Ordering::Equal) => {
                    self.step_left = true;
                    self.step_right = true;
                    match (self.left.link(), self.right.link()) {
                        (Leaf(left_metadata), Leaf(right_metadata)) => {
                            if left_metadata != right_metadata
                                && self.matcher.matches_file(self.left.path())
                            {
                                return diff_entry(
                                    self.left.path(),
                                    DiffType::Changed(*left_metadata, *right_metadata),
                                );
                            }
                        }
                        (Leaf(file_metadata), _) => {
                            try_skipping(&mut self.right, &self.matcher);
                            if self.matcher.matches_file(self.left.path()) {
                                return diff_entry(
                                    self.left.path(),
                                    DiffType::LeftOnly(*file_metadata),
                                );
                            }
                        }
                        (_, Leaf(file_metadata)) => {
                            try_skipping(&mut self.left, &self.matcher);
                            if self.matcher.matches_file(self.right.path()) {
                                return diff_entry(
                                    self.right.path(),
                                    DiffType::RightOnly(*file_metadata),
                                );
                            }
                        }
                        (Durable(left_entry), Durable(right_entry)) => {
                            if left_entry.node == right_entry.node
                                || self.matcher.matches_directory(self.left.path())
                                    == DirectoryMatch::Nothing
                            {
                                self.left.skip_subtree();
                                self.right.skip_subtree();
                            }
                        }
                        _ => {
                            // All other cases are two directories that we would iterate if not
                            // for the matcher
                            if self.matcher.matches_directory(self.left.path())
                                == DirectoryMatch::Nothing
                            {
                                self.left.skip_subtree();
                                self.right.skip_subtree();
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use pathmatcher::{AlwaysMatcher, TreeMatcher};
    use types::testutil::*;

    use crate::{tree::store::TestStore, Manifest};

    fn make_meta(hex: &str) -> FileMetadata {
        FileMetadata::regular(node(hex))
    }

    #[test]
    fn test_diff_generic() {
        let mut left = Tree::ephemeral(Arc::new(TestStore::new()));
        left.insert(repo_path_buf("a1/b1/c1/d1"), make_meta("10"))
            .unwrap();
        left.insert(repo_path_buf("a1/b2"), make_meta("20"))
            .unwrap();
        left.insert(repo_path_buf("a3/b1"), make_meta("40"))
            .unwrap();

        let mut right = Tree::ephemeral(Arc::new(TestStore::new()));
        right
            .insert(repo_path_buf("a1/b2"), make_meta("40"))
            .unwrap();
        right
            .insert(repo_path_buf("a2/b2/c2"), make_meta("30"))
            .unwrap();
        right
            .insert(repo_path_buf("a3/b1"), make_meta("40"))
            .unwrap();

        assert_eq!(
            Diff::new(&left, &right, &AlwaysMatcher::new())
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(
                DiffEntry::new(
                    repo_path_buf("a1/b1/c1/d1"),
                    DiffType::LeftOnly(make_meta("10"))
                ),
                DiffEntry::new(
                    repo_path_buf("a1/b2"),
                    DiffType::Changed(make_meta("20"), make_meta("40"))
                ),
                DiffEntry::new(
                    repo_path_buf("a2/b2/c2"),
                    DiffType::RightOnly(make_meta("30"))
                ),
            )
        );

        left.flush().unwrap();
        right.flush().unwrap();

        assert_eq!(
            Diff::new(&left, &right, &AlwaysMatcher::new())
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(
                DiffEntry::new(
                    repo_path_buf("a1/b1/c1/d1"),
                    DiffType::LeftOnly(make_meta("10"))
                ),
                DiffEntry::new(
                    repo_path_buf("a1/b2"),
                    DiffType::Changed(make_meta("20"), make_meta("40"))
                ),
                DiffEntry::new(
                    repo_path_buf("a2/b2/c2"),
                    DiffType::RightOnly(make_meta("30"))
                ),
            )
        );
        right
            .insert(repo_path_buf("a1/b1/c1/d1"), make_meta("10"))
            .unwrap();
        left.insert(repo_path_buf("a1/b2"), make_meta("40"))
            .unwrap();
        left.insert(repo_path_buf("a2/b2/c2"), make_meta("30"))
            .unwrap();

        assert!(Diff::new(&left, &right, &AlwaysMatcher::new())
            .next()
            .is_none());
    }

    #[test]
    fn test_diff_does_not_evaluate_durable_on_node_equality() {
        // Leaving the store empty intentionaly so that we get a panic if anything is read from it.
        let left = Tree::durable(Arc::new(TestStore::new()), node("10"));
        let right = Tree::durable(Arc::new(TestStore::new()), node("10"));
        assert!(Diff::new(&left, &right, &AlwaysMatcher::new())
            .next()
            .is_none());

        let right = Tree::durable(Arc::new(TestStore::new()), node("20"));
        assert!(Diff::new(&left, &right, &AlwaysMatcher::new())
            .next()
            .unwrap()
            .is_err());
    }

    #[test]
    fn test_diff_one_file_one_directory() {
        let mut left = Tree::ephemeral(Arc::new(TestStore::new()));
        left.insert(repo_path_buf("a1/b1"), make_meta("10"))
            .unwrap();
        left.insert(repo_path_buf("a2"), make_meta("20")).unwrap();

        let mut right = Tree::ephemeral(Arc::new(TestStore::new()));
        right.insert(repo_path_buf("a1"), make_meta("30")).unwrap();
        right
            .insert(repo_path_buf("a2/b2"), make_meta("40"))
            .unwrap();

        assert_eq!(
            Diff::new(&left, &right, &AlwaysMatcher::new())
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(
                DiffEntry::new(repo_path_buf("a1"), DiffType::RightOnly(make_meta("30"))),
                DiffEntry::new(repo_path_buf("a1/b1"), DiffType::LeftOnly(make_meta("10"))),
                DiffEntry::new(repo_path_buf("a2"), DiffType::LeftOnly(make_meta("20"))),
                DiffEntry::new(repo_path_buf("a2/b2"), DiffType::RightOnly(make_meta("40"))),
            )
        );
    }

    #[test]
    fn test_diff_left_empty() {
        let mut left = Tree::ephemeral(Arc::new(TestStore::new()));

        let mut right = Tree::ephemeral(Arc::new(TestStore::new()));
        right
            .insert(repo_path_buf("a1/b1/c1/d1"), make_meta("10"))
            .unwrap();
        right
            .insert(repo_path_buf("a1/b2"), make_meta("20"))
            .unwrap();
        right
            .insert(repo_path_buf("a2/b2/c2"), make_meta("30"))
            .unwrap();

        assert_eq!(
            Diff::new(&left, &right, &AlwaysMatcher::new())
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(
                DiffEntry::new(
                    repo_path_buf("a1/b1/c1/d1"),
                    DiffType::RightOnly(make_meta("10"))
                ),
                DiffEntry::new(repo_path_buf("a1/b2"), DiffType::RightOnly(make_meta("20"))),
                DiffEntry::new(
                    repo_path_buf("a2/b2/c2"),
                    DiffType::RightOnly(make_meta("30"))
                ),
            )
        );

        left.flush().unwrap();
        right.flush().unwrap();

        assert_eq!(
            Diff::new(&left, &right, &AlwaysMatcher::new())
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(
                DiffEntry::new(
                    repo_path_buf("a1/b1/c1/d1"),
                    DiffType::RightOnly(make_meta("10"))
                ),
                DiffEntry::new(repo_path_buf("a1/b2"), DiffType::RightOnly(make_meta("20"))),
                DiffEntry::new(
                    repo_path_buf("a2/b2/c2"),
                    DiffType::RightOnly(make_meta("30"))
                ),
            )
        );
    }

    #[test]
    fn test_diff_matcher() {
        let mut left = Tree::ephemeral(Arc::new(TestStore::new()));
        left.insert(repo_path_buf("a1/b1/c1/d1"), make_meta("10"))
            .unwrap();
        left.insert(repo_path_buf("a1/b2"), make_meta("20"))
            .unwrap();
        left.insert(repo_path_buf("a3/b1"), make_meta("40"))
            .unwrap();

        let mut right = Tree::ephemeral(Arc::new(TestStore::new()));
        right
            .insert(repo_path_buf("a1/b2"), make_meta("40"))
            .unwrap();
        right
            .insert(repo_path_buf("a2/b2/c2"), make_meta("30"))
            .unwrap();
        right
            .insert(repo_path_buf("a3/b1"), make_meta("40"))
            .unwrap();

        assert_eq!(
            Diff::new(&left, &right, &TreeMatcher::from_rules(["a1/b1"].iter()))
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(DiffEntry::new(
                repo_path_buf("a1/b1/c1/d1"),
                DiffType::LeftOnly(make_meta("10"))
            ),)
        );
        assert_eq!(
            Diff::new(&left, &right, &TreeMatcher::from_rules(["a1/b2"].iter()))
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(DiffEntry::new(
                repo_path_buf("a1/b2"),
                DiffType::Changed(make_meta("20"), make_meta("40"))
            ),)
        );
        assert_eq!(
            Diff::new(&left, &right, &TreeMatcher::from_rules(["a2/b2"].iter()))
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(DiffEntry::new(
                repo_path_buf("a2/b2/c2"),
                DiffType::RightOnly(make_meta("30"))
            ),)
        );
        assert_eq!(
            Diff::new(&left, &right, &TreeMatcher::from_rules(["*/b2"].iter()))
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(
                DiffEntry::new(
                    repo_path_buf("a1/b2"),
                    DiffType::Changed(make_meta("20"), make_meta("40"))
                ),
                DiffEntry::new(
                    repo_path_buf("a2/b2/c2"),
                    DiffType::RightOnly(make_meta("30"))
                ),
            )
        );
        assert!(
            Diff::new(&left, &right, &TreeMatcher::from_rules(["a3/**"].iter()))
                .next()
                .is_none()
        );
    }

    #[test]
    fn test_diff_on_sort_order_edge() {
        let store = Arc::new(TestStore::new());

        let mut left = Tree::ephemeral(store.clone());
        left.insert(repo_path_buf("foo/bar-test/a.txt"), make_meta("10"))
            .unwrap();
        left.insert(repo_path_buf("foo/bartest/b.txt"), make_meta("20"))
            .unwrap();

        let mut right = left.clone();
        right
            .insert(repo_path_buf("foo/bar/c.txt"), make_meta("30"))
            .unwrap();
        assert_eq!(
            Diff::new(&left, &right, &AlwaysMatcher::new())
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec![DiffEntry::new(
                repo_path_buf("foo/bar/c.txt"),
                DiffType::RightOnly(make_meta("30"))
            ),],
        );
    }
}
