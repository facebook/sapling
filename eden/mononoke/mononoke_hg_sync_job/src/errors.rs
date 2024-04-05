/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateLogEntry;
use bookmarks::BookmarkUpdateLogId;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("unexpected bookmark move: {0}")]
    UnexpectedBookmarkMove(String),
    #[error("sync failed for ids {ids:?}")]
    SyncFailed {
        ids: Vec<BookmarkUpdateLogId>,
        #[source]
        cause: Error,
    },
    #[error(
        "Programming error: cannot prepare a bundle for entry ids #{ids:?}: \
        entry {entry_id} modifies bookmark {entry_bookmark_name}, while bundle moves {bundle_bookmark_name}"
    )]
    BookmarkMismatchInBundleCombining {
        ids: Vec<BookmarkUpdateLogId>,
        entry_id: BookmarkUpdateLogId,
        entry_bookmark_name: BookmarkKey,
        bundle_bookmark_name: BookmarkKey,
    },
}

#[derive(Debug, Error)]
pub enum PipelineError {
    #[error("error processing entries {entries:?}")]
    EntryError {
        entries: Vec<BookmarkUpdateLogEntry>,
        #[source]
        cause: Error,
    },

    #[allow(dead_code)]
    #[error("error without tracking entry")]
    AnonymousError {
        #[source]
        cause: Error,
    },
}
