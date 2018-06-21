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
#[macro_use]
extern crate lazy_static;
extern crate serde;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate slog;
#[macro_use]
extern crate stats;
extern crate tokio_core;
extern crate uuid;

extern crate heapsize;

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
extern crate mercurial;
extern crate mercurial_types;
extern crate mononoke_types;
extern crate rocksblob;
extern crate rocksdb;
extern crate scuba_ext;
extern crate time_ext;

#[cfg(test)]
extern crate async_unit;
#[cfg(test)]
extern crate many_files_dirs;
#[cfg(test)]
extern crate mercurial_types_mocks;

mod repo;
mod changeset;
mod manifest;
mod memory_manifest;
mod file;
mod errors;
mod utils;
mod repo_commit;

pub use errors::*;

pub use changeset::BlobChangeset;
pub use file::HgBlobEntry;
pub use manifest::BlobManifest;
pub use repo::{BlobRepo, ContentBlobInfo, ContentBlobMeta, CreateChangeset, UploadHgFileContents,
               UploadHgFileEntry, UploadHgNodeHash, UploadHgTreeEntry};
pub use repo_commit::ChangesetHandle;
// TODO: This is exported for testing - is this the right place for it?
pub use repo_commit::compute_changed_files;
pub use utils::RawNodeBlob;
