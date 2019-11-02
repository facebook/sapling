/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! treestate - Tree-based State.
//!
//! The tree state stores a map from paths to a lightweight structure, and provides efficient
//! lookups.  In particular, for each file in the tree, it stores the mode flags, size, mtime, and
//! whether deleted or not, etc. These can be useful for source control to determine if the file
//! is tracked, or has changed, etc.

pub mod errors;
pub mod filestate;
pub mod filestore;
pub mod serialization;
pub mod store;
pub mod tree;
pub mod treedirstate;
pub mod treestate;
pub mod vecmap;
pub mod vecstack;

pub use crate::errors::*;
