/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fmt;

use anyhow::{bail, Result};
use bookmarks_types::{BookmarkName, Freshness};
use context::CoreContext;
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use mercurial_types::HgChangesetId;
use mononoke_types::{ChangesetId, RawBundle2Id, RepositoryId, Timestamp};
use sql::mysql_async::prelude::{ConvIr, FromValue};
use sql::mysql_async::{FromValueError, Value};

/// Entry that describes an update to a bookmark
#[derive(Clone, Debug)]
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
    pub to_changeset_id: Option<ChangesetId>,
    /// New position of a bookmark. It can be None if the bookmark was deleted
    pub from_changeset_id: Option<ChangesetId>,
    /// Reason for a bookmark update
    pub reason: BookmarkUpdateReason,
    /// When update happened
    pub timestamp: Timestamp,
}

pub trait BookmarkUpdateLog: Send + Sync + 'static {
    /// Read the next up to `limit` entries from Bookmark update log. It either returns
    /// new log entries with id bigger than `id` or empty stream if there are no more
    /// log entries with bigger id.
    fn read_next_bookmark_log_entries(
        &self,
        ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
        limit: u64,
        freshness: Freshness,
    ) -> BoxStream<'static, Result<BookmarkUpdateLogEntry>>;

    /// Same as `read_next_bookmark_log_entries`, but limits the stream of returned entries
    /// to all have the same reason and bookmark
    fn read_next_bookmark_log_entries_same_bookmark_and_reason(
        &self,
        ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
        limit: u64,
    ) -> BoxStream<'static, Result<BookmarkUpdateLogEntry>>;

    /// Read the log entry for specific bookmark with specified to changeset id.
    fn list_bookmark_log_entries(
        &self,
        _ctx: CoreContext,
        name: BookmarkName,
        repo_id: RepositoryId,
        max_rec: u32,
        offset: Option<u32>,
        freshness: Freshness,
    ) -> BoxStream<'static, Result<(Option<ChangesetId>, BookmarkUpdateReason, Timestamp)>>;

    /// Count the number of BookmarkUpdateLog entries with id greater than the given value,
    /// possibly excluding a given reason.
    fn count_further_bookmark_log_entries(
        &self,
        _ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
        exclude_reason: Option<BookmarkUpdateReason>,
    ) -> BoxFuture<'static, Result<u64>>;

    /// Count the number of BookmarkUpdateLog entries with id greater than the given value
    fn count_further_bookmark_log_entries_by_reason(
        &self,
        _ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
    ) -> BoxFuture<'static, Result<Vec<(BookmarkUpdateReason, u64)>>>;

    /// Find the last contiguous BookmarkUpdateLog entry matching the reason provided.
    fn skip_over_bookmark_log_entries_with_reason(
        &self,
        ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
        reason: BookmarkUpdateReason,
    ) -> BoxFuture<'static, Result<Option<u64>>>;
}

/// Describes why a bookmark was moved
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BookmarkUpdateReason {
    Pushrebase {
        /// For now, let the bundle handle be not specified.
        /// We may change it later
        bundle_replay_data: Option<BundleReplayData>,
    },
    Push {
        /// For now, let the bundle handle be not specified.
        /// We may change it later
        bundle_replay_data: Option<BundleReplayData>,
    },
    Blobimport,
    /// Bookmark was moved manually i.e. via mononoke_admin tool
    ManualMove,
    /// Only used for tests, should never be used in production
    TestMove {
        bundle_replay_data: Option<BundleReplayData>,
    },
    /// Used during sync from a large repo into small repo.
    Backsyncer {
        bundle_replay_data: Option<BundleReplayData>,
    },
    XRepoSync,
}

impl std::fmt::Display for BookmarkUpdateReason {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use BookmarkUpdateReason::*;

        let s = match self {
            Pushrebase { .. } => "pushrebase",
            Push { .. } => "push",
            Blobimport => "blobimport",
            ManualMove => "manualmove",
            TestMove { .. } => "testmove",
            Backsyncer { .. } => "backsyncer",
            XRepoSync { .. } => "xreposync",
        };
        write!(f, "{}", s)
    }
}

impl BookmarkUpdateReason {
    pub fn update_bundle_replay_data(
        self,
        bundle_replay_data: Option<BundleReplayData>,
    ) -> Result<Self> {
        use BookmarkUpdateReason::*;
        match self {
            Pushrebase { .. } => Ok(Pushrebase { bundle_replay_data }),
            Push { .. } => Ok(Push { bundle_replay_data }),
            Blobimport | ManualMove | XRepoSync => match bundle_replay_data {
                Some(..) => bail!("internal error: bundle replay data can not be specified"),
                None => Ok(self),
            },
            TestMove { .. } => Ok(TestMove { bundle_replay_data }),
            Backsyncer { .. } => Ok(Backsyncer { bundle_replay_data }),
        }
    }

    pub fn into_bundle_replay_data(self) -> Option<BundleReplayData> {
        use BookmarkUpdateReason::*;
        match self {
            Pushrebase { bundle_replay_data }
            | Push { bundle_replay_data }
            | TestMove { bundle_replay_data }
            | Backsyncer { bundle_replay_data } => bundle_replay_data,
            Blobimport | ManualMove | XRepoSync => None,
        }
    }

    pub fn get_bundle_replay_data(&self) -> Option<&BundleReplayData> {
        use BookmarkUpdateReason::*;
        match self {
            Pushrebase {
                ref bundle_replay_data,
            }
            | Push {
                ref bundle_replay_data,
            }
            | TestMove {
                ref bundle_replay_data,
            }
            | Backsyncer {
                ref bundle_replay_data,
            } => bundle_replay_data.as_ref(),
            Blobimport | ManualMove | XRepoSync => None,
        }
    }
}

impl ConvIr<BookmarkUpdateReason> for BookmarkUpdateReason {
    fn new(v: Value) -> Result<Self, FromValueError> {
        match v {
            Value::Bytes(ref b) if b == &b"pushrebase" => Ok(BookmarkUpdateReason::Pushrebase {
                bundle_replay_data: None,
            }),
            Value::Bytes(ref b) if b == &b"push" => Ok(BookmarkUpdateReason::Push {
                bundle_replay_data: None,
            }),
            Value::Bytes(ref b) if b == &b"blobimport" => Ok(BookmarkUpdateReason::Blobimport),
            Value::Bytes(ref b) if b == &b"manualmove" => Ok(BookmarkUpdateReason::ManualMove),
            Value::Bytes(ref b) if b == &b"testmove" => Ok(BookmarkUpdateReason::TestMove {
                bundle_replay_data: None,
            }),
            Value::Bytes(ref b) if b == &b"backsyncer" => Ok(BookmarkUpdateReason::Backsyncer {
                bundle_replay_data: None,
            }),
            Value::Bytes(ref b) if b == &b"xreposync" => Ok(BookmarkUpdateReason::XRepoSync),
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
        match bookmark_update_reason {
            BookmarkUpdateReason::Pushrebase { .. } => Value::Bytes(b"pushrebase".to_vec()),
            BookmarkUpdateReason::Push { .. } => Value::Bytes(b"push".to_vec()),
            BookmarkUpdateReason::Blobimport { .. } => Value::Bytes(b"blobimport".to_vec()),
            BookmarkUpdateReason::ManualMove { .. } => Value::Bytes(b"manualmove".to_vec()),
            BookmarkUpdateReason::TestMove { .. } => Value::Bytes(b"testmove".to_vec()),
            BookmarkUpdateReason::Backsyncer { .. } => Value::Bytes(b"backsyncer".to_vec()),
            BookmarkUpdateReason::XRepoSync { .. } => Value::Bytes(b"xreposync".to_vec()),
        }
    }
}

/// Encapsulation of the data required to replay a Mercurial bundle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundleReplayData {
    pub bundle_handle: String,
    pub commit_timestamps: HashMap<HgChangesetId, Timestamp>,
}

impl BundleReplayData {
    pub fn new(raw_bundle2_id: RawBundle2Id) -> Self {
        Self {
            bundle_handle: raw_bundle2_id.to_hex().as_str().to_owned(),
            commit_timestamps: HashMap::new(),
        }
    }

    pub fn with_timestamps(mut self, commit_timestamps: HashMap<HgChangesetId, Timestamp>) -> Self {
        self.commit_timestamps = commit_timestamps;
        self
    }
}
