// Copyright Facebook, Inc. 2017
//! treedirstate - Tree-based Directory State.
//!
//! This is a Rust implementation of the dirstate concept for Mercurial, using a tree structure
//! in an append-only storage back-end.
//!
//! The directory state stores information for all files in a working copy that are of interest
//! to Mercurial.  In particular, for each file in the working copy it stores the mode flags,
//! size, and modification time of the file.  These can be compared with current values to
//! determine if the file has changed.
//!
//! The directory state also stores files that are in the working copy parent manifest but have
//! been marked as removed.

extern crate byteorder;

#[cfg(not(test))]
#[macro_use]
extern crate cpython;

#[macro_use]
extern crate error_chain;

#[cfg(test)]
extern crate itertools;

#[cfg(test)]
#[macro_use]
extern crate quickcheck;

#[cfg(test)]
extern crate tempdir;

extern crate vlqencoding;

pub mod dirstate;
pub mod errors;
pub mod filestate;
pub mod filestore;
#[cfg(not(test))]
#[allow(non_camel_case_types)]
pub mod python;
pub mod serialization;
pub mod store;
pub mod tree;
pub mod vecmap;

pub use errors::*;
