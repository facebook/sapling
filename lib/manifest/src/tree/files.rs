// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{collections::VecDeque, mem};

use failure::Fallible;

use pathmatcher::{DirectoryMatch, Matcher};

use crate::{
    tree::{store::InnerStore, Directory, Tree},
    File,
};

pub struct Files<'a> {
    output: VecDeque<File>,
    current: VecDeque<Directory<'a>>,
    next: VecDeque<Directory<'a>>,
    store: &'a InnerStore,
    matcher: &'a dyn Matcher,
}

impl<'a> Files<'a> {
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
        let dirs = dirs
            .into_iter()
            .filter(|d| matcher.matches_directory(&d.path) != DirectoryMatch::Nothing);

        self.output.extend(files);
        self.next.extend(dirs);
        Ok(true)
    }

    /// Prefetch tree nodes for all directories in the next layer of the traversal.
    fn prefetch(&self) -> Fallible<()> {
        let keys = self.next.iter().filter_map(|d| d.key()).collect::<Vec<_>>();
        self.store.prefetch(keys)
    }
}

impl<'a> Iterator for Files<'a> {
    type Item = Fallible<File>;

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

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use pathmatcher::{AlwaysMatcher, TreeMatcher};
    use types::testutil::*;

    use crate::tree::{store::TestStore, testutil::*, Manifest};

    #[test]
    fn test_files_empty() {
        let tree = Tree::ephemeral(Arc::new(TestStore::new()));
        assert!(tree.files(&AlwaysMatcher::new()).next().is_none());
    }

    #[test]
    fn test_files_ephemeral() {
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
    }

    #[test]
    fn test_files_durable() {
        let store = Arc::new(TestStore::new());
        let mut tree = Tree::ephemeral(store.clone());
        tree.insert(repo_path_buf("a1/b1/c1/d1"), make_meta("10"))
            .unwrap();
        tree.insert(repo_path_buf("a1/b2"), make_meta("20"))
            .unwrap();
        tree.insert(repo_path_buf("a2/b2/c2"), make_meta("30"))
            .unwrap();
        let node = tree.flush().unwrap();
        let tree = Tree::durable(store.clone(), node);

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
    }

    #[test]
    fn test_files_matcher() {
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
            tree.files(&TreeMatcher::from_rules(["a2/b2"].iter()))
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(
                (repo_path_buf("a2/b2/c2"), make_meta("30")).into(),
                (repo_path_buf("a2/b2/c3"), make_meta("40")).into()
            )
        );
        assert_eq!(
            tree.files(&TreeMatcher::from_rules(["a1/*/c1"].iter()))
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
    }

    #[test]
    fn test_files_finish_on_error_when_collecting_to_vec() {
        let tree = Tree::durable(Arc::new(TestStore::new()), node("1"));
        let file_results = tree.files(&AlwaysMatcher::new()).collect::<Vec<_>>();
        assert_eq!(file_results.len(), 1);
        assert!(file_results[0].is_err());

        let files_result = tree
            .files(&AlwaysMatcher::new())
            .collect::<Result<Vec<_>, _>>();
        assert!(files_result.is_err());
    }
}
