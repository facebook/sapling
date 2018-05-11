// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Provides traits for walking and reading the content of a Virtual File System as well as
//! implementation of those traits for Vfs based on Manifest
#![deny(missing_docs)]
#![deny(warnings)]

#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;
extern crate itertools;
#[macro_use]
#[cfg(test)]
extern crate maplit;

extern crate mercurial_types;
extern crate mononoke_types;

#[cfg(test)]
extern crate boxfnonce;
#[cfg(test)]
extern crate mercurial_types_mocks;

pub mod errors;
mod manifest_vfs;
mod node;
mod tree;

pub use manifest_vfs::{vfs_from_manifest, ManifestVfsDir, ManifestVfsFile};
pub use node::{VfsDir, VfsFile, VfsNode, VfsWalker};

#[cfg(test)]
mod test;
