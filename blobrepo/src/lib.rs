// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(conservative_impl_trait)]

#[macro_use]
extern crate failure_ext as failure;
#[macro_use]
extern crate futures;

extern crate bincode;
extern crate bytes;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate tokio_core;

extern crate heapsize;
#[macro_use]
extern crate heapsize_derive;

extern crate blobstore;
extern crate bookmarks;
extern crate changesets;
extern crate fileblob;
extern crate filebookmarks;
extern crate fileheads;
extern crate filelinknodes;
#[macro_use]
extern crate futures_ext;
extern crate heads;
extern crate linknodes;
extern crate manifoldblob;
extern crate memblob;
extern crate membookmarks;
extern crate memheads;
extern crate memlinknodes;
extern crate mercurial;
extern crate mercurial_types;
extern crate rocksblob;
extern crate storage_types;

mod repo;
mod changeset;
mod manifest;
mod file;
mod errors;
mod utils;
mod repo_commit;

pub use errors::*;

pub use changeset::BlobChangeset;
pub use file::BlobEntry;
pub use manifest::BlobManifest;
pub use repo::BlobRepo;
pub use repo_commit::ChangesetHandle;
// TODO: This is exported for testing - is this the right place for it?
pub use repo_commit::compute_changed_files;
//
// TODO: (jsgf) T21597565 This is exposed here for blobimport -- don't use it for anything else.

pub use utils::RawNodeBlob;
