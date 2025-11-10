/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::LazyLock;

use super::SerializedTree;

/// Example created from the fbthrift repo.
pub static EXAMPLE1: LazyLock<SerializedTree> =
    LazyLock::new(|| SerializedTree::new(include_bytes!("example1.zst")));

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::*;
    use crate::types::*;

    #[test]
    fn test_example1_sanity_check() {
        // Check that an (example) tree can be read.
        let example_tree_id = TreeId(NonZeroU64::new(30542).unwrap());
        assert_eq!(
            EXAMPLE1.show_tree(example_tree_id, false, usize::MAX),
            r#"
1 = 1x
2 = 1x
3 = 1x
4 = 1
5/                              #30543 seed=76
  1/                             #30544 seed=77
    1/                           #30545 seed=78
      1 = 1l
"#
        );
    }
}
