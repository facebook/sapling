// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Provides traits for walking and reading the content of a Virtual File System as well as
//! implementation of those traits for Vfs based on Manifest
#![deny(missing_docs)]
// TODO(luk, T20453159) Remove once the library is ready to be used
#![allow(dead_code)]
#![deny(warnings)]

#[macro_use]
extern crate error_chain;
extern crate futures;
extern crate itertools;
extern crate mercurial_types;

#[cfg(test)]
extern crate boxfnonce;

pub mod errors;
mod node;
mod tree;

pub use node::{VfsDir, VfsFile, VfsNode, VfsWalker};
