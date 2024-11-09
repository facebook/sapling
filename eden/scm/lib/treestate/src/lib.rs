/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! treestate - Tree-based State.
//!
//! The tree state stores a map from paths to a lightweight structure, and provides efficient
//! lookups.  In particular, for each file in the tree, it stores the mode flags, size, mtime, and
//! whether deleted or not, etc. These can be useful for source control to determine if the file
//! is tracked, or has changed, etc.

pub mod dirstate;
pub mod errors;
mod filereadwrite;
pub mod filestate;
pub mod filestore;
pub mod metadata;
pub mod overlay_dirstate;
pub mod root;
pub mod serialization;
pub mod store;
pub mod tree;
pub mod treestate;
pub mod vecmap;
pub mod vecstack;
mod wait;

pub use wait::Wait;

pub use crate::errors::*;
