/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

mod bonsai_generation;
pub mod derive_hg_manifest;
pub mod file_history;
mod repo;
mod repo_commit;
mod utils;

pub use crate::errors::*;
pub use crate::repo::{save_bonsai_changesets, BlobRepo, CreateChangeset};
pub use crate::repo_commit::ChangesetHandle;
pub use changeset_fetcher::ChangesetFetcher;
// TODO: This is exported for testing - is this the right place for it?
pub use crate::repo_commit::{compute_changed_files, UploadEntries};
pub use utils::DangerousOverride;

pub mod internal {
    pub use crate::utils::{IncompleteFilenodeInfo, IncompleteFilenodes};
}

pub mod errors {
    pub use blobrepo_errors::*;
}

pub use filestore::StoreRequest;
