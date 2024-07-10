/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use bookmarks::BookmarkUpdateLogEntry;
use bookmarks::BookmarkUpdateLogId;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("sync failed for ids {ids:?}")]
    SyncFailed {
        ids: Vec<BookmarkUpdateLogId>,
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

    #[allow(dead_code)]
    #[error("error without tracking entry")]
    AnonymousError {
        #[source]
        cause: Error,
    },
}
