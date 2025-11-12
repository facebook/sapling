/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Stretch existing trees to produce larger trees. 3 dimensions to stretch:
//! - File count.
//! - Tree depth.
//! - Commit history length (root tree length).

pub(crate) mod deepen_trees;
pub(crate) mod repeat_files;
pub(crate) mod split_changes;

use std::sync::Arc;

pub use deepen_trees::DeepenTrees;
pub use repeat_files::RepeatFiles;
pub use split_changes::SplitChanges;

use crate::types::VirtualTreeProvider;

/// Stretch trees using default settings.
///
/// Commit (root trees), files, and trees can increase by up to (1 <<
/// factor_bits) times.
pub fn stretch_trees(
    mut provider: Arc<dyn VirtualTreeProvider>,
    factor_bits: u8,
) -> Arc<dyn VirtualTreeProvider> {
    let mut remaining_split_factor_bits = factor_bits;
    // Apply SplitChanges every SPLIT_PER_BITS bits.
    // Setting this too high leads to poor performance since SplitChanges needs
    // to deal with a large diff internally.
    // Setting this too low leads to use more bits of the TreeId space.
    // Run `cargo run --release -- 1st` in `virtual-tree/benches` to reason
    // about the performance.
    // NOTE: These values are not decided scientifically. Tweak if needed.
    let split_per_bits = match factor_bits {
        30.. => 4, // Use less bits, less performant.
        20.. => 3,
        _ => 2, // Use more bits, but more performant (avoid large diffs)
    };
    for i in 0..factor_bits {
        provider = Arc::new(RepeatFiles::new(provider, 1));
        provider = Arc::new(DeepenTrees::new(provider));
        if ((i + 1) % split_per_bits) == 0 {
            provider = Arc::new(SplitChanges::new(provider, split_per_bits));
            remaining_split_factor_bits -= split_per_bits;
        }
    }
    if remaining_split_factor_bits > 0 {
        provider = Arc::new(SplitChanges::new(provider, remaining_split_factor_bits));
    }
    provider
}
