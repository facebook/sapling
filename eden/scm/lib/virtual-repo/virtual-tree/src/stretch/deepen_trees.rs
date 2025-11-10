/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::num::NonZeroU64;
use std::sync::Arc;

use smallvec::SmallVec;

use crate::types::*;

/// Deepen trees. Increase tree depth by ~1.
/// Example: `dir1/file1`, `dir1/file2` -> `dir1/subdir1/file1`, dir1/file2`.
#[derive(Clone)]
pub struct DeepenTrees {
    original: Arc<dyn VirtualTreeProvider>,
}

// The lowest bit of the `tree_id` indicates whether the tree is created on the fly.
//
// Example:
// `dir1/file1`, `dir1/file2` -> `dir1/subdir1/file1`, dir1/file2`.
// The `subdir1` is the directory that is created on the fly.
//
// - Lowest bit = 0: Original tree (tree_id / 2). Half of the files. Optionally
//   add a subdir1 entry.
// - Lowest bit = 1: The other half of the files from the original tree.

impl VirtualTreeProvider for DeepenTrees {
    fn read_tree<'a>(&'a self, tree_id: TreeId) -> ReadTreeIter<'a> {
        let original_tree_id = tree_id.untagged();
        let is_created = tree_id.is_created_subtree();
        if is_created {
            // Half of the files. No subtrees.
            let items = self.original.read_tree(original_tree_id);
            Box::new(items.filter(move |(name_id, content_id)| {
                let typed_content_id = TypedContentId::from(*content_id);
                match typed_content_id {
                    TypedContentId::Tree(..) => false,
                    TypedContentId::File(..) => name_id.should_move_to_subtree(),
                    TypedContentId::Absent => unreachable!(),
                }
            }))
        } else {
            // Decide whether to create a subtree.
            let items = self
                .original
                .read_tree(original_tree_id)
                .collect::<SmallVec<[_; 16]>>();
            let should_create_subtree = {
                items.iter().any(|(name_id, content_id)| {
                    matches!(TypedContentId::from(*content_id), TypedContentId::File(..))
                        && name_id.should_move_to_subtree()
                })
            };
            // NameId 1 is reserved for the optional subtree.
            let maybe_subtree = if should_create_subtree {
                let new_subtree_id = original_tree_id.tagged(1);
                Box::new(std::iter::once((
                    NameId(NonZeroU64::new(1).unwrap()),
                    TypedContentId::Tree(new_subtree_id).into(),
                ))) as Box<dyn Iterator<Item = (NameId, ContentId)>>
            } else {
                Box::new(std::iter::empty()) as Box<dyn Iterator<Item = (NameId, ContentId)>>
            };
            // Concatenate with the items that should remain in the current tree.
            // Their NameIds and TreeIds need to be rewritten.
            Box::new(maybe_subtree.chain(items.into_iter().filter_map(
                move |(name_id, content_id)| {
                    let typed_content_id = TypedContentId::from(content_id);
                    // NameId(1) is reserved for the "maybe_subtree", so add 1 to NameIds.
                    let new_name_id = NameId(name_id.0.checked_add(1).unwrap());
                    match typed_content_id {
                        TypedContentId::Tree(orig_subtree_id) => {
                            let new_tree_id = orig_subtree_id.tagged(0);
                            Some((new_name_id, TypedContentId::Tree(new_tree_id).into()))
                        }
                        TypedContentId::File(..) => {
                            if name_id.should_move_to_subtree() {
                                None
                            } else {
                                Some((new_name_id, content_id))
                            }
                        }
                        TypedContentId::Absent => None,
                    }
                },
            )))
        }
    }

    fn get_tree_seed(&self, tree_id: TreeId) -> TreeSeed {
        let original_tree_id = tree_id.untagged();
        let seed = self.original.get_tree_seed(original_tree_id);
        // Tag seed with 1 bit "created tree" flag.
        TreeSeed(seed.0.wrapping_shl(1) ^ (tree_id.is_created_subtree() as u64))
    }

    fn root_tree_len(&self) -> usize {
        self.original.root_tree_len()
    }

    fn root_tree_id(&self, index: usize) -> TreeId {
        self.original.root_tree_id(index).tagged(0)
    }
}

impl NameId {
    fn should_move_to_subtree(self) -> bool {
        (self.0.get() & 1) != 0
    }
}

impl TreeId {
    fn tagged(self, bit: u64) -> TreeId {
        debug_assert_eq!(bit & 1, bit);
        Self(NonZeroU64::new((self.0.get() << 1) | bit).unwrap())
    }

    fn untagged(self) -> Self {
        Self(NonZeroU64::new(self.0.get() >> 1).unwrap())
    }

    fn is_created_subtree(self) -> bool {
        (self.0.get() & 1) != 0
    }
}

impl DeepenTrees {
    pub fn new(original: Arc<dyn VirtualTreeProvider>) -> Self {
        Self { original }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::*;

    #[test]
    fn test_deepen_trees_basic() {
        let tree = TestTree::new(&[&[
            ('A', "1/1/1"),
            ('A', "1/1/2"),
            ('A', "1/2"),
            ('A', "1/3"),
            ('A', "2/1/1"),
            ('A', "3/1/2"),
        ]]);
        assert_eq!(
            tree.show_root_trees(),
            r#"
            Root tree 1:         #7  seed=0
              1/                 #2  seed=1
                1/               #1  seed=2
                  1 = 1
                  2 = 1
                2 = 1
                3 = 1
              2/                 #4  seed=3
                1/               #3  seed=4
                  1 = 1
              3/                 #6  seed=5
                1/               #5  seed=6
                  2 = 1"#
        );

        let tree = Arc::new(tree);
        let stretched = DeepenTrees::new(tree.clone());

        // 1/1/ has files 1 and 2. One of them is moved to a subdir.
        // 1/ has 2 and 3. One of them is moved to a subdir.
        // 2/1/1 is moved to a subdir.
        // 3/1/2 is unchanged.
        assert_eq!(
            stretched.show_root_trees(),
            r#"
            Root tree 1:         #14 seed=0
              2/                 #4  seed=2
                1/               #5  seed=3
                  3 = 1
                2/               #2  seed=4
                  1/             #3  seed=5
                    1 = 1
                  3 = 1
                3 = 1
              3/                 #8  seed=6
                2/               #6  seed=8
                  1/             #7  seed=9
                    1 = 1
              4/                 #12 seed=10
                2/               #10 seed=12
                  3 = 1"#
        );
    }
}
