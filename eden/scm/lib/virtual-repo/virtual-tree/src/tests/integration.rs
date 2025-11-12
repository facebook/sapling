/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Test applying stretch methods to an example tree.

use std::sync::Arc;

use crate::serialized::EXAMPLE1;
use crate::stretch::stretch_trees;
use crate::tests::ShowTree;
use crate::types::*;

fn count_files_and_dirs(provider: &dyn VirtualTreeProvider, tree_id: TreeId) -> (usize, usize) {
    let mut file_count = 0;
    let mut dir_count = 1;
    for (_name_id, content_id) in provider.read_tree(tree_id) {
        let typed_content_id = TypedContentId::from(content_id);
        match typed_content_id {
            TypedContentId::Tree(sub_tree_id) => {
                let (subtree_file_count, subtree_dir_count) =
                    count_files_and_dirs(provider, sub_tree_id);
                file_count += subtree_file_count;
                dir_count += subtree_dir_count;
            }
            TypedContentId::File(_blob_id, _file_mode) => file_count += 1,
            TypedContentId::Absent => unreachable!(),
        }
    }
    (file_count, dir_count)
}

fn count_last_tree(provider: &dyn VirtualTreeProvider) -> (usize, usize) {
    let last_root_tree_id = provider.root_tree_id(provider.root_tree_len() - 1);
    count_files_and_dirs(provider, last_root_tree_id)
}

#[test]
fn test_integrated_tree() {
    let original = Arc::new(EXAMPLE1.clone());
    let stretched = stretch_trees(original.clone(), 8);

    // Count root trees.
    assert_eq!(original.root_tree_len(), 30468);
    assert_eq!(stretched.root_tree_len(), 7_799_553);

    // Count files and dirs.
    assert_eq!(count_last_tree(&*original), (14341, 3040));

    // Takes 3s in release build to count 3M files. Do not count this in debug build.
    if cfg!(not(debug_assertions)) {
        assert_eq!(count_last_tree(&*stretched), (3_671_296, 452_860));
    }

    // Take a look at the stretched trees. Render the first 2 levels of items.
    let tree_id = stretched.root_tree_id(stretched.root_tree_len() - 1);
    assert_eq!(
        stretched.show_tree(tree_id, true, 1),
        r#"
1/                              #32965386 seed=257 files=7
3/                              #32965388 seed=258 files=7
  1/                             #32965390 seed=259 files=7
7/                              #65930832 seed=260 files=7
  1/                             #65930834 seed=261 files=7
  3/                             #65930836 seed=262 files=7
15/                              #65930848 seed=264 files=7
  1/                             #65930850 seed=265 files=7
  3/                             #65930852 seed=266 files=7
  7/                             #65930864 seed=268 files=7
31/                              #131862144 seed=272 files=7
  1/                             #131862146 seed=273 files=7
  3/                             #131862148 seed=274 files=7
  7/                             #131862160 seed=276 files=7
  15/                              #131862176 seed=280 files=7
63/                              #131862272 seed=288 files=7
  1/                             #131862274 seed=289 files=7
  3/                             #131862276 seed=290 files=7
  7/                             #131862288 seed=292 files=7
  15/                              #131862304 seed=296 files=7
  31/                              #131862400 seed=304 files=7
127/                              #124797952 seed=320 files=7
  1/                             #124797954 seed=321 files=7
  3/                             #124797956 seed=322 files=7
  7/                             #124797968 seed=324 files=7
  15/                              #124797984 seed=328 files=7
  31/                              #124798080 seed=336 files=7
  63/                              #124798208 seed=352 files=7
255/                              #124798976 seed=384 files=7
  1/                             #124798978 seed=385 files=7
  3/                             #124798980 seed=386 files=7
  7/                             #124798992 seed=388 files=7
  15/                              #124799008 seed=392 files=7
  31/                              #124799104 seed=400 files=7
  63/                              #124799232 seed=416 files=7
  127/                              #124800000 seed=448 files=7
1279/                              #1604943872 seed=512 files=4
  1/                             #1604943874 seed=513 files=4
  3/                             #1604943876 seed=514 files=4
  7/                             #1604943888 seed=516 files=4
  15/                              #1604943904 seed=520 files=4
  31/                              #1604944000 seed=528 files=4
  63/                              #1604944128 seed=544 files=4
  127/                              #1604944896 seed=576 files=4
  255/                              #1604945920 seed=640 files=4
  3071/                              #1604886528 seed=1024 files=12
  3583/                              #1388404736 seed=2560 files=2
  4095/                              #1596047360 seed=2816 files=4
  4351/                              #1604947968 seed=3072 files=3
  4607/                              #1566924800 seed=33792 files=0
  5119/                              #1603616768 seed=36352 files=153
  7167/                              #1457934336 seed=136448 files=6
  7423/                              #1550061568 seed=187392 files=1
  7679/                              #1603608576 seed=311296 files=9
  8447/                              #1584381952 seed=354048 files=13
  10239/                              #1600847872 seed=1821440 files=5
2303/                              #1604849664 seed=184576 files=1
  1/                             #1604849666 seed=184577 files=1
  3/                             #1604849668 seed=184578 files=1
  7/                             #1604849680 seed=184580 files=1
  15/                              #1604849696 seed=184584 files=1
  31/                              #1604849792 seed=184592 files=1
  63/                              #1604849920 seed=184608 files=1
  127/                              #1604850688 seed=184640 files=1
  255/                              #1604851712 seed=184704 files=1
  1279/                              #1604853760 seed=185344 files=0
  1535/                              #1603973120 seed=186112 files=4
3327/                              #1553350656 seed=350976 files=0
  511/                              #1553354752 seed=351232 files=3
"#
    );
}
