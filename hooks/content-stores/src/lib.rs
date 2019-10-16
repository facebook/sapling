/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

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
