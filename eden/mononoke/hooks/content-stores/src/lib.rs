/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod errors;
mod memory;
mod repo;
mod store;
mod text_only;

pub use crate::memory::InMemoryFileContentManager;
pub use crate::memory::InMemoryFileText;
pub use crate::repo::RepoFileContentManager;
pub use crate::text_only::TextOnlyFileContentManager;
pub use store::FileChange;
pub use store::FileContentManager;
pub use store::PathContent;

use bookmarks::BookmarksArc;
use errors::ErrorKind;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::RepoDerivedDataArc;

pub fn repo_text_only_fetcher(
    repo: &(impl RepoBlobstoreArc + BookmarksArc + RepoDerivedDataArc),
    max_file_size: u64,
) -> Box<dyn FileContentManager> {
    let store = RepoFileContentManager::new(repo);
    Box::new(TextOnlyFileContentManager::new(store, max_file_size))
}
