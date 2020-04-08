/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]
use std::sync::Arc;

mod blobrepo;
mod errors;
mod memory;
mod store;
mod text_only;

pub use crate::blobrepo::{BlobRepoChangesetStore, BlobRepoFileContentStore};
pub use crate::memory::{InMemoryChangesetStore, InMemoryFileContentStore};
pub use crate::text_only::TextOnlyFileContentStore;
pub use store::{ChangedFileType, ChangesetStore, FileContentStore};

pub use crate::blobrepo::BlobRepoFileContentFetcher;
pub use crate::memory::{InMemoryFileContentFetcher, InMemoryFileText};
pub use crate::text_only::TextOnlyFileContentFetcher;
pub use store::FileContentFetcher;

use errors::ErrorKind;

pub fn blobrepo_text_only_fetcher(
    blobrepo: ::blobrepo::BlobRepo,
    max_file_size: u64,
) -> Box<dyn FileContentFetcher> {
    let store = BlobRepoFileContentFetcher::new(blobrepo);
    Box::new(TextOnlyFileContentFetcher::new(store, max_file_size))
}

pub fn blobrepo_text_only_store(
    blobrepo: ::blobrepo::BlobRepo,
    max_file_size: u64,
) -> Arc<dyn FileContentStore> {
    let store = BlobRepoFileContentStore::new(blobrepo);
    Arc::new(TextOnlyFileContentStore::new(store, max_file_size))
}
