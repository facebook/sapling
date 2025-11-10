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
1/                              #15599618 seed=257 files=7
3/                              #15599620 seed=258 files=7
  1/                             #15599622 seed=259 files=7
7/                              #15599624 seed=260 files=7
  1/                             #15599626 seed=261 files=7
  3/                             #15599628 seed=262 files=7
15/                              #15599632 seed=264 files=7
  1/                             #15599634 seed=265 files=7
  3/                             #15599636 seed=266 files=7
  7/                             #15599640 seed=268 files=7
31/                              #15599648 seed=272 files=7
  1/                             #15599650 seed=273 files=7
  3/                             #15599652 seed=274 files=7
  7/                             #15599656 seed=276 files=7
  15/                              #15599664 seed=280 files=7
63/                              #15599680 seed=288 files=7
  1/                             #15599682 seed=289 files=7
  3/                             #15599684 seed=290 files=7
  7/                             #15599688 seed=292 files=7
  15/                              #15599696 seed=296 files=7
  31/                              #15599712 seed=304 files=7
127/                              #15599744 seed=320 files=7
  1/                             #15599746 seed=321 files=7
  3/                             #15599748 seed=322 files=7
  7/                             #15599752 seed=324 files=7
  15/                              #15599760 seed=328 files=7
  31/                              #15599776 seed=336 files=7
  63/                              #15599808 seed=352 files=7
255/                              #15599872 seed=384 files=7
  1/                             #15599874 seed=385 files=7
  3/                             #15599876 seed=386 files=7
  7/                             #15599880 seed=388 files=7
  15/                              #15599888 seed=392 files=7
  31/                              #15599904 seed=400 files=7
  63/                              #15599936 seed=416 files=7
  127/                              #15600000 seed=448 files=7
1279/                              #200617984 seed=512 files=4
  1/                             #200617986 seed=513 files=4
  3/                             #200617988 seed=514 files=4
  7/                             #200617992 seed=516 files=4
  15/                              #200618000 seed=520 files=4
  31/                              #200618016 seed=528 files=4
  63/                              #200618048 seed=544 files=4
  127/                              #200618112 seed=576 files=4
  255/                              #200618240 seed=640 files=4
  3071/                              #200610816 seed=1024 files=12
  3583/                              #173550592 seed=2560 files=2
  4095/                              #199505920 seed=2816 files=4
  4351/                              #200618496 seed=3072 files=3
  4607/                              #195865600 seed=33792 files=0
  5119/                              #200452096 seed=36352 files=153
  7167/                              #182241792 seed=136448 files=6
  7423/                              #193757696 seed=187392 files=1
  7679/                              #200451072 seed=311296 files=9
  8447/                              #198047744 seed=354048 files=13
  10239/                              #200105984 seed=1821440 files=5
2303/                              #200606208 seed=184576 files=1
  1/                             #200606210 seed=184577 files=1
  3/                             #200606212 seed=184578 files=1
  7/                             #200606216 seed=184580 files=1
  15/                              #200606224 seed=184584 files=1
  31/                              #200606240 seed=184592 files=1
  63/                              #200606272 seed=184608 files=1
  127/                              #200606336 seed=184640 files=1
  255/                              #200606464 seed=184704 files=1
  1279/                              #200606720 seed=185344 files=0
  1535/                              #200496640 seed=186112 files=4
3327/                              #194168832 seed=350976 files=0
  511/                              #194169344 seed=351232 files=3
"#
    );
}
