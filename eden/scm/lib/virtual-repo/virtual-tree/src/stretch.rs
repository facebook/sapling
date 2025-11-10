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

pub use deepen_trees::DeepenTrees;
pub use repeat_files::RepeatFiles;
