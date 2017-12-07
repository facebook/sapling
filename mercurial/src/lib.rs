// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

// External dependencies

extern crate flate2;
extern crate futures;
extern crate futures_ext;

#[macro_use]
extern crate failure_ext as failure;

#[macro_use]
extern crate nom;

#[macro_use]
extern crate bitflags;

#[cfg(test)]
extern crate assert_matches;

extern crate itertools;
extern crate lz4;
extern crate memmap;
extern crate time;

#[cfg(test)]
#[macro_use]
extern crate quickcheck;

extern crate asyncmemo;
extern crate bookmarks;
extern crate mercurial_types;
extern crate stockbookmarks;
extern crate storage_types;

pub mod revlog;
pub mod manifest;
pub mod changeset;
pub mod revlogrepo;
pub mod file;
pub mod symlink;
mod errors;
pub use errors::*;

pub use revlogrepo::{RevlogManifest, RevlogRepo};
