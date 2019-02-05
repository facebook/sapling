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
#[macro_use]
extern crate slog;
#[macro_use]
extern crate stats;
#[macro_use]
extern crate futures_ext;
#[cfg(test)]
#[macro_use]
extern crate maplit;
extern crate blobrepo_errors as errors;

pub mod alias;
mod bonsai_generation;
mod file;
mod manifest;
mod memory_manifest;
mod repo;
mod repo_commit;
mod utils;

pub use crate::alias::*;
pub use crate::errors::*;
pub use crate::file::HgBlobEntry;
pub use crate::manifest::BlobManifest;
pub use crate::repo::{
    save_bonsai_changesets, BlobRepo, ContentBlobInfo, ContentBlobMeta, CreateChangeset,
    UploadHgFileContents, UploadHgFileEntry, UploadHgNodeHash, UploadHgTreeEntry,
};
pub use crate::repo_commit::ChangesetHandle;
pub use blob_changeset::{ChangesetMetadata, HgBlobChangeset, HgChangesetContent};
pub use changeset_fetcher::ChangesetFetcher;
// TODO: This is exported for testing - is this the right place for it?
pub use crate::repo_commit::compute_changed_files;

pub mod internal {
    pub use crate::memory_manifest::{MemoryManifestEntry, MemoryRootManifest};
    pub use crate::utils::{IncompleteFilenodeInfo, IncompleteFilenodes};
}
