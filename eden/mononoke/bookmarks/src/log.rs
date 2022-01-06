/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

use anyhow::{anyhow, Result};
use bookmarks_types::{BookmarkName, Freshness};
use context::CoreContext;
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use mononoke_types::{ChangesetId, RepositoryId, Timestamp};
use sql::mysql;
use sql::mysql_async::prelude::{ConvIr, FromValue};
use sql::mysql_async::{FromValueError, Value};

/// Entry that describes an update to a bookmark
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BookmarkUpdateLogEntry {
    /// Number that sets a total order on single bookmark updates. It can be used to fetch
    /// new log entries
    pub id: i64,
    /// Id of a repo
    pub repo_id: RepositoryId,
    /// Name of the bookmark
    pub bookmark_name: BookmarkName,
    /// Previous position of bookmark if it's known. It might not be known if a bookmark was
    /// force set or if a bookmark didn't exist
    pub from_changeset_id: Option<ChangesetId>,
    /// New position of a bookmark. It can be None if the bookmark was deleted
    pub to_changeset_id: Option<ChangesetId>,
    /// Reason for a bookmark update
    pub reason: BookmarkUpdateReason,
    /// When update happened
    pub timestamp: Timestamp,
    /// Raw bundle replay data
    pub bundle_replay_data: Option<RawBundleReplayData>,
}

#[facet::facet]
pub trait BookmarkUpdateLog: Send + Sync + 'static {
    /// Read the next up to `limit` entries from Bookmark update log. It either returns
    /// new log entries with id bigger than `id` or empty stream if there are no more
    /// log entries with bigger id.
    fn read_next_bookmark_log_entries(
        &self,
        ctx: CoreContext,
        id: u64,
        limit: u64,
        freshness: Freshness,
    ) -> BoxStream<'static, Result<BookmarkUpdateLogEntry>>;

    /// Same as `read_next_bookmark_log_entries`, but limits the stream of returned entries
    /// to all have the same reason and bookmark
    fn read_next_bookmark_log_entries_same_bookmark_and_reason(
        &self,
        ctx: CoreContext,
        id: u64,
        limit: u64,
    ) -> BoxStream<'static, Result<BookmarkUpdateLogEntry>>;

    /// Read the log entry for specific bookmark with specified to changeset id.
    fn list_bookmark_log_entries(
        &self,
        _ctx: CoreContext,
        name: BookmarkName,
        max_rec: u32,
        offset: Option<u32>,
        freshness: Freshness,
    ) -> BoxStream<'static, Result<(u64, Option<ChangesetId>, BookmarkUpdateReason, Timestamp)>>;

    /// Read the log entry for specific bookmark with specified to changeset id. Filter by ts range.
    fn list_bookmark_log_entries_ts_in_range(
        &self,
        _ctx: CoreContext,
        name: BookmarkName,
        max_rec: u32,
        min_ts: Timestamp,
        max_ts: Timestamp,
    ) -> BoxStream<'static, Result<(u64, Option<ChangesetId>, BookmarkUpdateReason, Timestamp)>>;

    /// Count the number of BookmarkUpdateLog entries with id greater than the given value,
    /// possibly excluding a given reason.
    fn count_further_bookmark_log_entries(
        &self,
        _ctx: CoreContext,
        id: u64,
        exclude_reason: Option<BookmarkUpdateReason>,
    ) -> BoxFuture<'static, Result<u64>>;

    /// Count the number of BookmarkUpdateLog entries with id greater than the given value
    fn count_further_bookmark_log_entries_by_reason(
        &self,
        _ctx: CoreContext,
        id: u64,
    ) -> BoxFuture<'static, Result<Vec<(BookmarkUpdateReason, u64)>>>;

    /// Find the last contiguous BookmarkUpdateLog entry matching the reason provided.
    fn skip_over_bookmark_log_entries_with_reason(
        &self,
        ctx: CoreContext,
        id: u64,
        reason: BookmarkUpdateReason,
    ) -> BoxFuture<'static, Result<Option<u64>>>;

    fn get_largest_log_id(
        &self,
        ctx: CoreContext,
        freshness: Freshness,
    ) -> BoxFuture<'static, Result<Option<u64>>>;
}

/// Describes why a bookmark was moved
#[derive(Clone, Copy, Debug, Eq, PartialEq, mysql::OptTryFromRowField)]
pub enum BookmarkUpdateReason {
    /// Bookmark was updated by a pushrebase.
    Pushrebase,

    /// Bookmark was update by a plain push.
    Push,

    /// Bookmark was updated by blobimport.
    Blobimport,

    /// Bookmark was moved manually i.e. via mononoke_admin tool
    ManualMove,

    /// Bookmark was moved by test code.
    ///
    /// Only used for tests, should never be used in production
    TestMove,

    /// Bookmark was moved during a back-sync from a large repo into a small repo.
    Backsyncer,

    /// Bookmark was moved during a sync from a small repo into a large repo.
    XRepoSync,

    /// Bookmark was moved by an API request.
    ApiRequest,
}

impl std::fmt::Display for BookmarkUpdateReason {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use BookmarkUpdateReason::*;

        let s = match self {
            Pushrebase => "pushrebase",
            Push => "push",
            Blobimport => "blobimport",
            ManualMove => "manualmove",
            TestMove => "testmove",
            Backsyncer => "backsyncer",
            XRepoSync => "xreposync",
            ApiRequest => "apirequest",
        };
        write!(f, "{}", s)
    }
}

impl ConvIr<BookmarkUpdateReason> for BookmarkUpdateReason {
    fn new(v: Value) -> Result<Self, FromValueError> {
        use BookmarkUpdateReason::*;

        match v {
            Value::Bytes(ref b) if b == b"pushrebase" => Ok(Pushrebase),
            Value::Bytes(ref b) if b == b"push" => Ok(Push),
            Value::Bytes(ref b) if b == b"blobimport" => Ok(Blobimport),
            Value::Bytes(ref b) if b == b"manualmove" => Ok(ManualMove),
            Value::Bytes(ref b) if b == b"testmove" => Ok(TestMove),
            Value::Bytes(ref b) if b == b"backsyncer" => Ok(Backsyncer),
            Value::Bytes(ref b) if b == b"xreposync" => Ok(XRepoSync),
            Value::Bytes(ref b) if b == b"apirequest" => Ok(ApiRequest),
            v => Err(FromValueError(v)),
        }
    }

    fn commit(self) -> BookmarkUpdateReason {
        self
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for BookmarkUpdateReason {
    type Intermediate = BookmarkUpdateReason;
}

impl From<BookmarkUpdateReason> for Value {
    fn from(bookmark_update_reason: BookmarkUpdateReason) -> Self {
        use BookmarkUpdateReason::*;

        match bookmark_update_reason {
            Pushrebase => Value::Bytes(b"pushrebase".to_vec()),
            Push => Value::Bytes(b"push".to_vec()),
            Blobimport => Value::Bytes(b"blobimport".to_vec()),
            ManualMove => Value::Bytes(b"manualmove".to_vec()),
            TestMove => Value::Bytes(b"testmove".to_vec()),
            Backsyncer => Value::Bytes(b"backsyncer".to_vec()),
            XRepoSync => Value::Bytes(b"xreposync".to_vec()),
            ApiRequest => Value::Bytes(b"apirequest".to_vec()),
        }
    }
}

/// Encapsulation of the data required to replay a Mercurial bundle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawBundleReplayData {
    pub bundle_handle: String,
    pub commit_timestamps_json: String,
}

impl RawBundleReplayData {
    pub fn maybe_new(
        bundle_handle: Option<String>,
        commit_timestamps_json: Option<String>,
    ) -> Result<Option<Self>> {
        match (bundle_handle, commit_timestamps_json) {
            (Some(bundle_handle), Some(commit_timestamps_json)) => Ok(Some(RawBundleReplayData {
                bundle_handle,
                commit_timestamps_json,
            })),
            (None, None) => Ok(None),
            _ => Err(anyhow!("inconsistent replay data")),
        }
    }
}

pub trait BundleReplay: Sync {
    fn to_raw(&self) -> Result<RawBundleReplayData>;
}

impl BundleReplay for RawBundleReplayData {
    fn to_raw(&self) -> Result<RawBundleReplayData> {
        Ok(self.clone())
    }
}
