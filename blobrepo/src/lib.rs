// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

#[macro_use]
extern crate cloned;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate tokio;

extern crate bincode;
extern crate bonsai_utils;
extern crate bytes;
#[macro_use]
extern crate slog;
#[macro_use]
extern crate stats;
extern crate tracing;
extern crate uuid;

extern crate heapsize;

extern crate futures_stats;

extern crate ascii;
extern crate blob_changeset;
extern crate blobrepo_errors as errors;
extern crate blobstore;
extern crate bonsai_hg_mapping;
extern crate bookmarks;
extern crate changeset_fetcher;
extern crate changesets;
extern crate context;
extern crate crypto;
extern crate filenodes;
#[macro_use]
extern crate futures_ext;
#[cfg(test)]
#[macro_use]
extern crate maplit;
extern crate mercurial;
extern crate mercurial_types;
extern crate metaconfig_types;
extern crate mononoke_types;
#[cfg(test)]
extern crate mononoke_types_mocks;
extern crate post_commit;
extern crate scuba;
extern crate scuba_ext;
extern crate time_ext;

#[cfg(test)]
extern crate async_unit;
#[cfg(test)]
extern crate fixtures;
#[cfg(test)]
extern crate mercurial_types_mocks;

pub mod alias;
mod bonsai_generation;
mod file;
mod manifest;
mod memory_manifest;
mod repo;
mod repo_commit;
mod utils;

pub use alias::*;
pub use blob_changeset::{ChangesetMetadata, HgBlobChangeset, HgChangesetContent};
pub use changeset_fetcher::ChangesetFetcher;
pub use errors::*;
pub use file::HgBlobEntry;
pub use manifest::BlobManifest;
pub use repo::{
    save_bonsai_changesets, BlobRepo, ContentBlobInfo, ContentBlobMeta, CreateChangeset,
    UploadHgFileContents, UploadHgFileEntry, UploadHgNodeHash, UploadHgTreeEntry,
};
pub use repo_commit::ChangesetHandle;
// TODO: This is exported for testing - is this the right place for it?
pub use repo_commit::compute_changed_files;

pub mod internal {
    pub use memory_manifest::{MemoryManifestEntry, MemoryRootManifest};
    pub use utils::{IncompleteFilenodeInfo, IncompleteFilenodes};
}
