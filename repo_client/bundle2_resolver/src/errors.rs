/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::collections::HashSet;

pub use failure_ext::prelude::*;
use thiserror::Error;

use bookmarks::BookmarkName;
use mercurial_types::HgChangesetId;
use mononoke_types::ChangesetId;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Bonsai not found for hg changeset: {0:?}")]
    BonsaiNotFoundForHgChangeset(HgChangesetId),
    #[error("Malformed treemanifest part: {0}")]
    MalformedTreemanifestPart(String),
    #[error("Pushrebase onto bookmark not found: {0:?}")]
    PushrebaseBookmarkNotFound(BookmarkName),
    #[error("Only one head is allowed in pushed set")]
    PushrebaseTooManyHeads,
    #[error("Error while uploading data for changesets, hashes: {0:?}")]
    WhileUploadingData(Vec<HgChangesetId>),
    #[error("No common root found between: bookmark:{0:?} roots:{1:?}")]
    PushrebaseNoCommonRoot(BookmarkName, HashSet<ChangesetId>),
    #[error("Repo is marked as read-only: {0}")]
    RepoReadOnly(String),
}
