/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{collections::VecDeque, mem};

use failure::Fallible;

use pathmatcher::{DirectoryMatch, Matcher};
use types::{HgId, RepoPathBuf};

use crate::{
    tree::{store::InnerStore, Directory, Tree},
    File,
};

pub struct Items<'a, T> {
    output: VecDeque<T>,
    current: VecDeque<Directory<'a>>,
    next: VecDeque<Directory<'a>>,
    store: &'a InnerStore,
    matcher: &'a dyn Matcher,
}

// This is a subset of Directory<'a> that hides the internal "link" field.
/// Directory information.
pub struct DirInfo {
    pub path: RepoPathBuf,
    pub hgid: Option<HgId>,
}

pub type Files<'a> = Items<'a, File>;
pub type Dirs<'a> = Items<'a, DirInfo>;

impl<'a, T: ItemOutput<'a>> Items<'a, T> {
    pub fn new(tree: &'a Tree, matcher: &'a dyn Matcher) -> Self {
        let root = Directory::from_root(&tree.root).expect("manifest root is not a directory");
        Self {
            output: VecDeque::new(),
            current: vec![root].into(),
            next: VecDeque::new(),
            store: &tree.store,
            matcher,
        }
    }

    fn process_next_dir(&mut self) -> Fallible<bool> {
        // If we've finished processing all directories in the current layer of the tree,
        // proceed to the next layer, prefetching all of the tree nodes in that layer
        // prior to traversing the corresponding directories.
        if self.current.is_empty() {
            self.prefetch()?;
            mem::swap(&mut self.current, &mut self.next);
        }

        let (files, dirs) = match self.current.pop_front() {
            Some(dir) => dir.list(&self.store)?,
            None => return Ok(false),
        };

        // Use the matcher to determine which files to output and which directories to visit.
        let matcher = self.matcher;
        let files = files.into_iter().filter(|f| matcher.matches_file(&f.path));
        let dirs: Vec<_> = dirs
            .into_iter()
            .filter(|d| matcher.matches_directory(&d.path) != DirectoryMatch::Nothing)
            .collect();

        T::extend_output(self, files, &dirs);
        self.next.extend(dirs);
        Ok(true)
    }

    /// Prefetch tree nodes for all directories in the next layer of the traversal.
    fn prefetch(&self) -> Fallible<()> {
        let keys = self.next.iter().filter_map(|d| d.key()).collect::<Vec<_>>();
        self.store.prefetch(keys)
    }
}

impl<'a, T: ItemOutput<'a>> Iterator for Items<'a, T> {
    type Item = Fallible<T>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.output.is_empty() {
            match self.process_next_dir() {
                Ok(true) => continue,
                Ok(false) => break,
                Err(e) => return Some(Err(e)),
            }
        }
        self.output.pop_front().map(Ok)
    }
}

pub trait ItemOutput<'a>: Sized {
    fn extend_output(
        items: &mut Items<'a, Self>,
        files: impl IntoIterator<Item = File>,
        dirs: &[Directory<'a>],
    );
}

impl<'a> ItemOutput<'a> for File {
    fn extend_output(
        items: &mut Items<'a, Self>,
        files: impl IntoIterator<Item = File>,
        _dirs: &[Directory<'a>],
    ) {
        items.output.extend(files);
    }
}

impl<'a> ItemOutput<'a> for DirInfo {
    fn extend_output(
        items: &mut Items<'a, Self>,
        _files: impl IntoIterator<Item = File>,
        dirs: &[Directory<'a>],
    ) {
        items.output.extend(dirs.iter().map(|d| DirInfo {
            path: d.path.clone(),
            hgid: d.hgid.clone(),
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use pathmatcher::{AlwaysMatcher, TreeMatcher};
    use types::testutil::*;

    use crate::tree::{store::TestStore, testutil::*, Manifest};

    #[test]
    fn test_items_empty() {
        let tree = Tree::ephemeral(Arc::new(TestStore::new()));
        assert!(tree.files(&AlwaysMatcher::new()).next().is_none());
        assert!(tree.dirs(&AlwaysMatcher::new()).next().is_none());
    }

    #[test]
    fn test_items_ephemeral() {
        let mut tree = Tree::ephemeral(Arc::new(TestStore::new()));
        tree.insert(repo_path_buf("a1/b1/c1/d1"), make_meta("10"))
            .unwrap();
        tree.insert(repo_path_buf("a1/b2"), make_meta("20"))
            .unwrap();
        tree.insert(repo_path_buf("a2/b2/c2"), make_meta("30"))
            .unwrap();

        assert_eq!(
            tree.files(&AlwaysMatcher::new())
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(
                (repo_path_buf("a1/b2"), make_meta("20")).into(),
                (repo_path_buf("a2/b2/c2"), make_meta("30")).into(),
                (repo_path_buf("a1/b1/c1/d1"), make_meta("10")).into(),
            )
        );

        assert_eq!(
            dirs(&tree, &AlwaysMatcher::new()),
            [
                "Ephemeral a1",
                "Ephemeral a2",
                "Ephemeral a1/b1",
                "Ephemeral a2/b2",
                "Ephemeral a1/b1/c1"
            ]
        );
    }

    #[test]
    fn test_items_durable() {
        let store = Arc::new(TestStore::new());
        let mut tree = Tree::ephemeral(store.clone());
        tree.insert(repo_path_buf("a1/b1/c1/d1"), make_meta("10"))
            .unwrap();
        tree.insert(repo_path_buf("a1/b2"), make_meta("20"))
            .unwrap();
        tree.insert(repo_path_buf("a2/b2/c2"), make_meta("30"))
            .unwrap();
        let hgid = tree.flush().unwrap();
        let tree = Tree::durable(store.clone(), hgid);

        assert_eq!(
            tree.files(&AlwaysMatcher::new())
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(
                (repo_path_buf("a1/b2"), make_meta("20")).into(),
                (repo_path_buf("a2/b2/c2"), make_meta("30")).into(),
                (repo_path_buf("a1/b1/c1/d1"), make_meta("10")).into(),
            )
        );

        assert_eq!(
            dirs(&tree, &AlwaysMatcher::new()),
            [
                "Durable   a1",
                "Durable   a2",
                "Durable   a1/b1",
                "Durable   a2/b2",
                "Durable   a1/b1/c1"
            ]
        );
    }

    #[test]
    fn test_items_matcher() {
        let mut tree = Tree::ephemeral(Arc::new(TestStore::new()));
        tree.insert(repo_path_buf("a1/b1/c1/d1"), make_meta("10"))
            .unwrap();
        tree.insert(repo_path_buf("a1/b2"), make_meta("20"))
            .unwrap();
        tree.insert(repo_path_buf("a2/b2/c2"), make_meta("30"))
            .unwrap();
        tree.insert(repo_path_buf("a2/b2/c3"), make_meta("40"))
            .unwrap();
        tree.insert(repo_path_buf("a3/b2/c3"), make_meta("50"))
            .unwrap();

        assert_eq!(
            tree.files(&TreeMatcher::from_rules(["a2/b2/**"].iter()))
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(
                (repo_path_buf("a2/b2/c2"), make_meta("30")).into(),
                (repo_path_buf("a2/b2/c3"), make_meta("40")).into()
            )
        );
        assert_eq!(
            tree.files(&TreeMatcher::from_rules(["a1/*/c1/**"].iter()))
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!((repo_path_buf("a1/b1/c1/d1"), make_meta("10")).into(),)
        );
        assert_eq!(
            tree.files(&TreeMatcher::from_rules(["**/c3"].iter()))
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(
                (repo_path_buf("a2/b2/c3"), make_meta("40").into()).into(),
                (repo_path_buf("a3/b2/c3"), make_meta("50").into()).into()
            )
        );

        // A prefix matcher works as expected.
        assert_eq!(
            dirs(&tree, &TreeMatcher::from_rules(["a1/**"].iter())),
            ["Ephemeral a1", "Ephemeral a1/b1", "Ephemeral a1/b1/c1"]
        );

        // A suffix matcher is not going to be effective.
        assert_eq!(
            dirs(&tree, &TreeMatcher::from_rules(["**/c2"].iter())),
            [
                "Ephemeral a1",
                "Ephemeral a2",
                "Ephemeral a3",
                "Ephemeral a1/b1",
                "Ephemeral a2/b2",
                "Ephemeral a3/b2",
                "Ephemeral a1/b1/c1"
            ]
        );
    }

    #[test]
    fn test_files_finish_on_error_when_collecting_to_vec() {
        let tree = Tree::durable(Arc::new(TestStore::new()), hgid("1"));
        let file_results = tree.files(&AlwaysMatcher::new()).collect::<Vec<_>>();
        assert_eq!(file_results.len(), 1);
        assert!(file_results[0].is_err());

        let files_result = tree
            .files(&AlwaysMatcher::new())
            .collect::<Result<Vec<_>, _>>();
        assert!(files_result.is_err());
    }

    fn dirs(tree: &Tree, matcher: &dyn Matcher) -> Vec<String> {
        tree.dirs(&matcher)
            .map(|t| {
                let t = t.unwrap();
                format!(
                    "{:9} {}",
                    if t.hgid.is_some() {
                        "Durable"
                    } else {
                        "Ephemeral"
                    },
                    t.path
                )
            })
            .collect::<Vec<_>>()
    }

}
