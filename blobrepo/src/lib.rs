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

mod bonsai_generation;
pub mod derive_hg_manifest;
mod envelope;
pub mod file;
pub mod file_history;
mod filenode_lookup;
mod manifest;
mod repo;
mod repo_commit;
mod utils;

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
pub use utils::UnittestOverride;

pub mod internal {
    pub use crate::utils::{IncompleteFilenodeInfo, IncompleteFilenodes};
}

pub mod errors {
    pub use blobrepo_errors::*;
}

pub use filestore::StoreRequest;
