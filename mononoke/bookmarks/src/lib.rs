/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]
#![feature(never_type)]

use anyhow::{bail, Error, Result};
use context::CoreContext;
use futures_ext::{BoxFuture, BoxStream};
use mercurial_types::HgChangesetId;
use mononoke_types::{ChangesetId, RawBundle2Id, RepositoryId, Timestamp};
use sql::mysql_async::{
    prelude::{ConvIr, FromValue},
    FromValueError, Value,
};
use sql_ext::TransactionResult;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

mod cache;
pub use bookmarks_types::{
    Bookmark, BookmarkHgKind, BookmarkName, BookmarkPrefix, BookmarkPrefixRange, Freshness,
};
pub use cache::CachedBookmarks;

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

pub trait Bookmarks: Send + Sync + 'static {
    /// Returns Some(ChangesetId) if bookmark exists, returns None if doesn't
    fn get(
        &self,
        ctx: CoreContext,
        name: &BookmarkName,
        repoid: RepositoryId,
    ) -> BoxFuture<Option<ChangesetId>, Error>;

    // TODO(stash): do we need to have a separate methods list_all() to avoid accidentally
    // listing all the bookmarks?

    /// List publishing bookmarks that match a given prefix. There should normally be few, it's
    /// reasonable to pass an empty prefix here.
    fn list_publishing_by_prefix(
        &self,
        ctx: CoreContext,
        prefix: &BookmarkPrefix,
        repoid: RepositoryId,
        freshness: Freshness,
    ) -> BoxStream<(Bookmark, ChangesetId), Error>;

    /// List pull default bookmarks that match a given prefix. There should normally be few, it's
    /// reasonable to pass an empty prefix here.
    fn list_pull_default_by_prefix(
        &self,
        ctx: CoreContext,
        prefix: &BookmarkPrefix,
        repoid: RepositoryId,
        freshness: Freshness,
    ) -> BoxStream<(Bookmark, ChangesetId), Error>;

    /// List all bookmarks that match the prefix. You should not normally call this with an empty
    /// prefix. Provide a max, which is an (exclusive!) limit representing how many bookmarks
    /// will be returned. If more bookmarks are found, an error will be rerturned (there is no
    /// provision for paging through results).
    fn list_all_by_prefix(
        &self,
        ctx: CoreContext,
        prefix: &BookmarkPrefix,
        repoid: RepositoryId,
        freshness: Freshness,
        max: u64,
    ) -> BoxStream<(Bookmark, ChangesetId), Error>;

    /// Creates a transaction that will be used for write operations.
    fn create_transaction(&self, ctx: CoreContext, repoid: RepositoryId) -> Box<dyn Transaction>;

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
    ) -> BoxStream<BookmarkUpdateLogEntry, Error>;

    /// Same as `read_next_bookmark_log_entries`, but limits the stream of returned entries
    /// to all have the same reason and bookmark
    fn read_next_bookmark_log_entries_same_bookmark_and_reason(
        &self,
        ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
        limit: u64,
    ) -> BoxStream<BookmarkUpdateLogEntry, Error>;

    /// Read the log entry for specific bookmark with specified to changeset id.
    fn list_bookmark_log_entries(
        &self,
        _ctx: CoreContext,
        name: BookmarkName,
        repo_id: RepositoryId,
        max_rec: u32,
    ) -> BoxStream<(Option<ChangesetId>, BookmarkUpdateReason, Timestamp), Error>;

    /// Count the number of BookmarkUpdateLog entries with id greater than the given value,
    /// possibly excluding a given reason.
    fn count_further_bookmark_log_entries(
        &self,
        _ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
        exclude_reason: Option<BookmarkUpdateReason>,
    ) -> BoxFuture<u64, Error>;

    /// Count the number of BookmarkUpdateLog entries with id greater than the given value
    fn count_further_bookmark_log_entries_by_reason(
        &self,
        _ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
    ) -> BoxFuture<Vec<(BookmarkUpdateReason, u64)>, Error>;

    /// Find the last contiguous BookmarkUpdateLog entry matching the reason provided.
    fn skip_over_bookmark_log_entries_with_reason(
        &self,
        ctx: CoreContext,
        id: u64,
        repoid: RepositoryId,
        reason: BookmarkUpdateReason,
    ) -> BoxFuture<Option<u64>, Error>;
}

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

pub trait Transaction: Send + Sync + 'static {
    /// Adds set() operation to the transaction set.
    /// Updates a bookmark's value. Bookmark should already exist and point to `old_cs`, otherwise
    /// committing the transaction will fail. The Bookmark should also not be Scratch.
    fn update(
        &mut self,
        key: &BookmarkName,
        new_cs: ChangesetId,
        old_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()>;

    /// Adds create() operation to the transaction set.
    /// Creates a bookmark. BookmarkName should not already exist, otherwise committing the
    /// transaction will fail. The resulting Bookmark will be PushDefault.
    fn create(
        &mut self,
        key: &BookmarkName,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()>;

    /// Adds force_set() operation to the transaction set.
    /// Unconditionally sets the new value of the bookmark. Succeeds regardless of whether bookmark
    /// exists or not.
    fn force_set(
        &mut self,
        key: &BookmarkName,
        new_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()>;

    /// Adds delete operation to the transaction set.
    /// Deletes bookmark only if it currently points to `old_cs`.
    fn delete(
        &mut self,
        key: &BookmarkName,
        old_cs: ChangesetId,
        reason: BookmarkUpdateReason,
    ) -> Result<()>;

    /// Adds force_delete operation to the transaction set.
    /// Deletes bookmark unconditionally.
    fn force_delete(&mut self, key: &BookmarkName, reason: BookmarkUpdateReason) -> Result<()>;

    /// Adds an infinitepush update operation to the transaction set.
    /// Updates the changeset referenced by the bookmark, if it is already a scratch bookmark.
    fn update_infinitepush(
        &mut self,
        key: &BookmarkName,
        new_cs: ChangesetId,
        old_cs: ChangesetId,
    ) -> Result<()>;

    /// Adds an infinitepush create operation to the transaction set.
    /// Creates a new bookmark, configured as scratch. It shuld not exist already.
    fn create_infinitepush(&mut self, key: &BookmarkName, new_cs: ChangesetId) -> Result<()>;

    /// Commits the transaction. Future succeeds if transaction has been
    /// successful, or errors if transaction has failed. Logical failure is indicated by
    /// returning a successful `false` value; infrastructure failure is reported via an Error.
    fn commit(self: Box<Self>) -> BoxFuture<bool, Error>;

    /// Commits the transaction using provided transaction. If bookmarks implementation
    /// is not support committing into transactions, then it should return an error.
    /// Future succeeds if transaction has been
    /// successful, or errors if transaction has failed. Logical failure is indicated by
    /// returning a successful `false` value; infrastructure failure is reported via an Error.
    fn commit_into_txn(
        self: Box<Self>,
        txn_factory: Arc<dyn Fn() -> BoxFuture<TransactionResult, Error> + Sync + Send>,
    ) -> BoxFuture<bool, Error>;
}
