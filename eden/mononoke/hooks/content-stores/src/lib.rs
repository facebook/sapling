/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]
use std::sync::Arc;

mod blobrepo;
mod errors;
mod memory;
mod store;
mod text_only;

pub use crate::blobrepo::{BlobRepoChangesetStore, BlobRepoFileContentStore};
pub use crate::memory::{InMemoryChangesetStore, InMemoryFileContentStore, InMemoryFileText};
pub use crate::text_only::TextOnlyFileContentStore;
pub use store::{ChangedFileType, ChangesetStore, FileContentStore};

use errors::ErrorKind;

pub fn blobrepo_text_only_store(
    blobrepo: ::blobrepo::BlobRepo,
    max_file_size: u64,
) -> Arc<dyn FileContentStore> {
    let store = BlobRepoFileContentStore::new(blobrepo);
    Arc::new(TextOnlyFileContentStore::new(store, max_file_size))
}
