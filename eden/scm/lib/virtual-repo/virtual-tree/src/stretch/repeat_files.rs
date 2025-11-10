/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::num::NonZeroU64;
use std::sync::Arc;

use crate::types::*;

/// Repeat files. Directories are unchanged.
/// Example: `dir/file1` -> `dir/file1a`, `dir/file1b`.
#[derive(Clone)]
pub struct RepeatFiles {
    factor_bits: u8,
    original: Arc<dyn VirtualTreeProvider>,
}

impl VirtualTreeProvider for RepeatFiles {
    fn read_tree<'a>(&'a self, tree_id: TreeId) -> ReadTreeIter<'a> {
        let items = self.original.read_tree(tree_id);
        Box::new(items.flat_map(move |(name_id, content_id)| {
            let typed_content_id = TypedContentId::from(content_id);
            match typed_content_id {
                TypedContentId::Tree(_tree_id) => {
                    // Do not duplicate trees. It creats too many files exponentially, O(N ** depth).
                    let new_name_id = NameId(self.scale_id_up(name_id.0, 0));
                    Box::new(std::iter::once((new_name_id, content_id)))
                        as Box<dyn Iterator<Item = (NameId, ContentId)>>
                }
                TypedContentId::File(blob_id, file_mode) => {
                    // Duplicate files.
                    Box::new((0..=self.mask()).map(move |offset| {
                        let new_name_id = NameId(self.scale_id_up(name_id.0, offset));
                        let new_content_id = TypedContentId::File(
                            BlobId(self.scale_id_up(blob_id.0, offset)),
                            file_mode,
                        )
                        .into();
                        (new_name_id, new_content_id)
                    }))
                }
                TypedContentId::Absent => unreachable!(),
            }
        }))
    }

    fn get_tree_seed(&self, tree_id: TreeId) -> TreeSeed {
        self.original.get_tree_seed(tree_id)
    }

    fn root_tree_len(&self) -> usize {
        self.original.root_tree_len()
    }

    fn root_tree_id(&self, index: usize) -> TreeId {
        self.original.root_tree_id(index)
    }
}

impl RepeatFiles {
    /// Derived tree provider. Files are repeated by `1 << factor_bits` times.
    pub fn new(original: Arc<dyn VirtualTreeProvider>, factor_bits: u8) -> Self {
        Self {
            original,
            factor_bits,
        }
    }

    fn mask(&self) -> u64 {
        (1u64 << self.factor_bits) - 1
    }

    fn scale_id_up(&self, id: NonZeroU64, offset: u64) -> NonZeroU64 {
        assert!(offset <= self.mask());
        NonZeroU64::new((id.get() << self.factor_bits) | offset).unwrap()
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::tests::*;

    #[test]
    fn test_repeat_files_basic() {
        let tree = TestTree::new(&[
            &[('A', "1/1/1"), ('A', "1/2"), ('A', "1/3")],
            &[('R', "1/1/1"), ('M', "1/3")],
        ]);
        assert_eq!(
            tree.show_root_trees(),
            r#"
            Root tree 1:         #3  seed=0
              1/                 #2  seed=1
                1/               #1  seed=2
                  1 = 1
                2 = 1
                3 = 1
            Root tree 2:         #5  seed=0
              1/                 #4  seed=1
                2 = 1
                3 = 2"#
        );

        // factor_bits = 0. Nothing is duplicated. Same layout.
        let tree = Arc::new(tree);
        let stretched0 = RepeatFiles::new(tree.clone(), 0);
        assert_eq!(stretched0.show_root_trees(), tree.show_root_trees(),);

        // factor_bits = 1. Files are duplicated. But trees are not (only changed names).
        let stretched1 = RepeatFiles::new(tree.clone(), 1);
        assert_eq!(
            stretched1.show_root_trees(),
            r#"
            Root tree 1:         #3  seed=0
              2/                 #2  seed=1
                2/               #1  seed=2
                  2 = 2
                  3 = 3
                4 = 2
                5 = 3
                6 = 2
                7 = 3
            Root tree 2:         #5  seed=0
              2/                 #4  seed=1
                4 = 2
                5 = 3
                6 = 4
                7 = 5"#
        );

        // factor_bits = 2. Files are x4.
        let stretched2 = RepeatFiles::new(tree.clone(), 2);
        assert_eq!(
            stretched2.show_root_trees(),
            r#"
            Root tree 1:         #3  seed=0
              4/                 #2  seed=1
                4/               #1  seed=2
                  4 = 4
                  5 = 5
                  6 = 6
                  7 = 7
                8 = 4
                9 = 5
                10 = 6
                11 = 7
                12 = 4
                13 = 5
                14 = 6
                15 = 7
            Root tree 2:         #5  seed=0
              4/                 #4  seed=1
                8 = 4
                9 = 5
                10 = 6
                11 = 7
                12 = 8
                13 = 9
                14 = 10
                15 = 11"#
        );
    }
}
