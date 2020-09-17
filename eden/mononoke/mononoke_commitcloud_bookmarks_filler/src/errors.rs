/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use bookmarks::BookmarkName;
use mercurial_types::HgChangesetId;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Bookmark transaction failed")]
    BookmarkTransactionFailed,

    #[error("Bookmark does not match scratch namespace: {0:?}")]
    InvalidBookmarkForNamespace(BookmarkName),

    #[error("HG Changeset does not exist: {0:?}")]
    HgChangesetDoesNotExist(HgChangesetId),

    #[error("An error ocurred interacting with the blob repo")]
    BlobRepoError(#[source] Error),
}
