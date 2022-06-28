/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateLogEntry;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("unexpected bookmark move: {0}")]
    UnexpectedBookmarkMove(String),
    #[error("sync failed for ids {ids:?}")]
    SyncFailed {
        ids: Vec<i64>,
        #[source]
        cause: Error,
    },
    #[error(
        "Programming error: cannot prepare a bundle for entry ids #{ids:?}: \
        entry {entry_id} modifies bookmark {entry_bookmark_name}, while bundle moves {bundle_bookmark_name}"
    )]
    BookmarkMismatchInBundleCombining {
        ids: Vec<i64>,
        entry_id: i64,
        entry_bookmark_name: BookmarkName,
        bundle_bookmark_name: BookmarkName,
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

    #[error("error without tracking entry")]
    AnonymousError {
        #[source]
        cause: Error,
    },
}
