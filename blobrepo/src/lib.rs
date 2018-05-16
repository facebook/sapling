// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

#[macro_use]
extern crate failure_ext as failure;
#[macro_use]
extern crate futures;

extern crate bincode;
extern crate bytes;
extern crate db;
extern crate serde;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate slog;
extern crate tokio_core;
extern crate uuid;

extern crate heapsize;
#[macro_use]
extern crate heapsize_derive;

extern crate futures_stats;

extern crate ascii;
extern crate blobstore;
extern crate bookmarks;
extern crate changesets;
extern crate dbbookmarks;
extern crate delayblob;
extern crate dieselfilenodes;
extern crate fileblob;
extern crate filenodes;
#[macro_use]
extern crate futures_ext;
extern crate manifoldblob;
extern crate memblob;
extern crate mercurial;
extern crate mercurial_types;
extern crate mononoke_types;
extern crate rocksblob;
extern crate rocksdb;
extern crate time_ext;

mod repo;
mod changeset;
mod manifest;
mod file;
mod errors;
mod utils;
mod repo_commit;

pub use errors::*;

// TODO(luk): T28348119 ChangesetContent is made publicly visible here for blobimport, once it's
// replaced by new blobimport it should be private again
pub use changeset::{BlobChangeset, ChangesetContent};
pub use file::HgBlobEntry;
pub use manifest::BlobManifest;
pub use repo::{BlobRepo, ContentBlobInfo, ContentBlobMeta, CreateChangeset, UploadHgEntry,
               UploadHgNodeHash};
pub use repo_commit::ChangesetHandle;
// TODO: This is exported for testing - is this the right place for it?
pub use repo_commit::compute_changed_files;
//
// TODO: (jsgf) T21597565 This is exposed here for blobimport -- don't use it for anything else.

pub use utils::RawNodeBlob;
