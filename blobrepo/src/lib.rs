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
extern crate bytes;
extern crate serde;
#[macro_use]
extern crate serde_derive;

extern crate heapsize;
#[macro_use]
extern crate heapsize_derive;

extern crate blobstore;
extern crate bookmarks;
extern crate fileblob;
extern crate filebookmarks;
extern crate fileheads;
extern crate futures_ext;
extern crate heads;
extern crate manifoldblob;
extern crate memblob;
extern crate membookmarks;
extern crate memheads;
extern crate mercurial;
extern crate mercurial_types;
extern crate rocksblob;
extern crate tokio_core;

mod repo;
mod changeset;
mod manifest;
mod state;
mod file;
mod errors;
mod utils;

pub use errors::*;

pub use changeset::BlobChangeset;
pub use manifest::BlobManifest;
pub use repo::BlobRepo;
pub use state::{BlobState, FilesBlobState, ManifoldBlobState, MemBlobState, RocksBlobState};
//
// TODO: (jsgf) T21597565 This is exposed here for blobimport -- don't use it for anything else.

pub use utils::RawNodeBlob;
