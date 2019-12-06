/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::VecDeque;

use anyhow::Result;

use pathmatcher::Matcher;
use types::{Key, RepoPath, RepoPathBuf};

use crate::{
    tree::{
        link::{DurableEntry, Link},
        store::InnerStore,
        Tree,
    },
    FsNode,
};

pub struct Items<'a> {
    queue: VecDeque<(RepoPathBuf, &'a Link)>,
    store: &'a InnerStore,
    matcher: &'a dyn Matcher,
}

impl<'a> Items<'a> {
    pub fn new(tree: &'a Tree, matcher: &'a dyn Matcher) -> Self {
        Self {
            queue: vec![(RepoPathBuf::new(), &tree.root)].into(),
            store: &tree.store,
            matcher,
        }
    }

    fn prefetch(&self, extra: (&RepoPath, &DurableEntry)) -> Result<()> {
        let mut keys = vec![Key::new(extra.0.to_owned(), extra.1.hgid)];
        let mut entries = vec![extra];
        for (path, link) in self.queue.iter() {
            if let Link::Durable(durable_entry) = link {
                keys.push(Key::new(path.clone(), durable_entry.hgid));
                entries.push((path, durable_entry));
            }
        }
        self.store.prefetch(keys)?;
        for (path, entry) in entries {
            entry.materialize_links(self.store, path)?;
        }
        Ok(())
    }
}

impl<'a> Iterator for Items<'a> {
    type Item = Result<(RepoPathBuf, FsNode)>;

    fn next(&mut self) -> Option<Self::Item> {
        let (path, children, hgid) = match self.queue.pop_front() {
            None => return None,
            Some((path, link)) => match link {
                Link::Leaf(file_metadata) => return Some(Ok((path, FsNode::File(*file_metadata)))),
                Link::Ephemeral(children) => (path, children, None),
                Link::Durable(entry) => loop {
                    match entry.get_links() {
                        None => match self.prefetch((&path, &entry)) {
                            Ok(_) => (),
                            Err(e) => return Some(Err(e)),
                        },
                        Some(children_result) => match children_result {
                            Ok(children) => break (path, children, Some(entry.hgid)),
                            Err(e) => return Some(Err(e)),
                        },
                    };
                },
            },
        };
        for (component, link) in children.iter() {
            let mut child_path = path.clone();
            child_path.push(component.as_ref());
            if link.matches(&self.matcher, &child_path) {
                self.queue.push_back((child_path, &link));
            }
        }
        Some(Ok((path, FsNode::Directory(hgid))))
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
        assert_eq!(dirs(&tree, &AlwaysMatcher::new()), ["Ephemeral ''"]);
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
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            vec!(
                make_file("a1/b2", "20"),
                make_file("a2/b2/c2", "30"),
                make_file("a1/b1/c1/d1", "10"),
            )
        );

        assert_eq!(
            dirs(&tree, &AlwaysMatcher::new()),
            [
                "Ephemeral ''",
                "Ephemeral 'a1'",
                "Ephemeral 'a2'",
                "Ephemeral 'a1/b1'",
                "Ephemeral 'a2/b2'",
                "Ephemeral 'a1/b1/c1'"
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
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            vec!(
                make_file("a1/b2", "20"),
                make_file("a2/b2/c2", "30"),
                make_file("a1/b1/c1/d1", "10"),
            )
        );

        assert_eq!(
            dirs(&tree, &AlwaysMatcher::new()),
            [
                "Durable   ''",
                "Durable   'a1'",
                "Durable   'a2'",
                "Durable   'a1/b1'",
                "Durable   'a2/b2'",
                "Durable   'a1/b1/c1'"
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
            tree.files(&TreeMatcher::from_rules(["a2/b2/**"].iter()).unwrap())
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            vec!(make_file("a2/b2/c2", "30"), make_file("a2/b2/c3", "40"))
        );
        assert_eq!(
            tree.files(&TreeMatcher::from_rules(["a1/*/c1/**"].iter()).unwrap())
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            vec!(make_file("a1/b1/c1/d1", "10"),)
        );
        assert_eq!(
            tree.files(&TreeMatcher::from_rules(["**/c3"].iter()).unwrap())
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            vec!(make_file("a2/b2/c3", "40"), make_file("a3/b2/c3", "50"))
        );

        // A prefix matcher works as expected.
        assert_eq!(
            dirs(&tree, &TreeMatcher::from_rules(["a1/**"].iter()).unwrap()),
            [
                "Ephemeral ''",
                "Ephemeral 'a1'",
                "Ephemeral 'a1/b1'",
                "Ephemeral 'a1/b1/c1'"
            ]
        );

        // A suffix matcher is not going to be effective.
        assert_eq!(
            dirs(&tree, &TreeMatcher::from_rules(["**/c2"].iter()).unwrap()),
            [
                "Ephemeral ''",
                "Ephemeral 'a1'",
                "Ephemeral 'a2'",
                "Ephemeral 'a3'",
                "Ephemeral 'a1/b1'",
                "Ephemeral 'a2/b2'",
                "Ephemeral 'a3/b2'",
                "Ephemeral 'a1/b1/c1'"
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
            .collect::<Result<Vec<_>>>();
        assert!(files_result.is_err());
    }

    fn dirs(tree: &Tree, matcher: &dyn Matcher) -> Vec<String> {
        tree.dirs(&matcher)
            .map(|t| {
                let t = t.unwrap();
                format!(
                    "{:9} '{}'",
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
