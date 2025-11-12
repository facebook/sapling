/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Benchmark for virtual-tree algorithms.

use std::sync::Arc;

use minibench::bench;
use minibench::elapsed;
use virtual_tree::serialized::EXAMPLE1;
use virtual_tree::stretch::stretch_trees;
use virtual_tree::types::VirtualTreeProvider;

fn read_2nd_last_root_tree(tree_provider: &dyn VirtualTreeProvider) -> usize {
    let root_tree_len = tree_provider.root_tree_len();
    let root_tree_id = tree_provider.root_tree_id(root_tree_len - 2);
    tree_provider.read_tree(root_tree_id).count()
}

fn main() {
    for factor_bits in [6, 7, 8, 9, 10] {
        let example1 = Arc::new(EXAMPLE1.clone());
        bench(
            format!("factor_bits={factor_bits} 2nd-last root tree (1st time)"),
            || {
                let tree_provider = stretch_trees(example1.clone(), factor_bits);
                elapsed(|| {
                    read_2nd_last_root_tree(&*tree_provider);
                })
            },
        );
        bench(
            format!("factor_bits={factor_bits} 2nd-last root tree (2nd time)"),
            || {
                let tree_provider = stretch_trees(example1.clone(), factor_bits);
                read_2nd_last_root_tree(&*tree_provider);
                elapsed(|| {
                    read_2nd_last_root_tree(&*tree_provider);
                })
            },
        );
    }
}
