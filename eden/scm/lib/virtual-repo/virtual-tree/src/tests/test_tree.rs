/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::num::NonZeroU64;
use std::sync::Arc;

use crate::types::*;

/// Implements [`VirtualTreeProvider`] for unit tests.
/// Trees can be built from sapling status-like information.
/// See [`TestTree::new`].
#[derive(Default)]
pub struct TestTree {
    trees: Vec<TestTreeBody>,
    root_tree_ids: Vec<TreeId>,
}

#[derive(Default)]
struct TestTreeBody {
    seed: TreeSeed,
    items: Arc<Vec<(NameId, ContentId)>>,
}

impl TestTree {
    /// `changes` specifies file path changes in `sl status` format.
    /// Example: `[[('A', path1), ('A', path2)], [('M', path1), ('R', path2)]]`.
    /// Meaning: The 1st commit adds path1 and path2. The 2nd commit modifies
    /// path1 and removes path2.
    pub fn new(changes: &[&[(char, &str)]]) -> Self {
        let mut root_tree = Tree::default();
        let mut trees = Vec::new();
        let mut root_tree_ids = Vec::with_capacity(changes.len());
        // Reverse of `trees`. Used to de-duplicate trees.
        let mut trees_reversed = HashMap::new();
        // Used to assign `seed` for the same path (same Vec<NameId>).
        let mut path_to_seed: HashMap<Vec<NameId>, TreeSeed> = HashMap::new();
        for change in changes {
            for (status, path) in *change {
                match status {
                    'A' | 'M' => root_tree.modify_path(path),
                    'R' => {
                        root_tree.delete_path(path);
                    }
                    _ => panic!("unknown status: {status} (expected: A, M, R; path: {path})"),
                }
            }
            let mut path = Vec::new();
            let tree_id = root_tree.commit(
                &mut trees,
                &mut trees_reversed,
                &mut path_to_seed,
                &mut path,
            );
            root_tree_ids.push(tree_id);
        }
        Self {
            trees,
            root_tree_ids,
        }
    }
}

impl VirtualTreeProvider for TestTree {
    fn read_tree(
        &self,
        tree_id: crate::types::TreeId,
    ) -> Box<dyn Iterator<Item = (crate::types::NameId, crate::types::ContentId)> + '_> {
        Box::new(
            self.trees[tree_id.0.get() as usize - 1]
                .items
                .iter()
                .copied(),
        )
    }

    fn get_tree_seed(&self, tree_id: TreeId) -> TreeSeed {
        self.trees[tree_id.0.get() as usize - 1].seed
    }

    fn root_tree_len(&self) -> usize {
        self.root_tree_ids.len()
    }

    fn root_tree_id(&self, index: usize) -> crate::types::TreeId {
        self.root_tree_ids[index]
    }
}

enum Either<A, B> {
    A(A),
    B(B),
}

/// Used by [`TestTree::new`]` internally.
#[derive(Default)]
struct Tree<'a> {
    inner: BTreeMap<&'a str, Either<Tree<'a>, BlobId>>,
    id: Option<TreeId>,
}

impl<'a> Tree<'a> {
    /// Modify a (file) path. Creates the file path and parent trees on demand.
    fn modify_path(&mut self, path: &'a str) {
        self.id = None;

        match path.split_once('/') {
            Some((name, rest)) => {
                let v = self
                    .inner
                    .entry(name)
                    .or_insert_with(|| Either::A(Tree::default()));
                match v {
                    Either::A(v) => v.modify_path(rest),
                    Either::B(v) => panic!("modify_path: expect tree, got file {:?}", v),
                };
            }
            None => {
                self.inner
                    .entry(path)
                    .and_modify(|v| match v {
                        Either::A(_v) => panic!("modify_path: expect file, got tree"),
                        Either::B(v) => {
                            *v = BlobId(v.0.checked_add(1).unwrap());
                        }
                    })
                    .or_insert_with(|| Either::B(BlobId(NonZeroU64::new(1).unwrap())));
            }
        }
    }

    /// Delete a (file) path.
    /// Delete empty trees caused by file deletion.
    /// Returns true if the `self` tree becomes empty.
    fn delete_path(&mut self, path: &'a str) -> bool {
        self.id = None;

        let (name, rest) = path.split_once('/').unwrap_or((path, ""));
        let tree = self
            .inner
            .get_mut(name)
            .expect("delete_path: path should exist");
        match (tree, rest) {
            (Either::A(_subtree), "") => {
                panic!("delete_path: expect file, got tree");
            }
            (Either::A(subtree), rest) => {
                if subtree.delete_path(rest) {
                    // Delete the empty subtree too.
                    self.inner.remove(name);
                }
            }
            (Either::B(_), "") => {
                self.inner.remove(name);
            }
            (Either::B(_), _rest) => {
                panic!("delete_path: expect tree, got file");
            }
        }

        self.inner.is_empty()
    }

    /// Commit changed trees (`id = None`). Returns the committed TreeId.
    /// If the tree is not modified, return its previously committed TreeId.
    fn commit(
        &mut self,
        trees: &mut Vec<TestTreeBody>,
        trees_reversed: &mut HashMap<Arc<Vec<(NameId, ContentId)>>, TreeId>,
        path_to_seed: &mut HashMap<Vec<NameId>, TreeSeed>,
        path: &mut Vec<NameId>,
    ) -> TreeId {
        if let Some(id) = self.id {
            return id;
        }

        let seed = match path_to_seed.get(&*path) {
            Some(v) => *v,
            None => {
                let seed = TreeSeed(path_to_seed.len() as u64);
                path_to_seed.insert(path.clone(), seed);
                seed
            }
        };

        // Commit subtrees.
        let mut entries = Vec::with_capacity(self.inner.len());
        for (name, value) in self.inner.iter_mut() {
            let name_id = NameId(NonZeroU64::new(name.parse::<u64>().unwrap()).unwrap());
            let content_id = match value {
                Either::A(subtree) => {
                    path.push(name_id);
                    let tree_id = subtree.commit(trees, trees_reversed, path_to_seed, path);
                    path.pop();
                    ContentId::from(TypedContentId::Tree(tree_id))
                }
                Either::B(blob_id) => {
                    ContentId::from(TypedContentId::File(*blob_id, FileMode::Regular))
                }
            };
            entries.push((name_id, content_id));
        }
        let tree_items = Arc::new(entries);

        // Reuse an existing tree? Note: seed might conflict in this case, but
        // that's intentional to make the test case more interesting. For
        // example, split_changes needs to split the same tree appeared multiple
        // times separately.
        match trees_reversed.get(&tree_items) {
            Some(&tree_id) => {
                self.id = Some(tree_id);
                tree_id
            }
            None => {
                let tree_body = TestTreeBody {
                    items: tree_items.clone(),
                    seed,
                };
                trees.push(tree_body);
                let tree_id = TreeId(NonZeroU64::new(trees.len() as u64).unwrap());
                trees_reversed.insert(tree_items, tree_id);
                self.id = Some(tree_id);
                tree_id
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::ShowTree;
    use crate::tests::TestTree;

    #[test]
    fn test_basic_test_tree() {
        let root_tree = TestTree::new(&[
            &[
                ('A', "1/1/1"),
                ('A', "1/2/2"),
                ('A', "2/1/3"),
                ('A', "2/2/4"),
            ],
            &[('M', "1/1/1"), ('A', "1/2/3")],
            &[('R', "2/2/4"), ('R', "2/1/3")],
        ]);

        // Root tree2 should assign 1/1 as #8, not #9.
        // Root tree3 should have 2/ removed.
        assert_eq!(
            root_tree.show_root_trees(),
            r#"
            Root tree 1:         #7  seed=0
              1/                 #3  seed=1
                1/               #1  seed=2
                  1 = 1
                2/               #2  seed=3
                  2 = 1
              2/                 #6  seed=4
                1/               #4  seed=5
                  3 = 1
                2/               #5  seed=6
                  4 = 1
            Root tree 2:         #11 seed=0
              1/                 #10 seed=1
                1/               #8  seed=2
                  1 = 2
                2/               #9  seed=3
                  2 = 1
                  3 = 1
              2/                 #6  seed=4
                1/               #4  seed=5
                  3 = 1
                2/               #5  seed=6
                  4 = 1
            Root tree 3:         #12 seed=0
              1/                 #10 seed=1
                1/               #8  seed=2
                  1 = 2
                2/               #9  seed=3
                  2 = 1
                  3 = 1"#
        );
    }

    #[test]
    fn test_same_tree_reuse() {
        let root_tree = TestTree::new(&[
            &[
                ('A', "1/1/1"),
                ('A', "1/2/1"),
                ('A', "2/1/1"),
                ('A', "2/2/1"),
                ('A', "3/1"),
                ('A', "3/2"),
            ],
            &[('A', "1/1/2"), ('R', "3/2")],
        ]);

        // Root tree 1 should have the same TreeId for:
        // - 1/1, 1/2, 2/1, 2/2 (same tree with "1"). Uses #1.
        // - 1/ and 2/ (same tree with "1/1, 2/1"). Uses #2.
        //
        // Root tree 2 should reuse the TreeId in Root tree 1 for:
        // - 1/1: matches 3/ in Root tree 1. Resues #3.
        // - 3/1: matches 1/1 in Root tree 1. Reuses #1.
        assert_eq!(
            root_tree.show_root_trees(),
            r#"
            Root tree 1:         #4  seed=0
              1/                 #2  seed=1
                1/               #1  seed=2
                  1 = 1
                2/               #1  seed=2
                  1 = 1
              2/                 #2  seed=1
                1/               #1  seed=2
                  1 = 1
                2/               #1  seed=2
                  1 = 1
              3/                 #3  seed=7
                1 = 1
                2 = 1
            Root tree 2:         #6  seed=0
              1/                 #5  seed=1
                1/               #3  seed=7
                  1 = 1
                  2 = 1
                2/               #1  seed=2
                  1 = 1
              2/                 #2  seed=1
                1/               #1  seed=2
                  1 = 1
                2/               #1  seed=2
                  1 = 1
              3/                 #1  seed=2
                1 = 1"#
        );
    }
}
