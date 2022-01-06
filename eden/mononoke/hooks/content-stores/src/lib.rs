/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]
mod blobrepo;
mod errors;
mod memory;
mod store;
mod text_only;

pub use crate::blobrepo::BlobRepoFileContentManager;
pub use crate::memory::{InMemoryFileContentManager, InMemoryFileText};
pub use crate::text_only::TextOnlyFileContentManager;
pub use store::{FileChange, FileContentManager, PathContent};

use errors::ErrorKind;

pub fn blobrepo_text_only_fetcher(
    blobrepo: ::blobrepo::BlobRepo,
    max_file_size: u64,
) -> Box<dyn FileContentManager> {
    let store = BlobRepoFileContentManager::new(blobrepo);
    Box::new(TextOnlyFileContentManager::new(store, max_file_size))
}
