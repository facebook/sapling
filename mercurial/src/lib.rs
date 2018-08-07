// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(const_fn)]

// External dependencies

extern crate ascii;
extern crate bytes;
extern crate flate2;
extern crate futures;
#[macro_use]
extern crate futures_ext;

#[macro_use]
extern crate failure_ext as failure;

extern crate heapsize;

#[macro_use]
extern crate nom;

#[macro_use]
extern crate bitflags;

#[cfg(test)]
#[macro_use]
extern crate assert_matches;

extern crate itertools;
extern crate memmap;

#[cfg(not(test))]
extern crate quickcheck;
#[cfg(test)]
#[macro_use]
extern crate quickcheck;

extern crate serde;

extern crate mercurial_types;
extern crate mercurial_types_mocks;
extern crate mononoke_types;
extern crate mononoke_types_thrift;
extern crate pylz4;
extern crate storage_types;

pub mod revlog;
pub mod manifest;
pub mod changeset;
pub mod revlogrepo;
pub mod file;
pub mod stockbookmarks;
pub mod symlink;
mod errors;
pub use errors::*;

pub use changeset::RevlogChangeset;
pub use manifest::{EntryContent, RevlogEntry};
pub use revlogrepo::{RevlogManifest, RevlogRepo, RevlogRepoOptions};

mod thrift {
    pub use mononoke_types_thrift::*;
}
