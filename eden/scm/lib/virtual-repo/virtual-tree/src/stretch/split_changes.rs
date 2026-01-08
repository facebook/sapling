/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::cmp::Ordering;
use std::collections::HashMap;
use std::num::NonZeroU64;
use std::ops::Range;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::types::*;

/// Split changes. 1 change -> `1 << split_bits` changes.
/// Might produce empty changes (empty commit).
#[derive(Clone)]
pub struct SplitChanges {
    root_tree_bits: u8,
    split_bits: u8,
    original: Arc<dyn VirtualTreeProvider>,
    cache: Arc<RwLock<HashMap<CacheId, GeneratedTrees>>>,
}

// Under the hood, there are 2 kinds of tree_ids:
//
// ```ignore
// The lowest bit is used to tag whether the TreeId is original or generated.
//
// tree_id | 0b0
//         | 1 bit (bit length)
// Use the original tree as-is.
//
// split_tree_index | root_tree_index | 0b1
//                  | root_tree_bits  | 1 bit (bit length)
// Use split_tree_index for the root_tree_index to root_tree_index+1 split.
// ```
//
// The `split_tree_index` starts from 0, and is scoped to the specific
// `root_tree_index`. Note `tree_id | root_tree_index | split_id` is not
// a good choice because one `tree_id` might be shared by 2 different paths
// that need to be split differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeneratedTreeId {
    Original(TreeId),
    Generated {
        /// Before-split root tree index.
        root_tree_index: usize,
        /// Split index. 0 .. (1 << split_bits)
        split_tree_index: usize,
    },
}

/// Tracks diff of 2 root trees recursively.
/// This forms a tree structure of all changed paths regardless of whether the
/// path is a file or a tree. A path is represented by a list of `NameId`s.
/// This is used as an intermediate state to calculate GeneratedTrees.
#[derive(Debug)]
struct TreeDiffNode {
    /// Before-split tree diff at this path.
    subtree: HashMap<NameId, Box<TreeDiffNode>>,
    /// Before-split `ContentId` at this path.
    content_id1: ContentId,
    /// Before-split `ContentId` at this path.
    content_id2: ContentId,
    /// Assigned id for blob change at this path. Useful to split changes
    /// (ex. moving edit_id < threshold edits to a change).
    edit_id: Option<usize>,
    /// Range of the edit_id, recursively.
    edit_id_range: Range<usize>,
}

// Stores the result of "generated" trees.
#[derive(Debug)]
struct GeneratedTrees {
    /// After-split trees that have to be generated, cannot be reused in the
    /// original tree provider.
    trees: Vec<GeneratedTreeBody>,
}

#[derive(Debug, Default, Clone)]
struct GeneratedTreeBody {
    seed: TreeSeed,
    items: Arc<Vec<(NameId, ContentId)>>,
}

// The generated trees come from diff calculation, which can be expensive.
// So we need to cache the calculation. The cache is keyed by before-split
// root_tree_index.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub(crate) struct CacheId {
    // Before-split root-tree index.
    root_tree_index: usize,
}

const DUMMY_ITEM: (NameId, ContentId) = (NameId(NonZeroU64::MAX), ContentId::ABSENT);

impl TreeDiffNode {
    fn new(
        provider: &dyn VirtualTreeProvider,
        content_id1: ContentId,
        content_id2: ContentId,
        next_edit_id: &mut usize,
    ) -> Self {
        // Assign `edit_id` if one side is a blob.
        let min_edit_id = *next_edit_id;
        let edit_id = if content_id1.tree_id().is_none() && content_id2.tree_id().is_none() {
            let id = Some(*next_edit_id);
            *next_edit_id += 1;
            id
        } else {
            None
        };
        let mut node = Self {
            subtree: HashMap::default(),
            content_id1,
            content_id2,
            edit_id,
            edit_id_range: min_edit_id..min_edit_id, // temporary
        };
        node.populate_subtree(provider, next_edit_id);
        let max_edit_id = *next_edit_id;
        node.edit_id_range.end = max_edit_id;
        node
    }

    // Calculate the subtree field recursively. Updates `next_edit_id`.
    fn populate_subtree(&mut self, provider: &dyn VirtualTreeProvider, next_edit_id: &mut usize) {
        let mut items1 = maybe_read_tree(provider, self.content_id1.tree_id());
        let mut items2 = maybe_read_tree(provider, self.content_id2.tree_id());

        let (mut name1, mut content1) = items1.next().unwrap_or(DUMMY_ITEM);
        let (mut name2, mut content2) = items2.next().unwrap_or(DUMMY_ITEM);
        let mut last_name1 = 0;
        let mut last_name2 = 0;

        loop {
            assert!(
                last_name1 <= name1.0.get() && last_name2 <= name2.0.get(),
                "read_tree iterator should produce NameId in ASC order"
            );
            match name1.cmp(&name2) {
                Ordering::Less => {
                    // name1 is removed.
                    let node =
                        TreeDiffNode::new(provider, content1, ContentId::ABSENT, next_edit_id);
                    self.subtree.insert(name1, Box::new(node));
                    (name1, content1) = items1.next().unwrap_or(DUMMY_ITEM);
                }
                Ordering::Equal => {
                    if name1 == DUMMY_ITEM.0 {
                        break;
                    } else if content1 != content2 {
                        // name1 (and name2) is modified.
                        let node = TreeDiffNode::new(provider, content1, content2, next_edit_id);
                        self.subtree.insert(name1, Box::new(node));
                    }
                    (name1, content1) = items1.next().unwrap_or(DUMMY_ITEM);
                    (name2, content2) = items2.next().unwrap_or(DUMMY_ITEM);
                }
                Ordering::Greater => {
                    // name2 is added.
                    let node =
                        TreeDiffNode::new(provider, ContentId::ABSENT, content2, next_edit_id);
                    self.subtree.insert(name2, Box::new(node));
                    (name2, content2) = items2.next().unwrap_or(DUMMY_ITEM);
                }
            }
            last_name1 = name1.0.get();
            last_name2 = name2.0.get();
        }
    }

    /// Pick changes based on edit_id (edit_id < cutoff_edit_id => pick 1;
    /// otherwise pick 2) and generate trees for it.
    /// If `preallocated_index` is Some(i), write to out_trees[i].
    /// Return (generated) after-split ContentId.
    fn generate_trees(
        &self,
        split_changes: &SplitChanges,
        root_tree_index: usize,
        cutoff_edit_id: usize,
        out_trees: &mut Vec<GeneratedTreeBody>,
        preallocated_index: Option<usize>,
    ) -> ContentId {
        if preallocated_index.is_none() {
            if self.edit_id_range.start >= cutoff_edit_id {
                // No edits are included.
                return self.content_id1.from_before_split_to_after_split();
            } else if self.edit_id_range.end <= cutoff_edit_id {
                // All edits are included.
                return self.content_id2.from_before_split_to_after_split();
            }
        }

        let tree_id1 = self.content_id1.tree_id();
        let tree_id2 = self.content_id2.tree_id();

        match (tree_id1, tree_id2, self.edit_id) {
            (None, None, Some(edit_id)) => {
                // File -> file.
                if edit_id < cutoff_edit_id {
                    self.content_id2.from_before_split_to_after_split()
                } else {
                    self.content_id1.from_before_split_to_after_split()
                }
            }
            (_, _, _) => {
                let mut items1 = maybe_read_tree(split_changes.original.as_ref(), tree_id1);
                let mut items2 = maybe_read_tree(split_changes.original.as_ref(), tree_id2);

                let mut new_items = Vec::new();
                let (mut name1, mut content1) = items1.next().unwrap_or(DUMMY_ITEM);
                let (mut name2, mut content2) = items2.next().unwrap_or(DUMMY_ITEM);

                loop {
                    let mut consider_name_id = None;
                    match name1.cmp(&name2) {
                        Ordering::Less => {
                            consider_name_id = Some(name1);
                            (name1, content1) = items1.next().unwrap_or(DUMMY_ITEM);
                        }
                        Ordering::Equal => {
                            if name1 == DUMMY_ITEM.0 {
                                break;
                            } else if content1 != content2 {
                                consider_name_id = Some(name1);
                            } else {
                                if !content1.is_absent() {
                                    new_items
                                        .push((name1, content1.from_before_split_to_after_split()));
                                }
                            }
                            (name1, content1) = items1.next().unwrap_or(DUMMY_ITEM);
                            (name2, content2) = items2.next().unwrap_or(DUMMY_ITEM);
                        }
                        Ordering::Greater => {
                            consider_name_id = Some(name2);
                            (name2, content2) = items2.next().unwrap_or(DUMMY_ITEM);
                        }
                    }
                    if let Some(name_id) = consider_name_id {
                        let content_id = self.subtree[&name_id].generate_trees(
                            split_changes,
                            root_tree_index,
                            cutoff_edit_id,
                            out_trees,
                            None,
                        );
                        if !content_id.is_absent() {
                            new_items.push((name_id, content_id));
                        }
                    }
                }

                let tree_body = GeneratedTreeBody {
                    items: Arc::new(new_items),
                    seed: {
                        let original_tree_id = tree_id1.or(tree_id2).unwrap();
                        split_changes.original.get_tree_seed(original_tree_id)
                    },
                };
                let split_tree_index = match preallocated_index {
                    Some(i) => {
                        out_trees[i] = tree_body;
                        i
                    }
                    None => {
                        if tree_body.items.is_empty() {
                            return ContentId::ABSENT;
                        }
                        let i = out_trees.len();
                        out_trees.push(tree_body);
                        i
                    }
                };

                assert!(root_tree_index > 0);
                let tree_id = split_changes.unparse_tree_id(GeneratedTreeId::Generated {
                    root_tree_index,
                    split_tree_index,
                });
                TypedContentId::Tree(tree_id).into()
            }
        }
    }
}

impl GeneratedTrees {
    /// root_tree_index: index before-split.
    fn new(split_changes: &SplitChanges, root_tree_index: usize) -> Self {
        assert!(root_tree_index > 0);
        assert!(root_tree_index < split_changes.original.root_tree_len());
        let mut trees = Vec::new();
        let tree_diff = {
            let tree1 = split_changes.original.root_tree_id(root_tree_index - 1);
            let tree2 = split_changes.original.root_tree_id(root_tree_index);
            let mut next_edit_id = 0;
            TreeDiffNode::new(
                split_changes.original.as_ref(),
                TypedContentId::Tree(tree1).into(),
                TypedContentId::Tree(tree2).into(),
                &mut next_edit_id,
            )
        };

        // trees[0..split_len] are reserved for root trees.
        let split_len = 1usize.lossless_shl(split_changes.split_bits);
        trees.resize(split_len, Default::default());
        for i in 0..split_len {
            // edit_id < cutoff should be included in this change.
            // edit_id_range.end is exclusive.
            let cutoff = tree_diff.edit_id_range.end * (i + 1) / split_len;
            let _root_content_id = tree_diff.generate_trees(
                split_changes,
                root_tree_index,
                cutoff,
                &mut trees,
                Some(i),
            );
        }

        Self { trees }
    }
}

impl ContentId {
    fn tree_id(self) -> Option<TreeId> {
        match TypedContentId::from(self) {
            TypedContentId::Tree(tree_id) => Some(tree_id),
            TypedContentId::File(..) => None,
            TypedContentId::Absent => None,
        }
    }
    fn from_before_split_to_after_split(self) -> Self {
        match self.tree_id() {
            Some(tree_id) => TypedContentId::Tree(TreeId(tree_id.0.lossless_shl(1))).into(),
            None => self,
        }
    }
}

impl SplitChanges {
    /// TreeId -> GeneratedTreeId
    fn parse_tree_id(&self, tree_id: TreeId) -> GeneratedTreeId {
        let value = tree_id.0.get();
        if value & 1 == 0 {
            GeneratedTreeId::Original(TreeId(NonZeroU64::new(tree_id.0.get() >> 1).unwrap()))
        } else {
            let value = value >> 1;
            let root_tree_index = (value & (1u64.lossless_shl(self.root_tree_bits) - 1)) as usize;
            assert!(root_tree_index > 0);
            assert!(root_tree_index < self.original.root_tree_len());
            let generated_tree_index = (value >> self.root_tree_bits) as usize;
            GeneratedTreeId::Generated {
                root_tree_index,
                split_tree_index: generated_tree_index,
            }
        }
    }

    /// GeneratedTreeId -> TreeId
    fn unparse_tree_id(&self, generated_tree_id: GeneratedTreeId) -> TreeId {
        let value = match generated_tree_id {
            GeneratedTreeId::Original(tree_id) => tree_id.0.get().lossless_shl(1),
            GeneratedTreeId::Generated {
                root_tree_index,
                split_tree_index: generated_tree_index,
            } => {
                assert!(root_tree_index > 0);
                assert!(root_tree_index < self.original.root_tree_len());
                (((generated_tree_index as u64).lossless_shl(self.root_tree_bits)
                    | (root_tree_index as u64))
                    .lossless_shl(1))
                    | 1
            }
        };
        let tree_id = TreeId(NonZeroU64::new(value).unwrap());
        debug_assert_eq!(generated_tree_id, self.parse_tree_id(tree_id));
        tree_id
    }
}

impl VirtualTreeProvider for SplitChanges {
    fn read_tree<'a>(&'a self, tree_id: TreeId) -> ReadTreeIter<'a> {
        match self.parse_tree_id(tree_id) {
            GeneratedTreeId::Original(tree_id) => Box::new(self.original.read_tree(tree_id).map(
                |(name_id, content_id)| (name_id, content_id.from_before_split_to_after_split()),
            )),
            GeneratedTreeId::Generated {
                root_tree_index,
                split_tree_index: generated_tree_index,
            } => {
                let tree_body = self.with_cached_tree_for_root_tree_index(root_tree_index, |g| {
                    g.trees[generated_tree_index].clone()
                });
                let iter = ArcVecIter(tree_body.items.clone(), 0);
                Box::new(iter)
            }
        }
    }

    fn get_tree_seed(&self, tree_id: TreeId) -> TreeSeed {
        match self.parse_tree_id(tree_id) {
            GeneratedTreeId::Original(tree_id) => self.original.get_tree_seed(tree_id),
            GeneratedTreeId::Generated {
                root_tree_index,
                split_tree_index: generated_tree_index,
            } => self.with_cached_tree_for_root_tree_index(root_tree_index, |g| {
                g.trees[generated_tree_index].seed
            }),
        }
    }

    fn root_tree_len(&self) -> usize {
        // The first root tree cannot be split.
        (self.original.root_tree_len() - 1).lossless_shl(self.split_bits) + 1
    }

    fn root_tree_id(&self, index: usize) -> TreeId {
        if index == 0 {
            let tree_id = self.original.root_tree_id(0);
            self.unparse_tree_id(GeneratedTreeId::Original(tree_id))
        } else {
            let split_index = (index - 1) & (1usize.lossless_shl(self.split_bits) - 1);
            let orig_root_tree_index = ((index - 1) >> self.split_bits) + 1;
            self.unparse_tree_id(GeneratedTreeId::Generated {
                root_tree_index: orig_root_tree_index,
                split_tree_index: split_index,
            })
        }
    }
}

fn maybe_read_tree(
    provider: &dyn VirtualTreeProvider,
    tree_id: Option<TreeId>,
) -> Box<dyn Iterator<Item = (NameId, ContentId)> + '_> {
    match tree_id {
        Some(tree_id) => provider.read_tree(tree_id),
        None => Box::new(std::iter::empty()),
    }
}

struct ArcVecIter<T>(Arc<Vec<T>>, usize);

impl<T: Copy> Iterator for ArcVecIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        let index = self.1;
        self.1 += 1;
        self.0.get(index).copied()
    }
}

impl SplitChanges {
    /// Derived tree provider. Files are repeated by `1 << split_bits` times.
    pub fn new(tree: Arc<dyn VirtualTreeProvider>, split_bits: u8) -> Self {
        let root_tree_bits = (usize::BITS - tree.root_tree_len().leading_zeros())
            .try_into()
            .unwrap();
        Self {
            original: tree,
            split_bits,
            root_tree_bits,
            cache: Default::default(),
        }
    }

    fn cache_id_for_root_tree_index(&self, index: usize) -> CacheId {
        CacheId {
            root_tree_index: index,
        }
    }

    fn with_cached_tree_for_root_tree_index<T>(
        &self,
        root_tree_index: usize,
        f: impl Fn(&GeneratedTrees) -> T,
    ) -> T {
        let cache_id = self.cache_id_for_root_tree_index(root_tree_index);
        {
            let cache = self.cache.read();
            if let Some(got) = cache.get(&cache_id) {
                return f(got);
            }
        }
        {
            let mut cache = self.cache.write();
            let generated = cache
                .entry(cache_id)
                .or_insert_with(|| GeneratedTrees::new(self, root_tree_index));
            f(&*generated)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::*;

    #[test]
    fn test_flat_modified_changes() {
        let tree = TestTree::new(&[&[('A', "11"), ('A', "22")], &[('M', "11"), ('M', "22")]]);
        assert_eq!(
            tree.show_root_trees(),
            r#"
            Root tree 1:         #1  seed=0
              11 = 1
              22 = 1
            Root tree 2:         #2  seed=0
              11 = 2
              22 = 2"#
        );

        let tree = Arc::new(tree);
        let stretched = SplitChanges::new(tree.clone(), 1);
        assert_eq!(
            stretched.show_root_trees(),
            r#"
            Root tree 1:         #2  seed=0
              11 = 1
              22 = 1
            Root tree 2:         #3  seed=0
              11 = 2
              22 = 1
            Root tree 3:         #11 seed=0
              11 = 2
              22 = 2"#
        );
    }

    #[test]
    fn test_nested_modified_changes() {
        let tree = TestTree::new(&[
            &[
                ('A', "1/1/1"),
                ('A', "1/1/2"),
                ('A', "1/2/1"),
                ('A', "1/2/2"),
            ],
            &[
                ('M', "1/1/1"),
                ('M', "1/1/2"),
                ('M', "1/2/1"),
                ('M', "1/2/2"),
            ],
        ]);
        assert_eq!(
            tree.show_root_trees(),
            r#"
            Root tree 1:         #3  seed=0
              1/                 #2  seed=1
                1/               #1  seed=2
                  1 = 1
                  2 = 1
                2/               #1  seed=2
                  1 = 1
                  2 = 1
            Root tree 2:         #6  seed=0
              1/                 #5  seed=1
                1/               #4  seed=2
                  1 = 2
                  2 = 2
                2/               #4  seed=2
                  1 = 2
                  2 = 2"#
        );

        let tree = Arc::new(tree);
        let stretched = SplitChanges::new(tree.clone(), 1);
        assert_eq!(
            stretched.show_root_trees(),
            r#"
            Root tree 1:         #6  seed=0
              1/                 #4  seed=1
                1/               #2  seed=2
                  1 = 1
                  2 = 1
                2/               #2  seed=2
                  1 = 1
                  2 = 1
            Root tree 2:         #3  seed=0
              1/                 #19 seed=1
                1/               #8  seed=2
                  1 = 2
                  2 = 2
                2/               #2  seed=2
                  1 = 1
                  2 = 1
            Root tree 3:         #11 seed=0
              1/                 #10 seed=1
                1/               #8  seed=2
                  1 = 2
                  2 = 2
                2/               #8  seed=2
                  1 = 2
                  2 = 2"#
        );

        let stretched = SplitChanges::new(tree.clone(), 2);
        assert_eq!(
            stretched.show_root_trees(),
            r#"
            Root tree 1:         #6  seed=0
              1/                 #4  seed=1
                1/               #2  seed=2
                  1 = 1
                  2 = 1
                2/               #2  seed=2
                  1 = 1
                  2 = 1
            Root tree 2:         #3  seed=0
              1/                 #43 seed=1
                1/               #35 seed=2
                  1 = 2
                  2 = 1
                2/               #2  seed=2
                  1 = 1
                  2 = 1
            Root tree 3:         #11 seed=0
              1/                 #51 seed=1
                1/               #8  seed=2
                  1 = 2
                  2 = 2
                2/               #2  seed=2
                  1 = 1
                  2 = 1
            Root tree 4:         #19 seed=0
              1/                 #67 seed=1
                1/               #8  seed=2
                  1 = 2
                  2 = 2
                2/               #59 seed=2
                  1 = 2
                  2 = 1
            Root tree 5:         #27 seed=0
              1/                 #10 seed=1
                1/               #8  seed=2
                  1 = 2
                  2 = 2
                2/               #8  seed=2
                  1 = 2
                  2 = 2"#
        );
    }

    #[test]
    fn test_removed_changes() {
        let tree = TestTree::new(&[
            &[
                ('A', "1/2/4"),
                ('A', "1/2/5"),
                ('A', "1/3/4"),
                ('A', "1/3/5"),
            ],
            &[('R', "1/2/4"), ('R', "1/2/5")],
            &[('R', "1/3/4"), ('R', "1/3/5")],
        ]);
        assert_eq!(
            tree.show_root_trees(),
            r#"
            Root tree 1:         #3  seed=0
              1/                 #2  seed=1
                2/               #1  seed=2
                  4 = 1
                  5 = 1
                3/               #1  seed=2
                  4 = 1
                  5 = 1
            Root tree 2:         #5  seed=0
              1/                 #4  seed=1
                3/               #1  seed=2
                  4 = 1
                  5 = 1
            Root tree 3:         #6  seed=0"#
        );

        let tree = Arc::new(tree);
        let stretched = SplitChanges::new(tree.clone(), 1);
        assert_eq!(
            stretched.show_root_trees(),
            r#"
            Root tree 1:         #6  seed=0
              1/                 #4  seed=1
                2/               #2  seed=2
                  4 = 1
                  5 = 1
                3/               #2  seed=2
                  4 = 1
                  5 = 1
            Root tree 2:         #3  seed=0
              1/                 #27 seed=1
                2/               #19 seed=2
                  5 = 1
                3/               #2  seed=2
                  4 = 1
                  5 = 1
            Root tree 3:         #11 seed=0
              1/                 #8  seed=1
                3/               #2  seed=2
                  4 = 1
                  5 = 1
            Root tree 4:         #5  seed=0
              1/                 #29 seed=1
                3/               #21 seed=2
                  5 = 1
            Root tree 5:         #13 seed=0"#
        );
    }

    #[test]
    fn test_added_changes() {
        let tree = TestTree::new(&[
            &[],
            &[('A', "1/2/4"), ('A', "1/2/5"), ('A', "1/4"), ('A', "1/5")],
        ]);
        assert_eq!(
            tree.show_root_trees(),
            r#"
            Root tree 1:         #1  seed=0
            Root tree 2:         #4  seed=0
              1/                 #3  seed=1
                2/               #2  seed=2
                  4 = 1
                  5 = 1
                4 = 1
                5 = 1"#
        );

        let tree = Arc::new(tree);
        let stretched = SplitChanges::new(tree.clone(), 2);
        assert_eq!(
            stretched.show_root_trees(),
            r#"
            Root tree 1:         #2  seed=0
            Root tree 2:         #3  seed=0
              1/                 #43 seed=1
                2/               #35 seed=2
                  4 = 1
            Root tree 3:         #11 seed=0
              1/                 #51 seed=1
                2/               #4  seed=2
                  4 = 1
                  5 = 1
            Root tree 4:         #19 seed=0
              1/                 #59 seed=1
                2/               #4  seed=2
                  4 = 1
                  5 = 1
                4 = 1
            Root tree 5:         #27 seed=0
              1/                 #6  seed=1
                2/               #4  seed=2
                  4 = 1
                  5 = 1
                4 = 1
                5 = 1"#
        );
    }
}
