// Copyright Facebook, Inc. 2017
//! treestate - Tree-based State.
//!
//! The tree state stores a map from paths to a lightweight structure, and provides efficient
//! lookups.  In particular, for each file in the tree, it stores the mode flags, size, mtime, and
//! whether deleted or not, etc. These can be useful for source control to determine if the file
//! is tracked, or has changed, etc.

#[macro_use]
extern crate bitflags;

extern crate byteorder;

#[macro_use]
extern crate failure;

#[cfg(test)]
extern crate itertools;

#[cfg(test)]
#[macro_use]
extern crate quickcheck;

#[cfg(test)]
extern crate rand;

#[cfg(test)]
extern crate rand_chacha;

#[cfg(test)]
extern crate tempdir;

extern crate twox_hash;

extern crate vlqencoding;

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

pub use errors::*;
