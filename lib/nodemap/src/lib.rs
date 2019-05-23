// Copyright Facebook, Inc. 2018
//! nodemap - A store for node-to-node mappings, with bidirectional indexes.

#[macro_use]
extern crate failure;
extern crate indexedlog;
#[cfg(test)]
#[macro_use]
extern crate quickcheck;
#[cfg(test)]
extern crate tempfile;
extern crate types;

pub mod nodemap;
pub use crate::nodemap::NodeMap;
