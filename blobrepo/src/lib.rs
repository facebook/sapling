// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

#[macro_use]
extern crate error_chain;
#[macro_use]
extern crate futures;

extern crate bincode;
extern crate serde;
#[macro_use]
extern crate serde_derive;

extern crate heapsize;
#[macro_use]
extern crate heapsize_derive;

extern crate blobstore;
extern crate bookmarks;
extern crate futures_ext;
extern crate heads;
extern crate mercurial;
extern crate mercurial_types;

mod repo;
mod changeset;
mod manifest;
mod file;
mod errors;
mod utils;

pub use errors::*;

pub use changeset::BlobChangeset;
pub use manifest::BlobManifest;
pub use repo::BlobRepo;
