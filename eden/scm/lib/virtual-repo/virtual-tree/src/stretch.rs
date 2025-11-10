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
    for _i in 0..factor_bits {
        provider = Arc::new(RepeatFiles::new(provider, 1));
        provider = Arc::new(DeepenTrees::new(provider));
    }
    Arc::new(SplitChanges::new(provider, factor_bits))
}
