/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]
#![feature(never_type)]

use ascii::{AsciiChar, AsciiString};
use context::CoreContext;
use failure_ext::{err_msg, format_err, Error, Result};
use futures_ext::{BoxFuture, BoxStream};
use mercurial_types::HgChangesetId;
use mononoke_types::{ChangesetId, RawBundle2Id, RepositoryId, Timestamp};
use quickcheck::{Arbitrary, Gen};
use rand::Rng;
use sql::mysql_async::{
    prelude::{ConvIr, FromValue},
    FromValueError, Value,
};
use std::collections::HashMap;
use std::fmt;
use std::ops::{Bound, Range, RangeBounds, RangeFrom, RangeFull};

mod cache;
pub use cache::CachedBookmarks;

/// This enum represents how fresh you want results to be. MostRecent will go to the master, so you
/// normally don't want to issue queries using MostRecent unless you have a very good reason.
/// MaybeStale will go to a replica, which might lag behind the master (there is no SLA on
/// replication lag). MaybeStale reads might also be served from a local cache.
#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum Freshness {
    MostRecent,
    MaybeStale,
}

impl Arbitrary for Freshness {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        use Freshness::*;

        match g.gen_range(0, 2) {
            0 => MostRecent,
            1 => MaybeStale,
            _ => unreachable!(),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Bookmark {
    pub(crate) name: BookmarkName,
    pub(crate) hg_kind: BookmarkHgKind,
}

impl Arbitrary for Bookmark {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        let name = BookmarkName::arbitrary(g);
        Self {
            name,
            hg_kind: Arbitrary::arbitrary(g),
        }
    }
}

impl Bookmark {
    pub fn new(name: BookmarkName, hg_kind: BookmarkHgKind) -> Self {
        Bookmark { name, hg_kind }
    }

    pub fn into_name(self) -> BookmarkName {
        self.name
    }

    pub fn name(&self) -> &BookmarkName {
        &self.name
    }

    pub fn hg_kind(&self) -> &BookmarkHgKind {
        &self.hg_kind
    }

    pub fn publishing(&self) -> bool {
        use BookmarkHgKind::*;

        match self.hg_kind {
            Scratch => false,
            PublishingNotPullDefault => true,
            PullDefault => true,
        }
    }

    pub fn pull_default(&self) -> bool {
        use BookmarkHgKind::*;

        match self.hg_kind {
            Scratch => false,
            PublishingNotPullDefault => false,
            PullDefault => true,
        }
    }
}

type FromValueResult<T> = ::std::result::Result<T, FromValueError>;

#[derive(Clone, Debug, Eq, Hash, PartialEq, Ord, PartialOrd)]
pub struct BookmarkName {
    bookmark: AsciiString,
}

impl fmt::Display for BookmarkName {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.bookmark)
    }
}

impl Arbitrary for BookmarkName {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        // NOTE: We use a specific large size here because our tests exercise DB Bookmarks, which
        // require unique names in the DB.
        let size = 128;
        let mut bookmark = AsciiString::with_capacity(size);
        for _ in 0..size {
            bookmark.push(ascii_ext::AsciiChar::arbitrary(g).0);
        }
        Self { bookmark }
    }
}

impl BookmarkName {
    pub fn new<B: AsRef<str>>(bookmark: B) -> Result<Self> {
        Ok(Self {
            bookmark: AsciiString::from_ascii(bookmark.as_ref())
                .map_err(|bytes| format_err!("non-ascii bookmark name: {:?}", bytes))?,
        })
    }

    pub fn new_ascii(bookmark: AsciiString) -> Self {
        Self { bookmark }
    }

    pub fn as_ascii(&self) -> &AsciiString {
        &self.bookmark
    }

    pub fn to_string(&self) -> String {
        self.bookmark.clone().into()
    }

    pub fn into_string(self) -> String {
        self.bookmark.into()
    }

    pub fn as_str(&self) -> &str {
        self.bookmark.as_str()
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct BookmarkPrefix {
    bookmark_prefix: AsciiString,
}

impl fmt::Display for BookmarkPrefix {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.bookmark_prefix)
    }
}

pub enum BookmarkPrefixRange {
    Range(Range<BookmarkName>),
    RangeFrom(RangeFrom<BookmarkName>),
    RangeFull(RangeFull),
}

impl RangeBounds<BookmarkName> for BookmarkPrefixRange {
    fn start_bound(&self) -> Bound<&BookmarkName> {
        use BookmarkPrefixRange::*;
        match self {
            Range(r) => r.start_bound(),
            RangeFrom(r) => r.start_bound(),
            RangeFull(r) => r.start_bound(),
        }
    }

    fn end_bound(&self) -> Bound<&BookmarkName> {
        use BookmarkPrefixRange::*;
        match self {
            Range(r) => r.end_bound(),
            RangeFrom(r) => r.end_bound(),
            RangeFull(r) => r.end_bound(),
        }
    }
}

impl BookmarkPrefix {
    pub fn new<B: AsRef<str>>(bookmark_prefix: B) -> Result<Self> {
        Ok(Self {
            bookmark_prefix: AsciiString::from_ascii(bookmark_prefix.as_ref())
                .map_err(|bytes| format_err!("non-ascii bookmark prefix: {:?}", bytes))?,
        })
    }

    pub fn new_ascii(bookmark_prefix: AsciiString) -> Self {
        Self { bookmark_prefix }
    }

    pub fn empty() -> Self {
        Self {
            bookmark_prefix: AsciiString::default(),
        }
    }

    pub fn to_string(&self) -> String {
        self.bookmark_prefix.clone().into()
    }

    pub fn is_empty(&self) -> bool {
        self.bookmark_prefix.is_empty()
    }

    pub fn to_range(&self) -> BookmarkPrefixRange {
        match prefix_to_range_end(self.bookmark_prefix.clone()) {
            Some(range_end) => {
                let range = Range {
                    start: BookmarkName::new_ascii(self.bookmark_prefix.clone()),
                    end: BookmarkName::new_ascii(range_end),
                };
                BookmarkPrefixRange::Range(range)
            }
            None => match self.bookmark_prefix.len() {
                0 => BookmarkPrefixRange::RangeFull(RangeFull),
                _ => {
                    let range = RangeFrom {
                        start: BookmarkName::new_ascii(self.bookmark_prefix.clone()),
                    };
                    BookmarkPrefixRange::RangeFrom(range)
                }
            },
        }
    }
}

fn prefix_to_range_end(mut prefix: AsciiString) -> Option<AsciiString> {
    // If we have a prefix, then we need to take the last character of the prefix, increment it by
    // 1, then take that as an ASCII char. So, if you prefix is foobarA, then the range will be
    // from foobarA (inclusive) to foobarB (exclusive). Basically, what we're trying to implement
    // here is a little bit like Ruby's str#next.
    loop {
        match prefix.pop() {
            Some(chr) => match AsciiChar::from_ascii(chr.as_byte() + 1) {
                Ok(next_chr) => {
                    // Happy path, we found the next character, so just put that in and move on.
                    prefix.push(next_chr);
                    return Some(prefix);
                }
                Err(_) => {
                    // The last character doesn't fit in ASCII (i.e. it's DEL). This means we have
                    // something like foobaA[DEL]. In this case, we need to set the bound to be the
                    // character after the one before the DEL, so we want foobaB[DEL]
                    continue;
                }
            },
            None => {
                // We exhausted the entire string. This will only happen if the string is 0 or more
                // DEL characters. In this case, return the fact that no next string can be
                // produced.
                return None;
            }
        }
    }
}

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
            Blobimport | ManualMove => match bundle_replay_data {
                Some(..) => Err(err_msg(
                    "internal error: bundle replay data can not be specified",
                )),
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
            Blobimport | ManualMove => None,
        }
    }
}

impl ConvIr<BookmarkUpdateReason> for BookmarkUpdateReason {
    fn new(v: Value) -> FromValueResult<Self> {
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
}

impl From<BookmarkName> for Value {
    fn from(bookmark: BookmarkName) -> Self {
        Value::Bytes(bookmark.bookmark.into())
    }
}

impl ConvIr<BookmarkName> for BookmarkName {
    fn new(v: Value) -> FromValueResult<Self> {
        match v {
            Value::Bytes(bytes) => AsciiString::from_ascii(bytes)
                .map_err(|err| FromValueError(Value::Bytes(err.into_source())))
                .map(BookmarkName::new_ascii),
            v => Err(FromValueError(v)),
        }
    }

    fn commit(self) -> BookmarkName {
        self
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for BookmarkName {
    type Intermediate = BookmarkName;
}

impl From<BookmarkPrefix> for Value {
    fn from(bookmark_prefix: BookmarkPrefix) -> Self {
        Value::Bytes(bookmark_prefix.bookmark_prefix.into())
    }
}

/// Describes the behavior of a Bookmark in Mercurial operations.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Copy)]
pub enum BookmarkHgKind {
    Scratch,
    PublishingNotPullDefault,
    /// NOTE: PullDefault implies Publishing.
    PullDefault,
}

impl std::fmt::Display for BookmarkHgKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use BookmarkHgKind::*;

        let s = match self {
            Scratch => "scratch",
            PublishingNotPullDefault => "publishing",
            PullDefault => "pull_default",
        };

        write!(f, "{}", s)
    }
}

const SCRATCH_HG_KIND: &[u8] = b"scratch";
const PUBLISHING_HG_KIND: &[u8] = b"publishing";
const PULL_DEFAULT_HG_KIND: &[u8] = b"pull_default";

impl ConvIr<BookmarkHgKind> for BookmarkHgKind {
    fn new(v: Value) -> FromValueResult<Self> {
        use BookmarkHgKind::*;

        match v {
            Value::Bytes(ref b) if b == &SCRATCH_HG_KIND => Ok(Scratch),
            Value::Bytes(ref b) if b == &PUBLISHING_HG_KIND => Ok(PublishingNotPullDefault),
            Value::Bytes(ref b) if b == &PULL_DEFAULT_HG_KIND => Ok(PullDefault),
            v => Err(FromValueError(v)),
        }
    }

    fn commit(self) -> BookmarkHgKind {
        self
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for BookmarkHgKind {
    type Intermediate = BookmarkHgKind;
}

impl From<BookmarkHgKind> for Value {
    fn from(bookmark_update_reason: BookmarkHgKind) -> Self {
        use BookmarkHgKind::*;

        match bookmark_update_reason {
            Scratch => Value::Bytes(SCRATCH_HG_KIND.to_vec()),
            PublishingNotPullDefault => Value::Bytes(PUBLISHING_HG_KIND.to_vec()),
            PullDefault => Value::Bytes(PULL_DEFAULT_HG_KIND.to_vec()),
        }
    }
}

impl Arbitrary for BookmarkHgKind {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        use BookmarkHgKind::*;

        match g.gen_range(0, 3) {
            0 => Scratch,
            1 => PublishingNotPullDefault,
            2 => PullDefault,
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::quickcheck;

    quickcheck! {
        fn test_prefix_range_contains_self(bookmark: Bookmark) -> bool {
            let prefix = BookmarkPrefix::new_ascii(bookmark.name().as_ascii().clone());
            prefix.to_range().contains(bookmark.name())
        }

        fn test_prefix_range_contains_its_suffixes(bookmark: Bookmark, more: ascii_ext::AsciiString) -> bool {
            let prefix = BookmarkPrefix::new_ascii(bookmark.name().as_ascii().clone());
            let mut name = bookmark.name().as_ascii().clone();
            name.push_str(&more.0);
            prefix.to_range().contains(&BookmarkName::new_ascii(name))
        }

        fn test_prefix_range_does_not_contains_its_prefixes(bookmark: Bookmark, chr: ascii_ext::AsciiChar) -> bool {
            let mut prefix = bookmark.name().as_ascii().clone();
            prefix.push(chr.0);
            let prefix = BookmarkPrefix::new_ascii(prefix);

            !prefix.to_range().contains(bookmark.name())
        }

        fn test_empty_range_contains_any(bookmark: Bookmark) -> bool {
            let prefix = BookmarkPrefix::empty();
            prefix.to_range().contains(bookmark.name())
        }
    }
}
