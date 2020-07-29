/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use bookmarks::BookmarkUpdateLogEntry;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("replay data is missing for id {id}")]
    ReplayDataMissing { id: i64 },
    #[error("unexpected bookmark move: {0}")]
    UnexpectedBookmarkMove(String),
    #[error("sync failed for ids {ids:?}")]
    SyncFailed {
        ids: Vec<i64>,
        #[source]
        cause: Error,
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
