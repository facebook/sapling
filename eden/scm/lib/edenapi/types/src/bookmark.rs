/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(any(test, feature = "for-tests"))]
use quickcheck_arbitrary_derive::Arbitrary;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use type_macros::auto_wire;
use types::hgid::HgId;

use crate::ServerError;
use crate::commitid::BonsaiChangesetId;
use crate::land::PushVar;

/// Which kind of bookmark to include in results.
/// Mirrors the server-side `BookmarkKind` from `bookmarks_types`.
#[auto_wire]
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize
)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub enum BookmarkKind {
    #[id(1)]
    Scratch,
    #[id(2)]
    Publishing,
    #[id(3)]
    #[default]
    PullDefaultPublishing,
}

#[auto_wire]
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct BookmarkRequest {
    #[id(0)]
    pub bookmarks: Vec<String>,
}

#[auto_wire]
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct Bookmark2Request {
    #[id(0)]
    pub bookmarks: Vec<String>,
    #[id(1)]
    pub freshness: Freshness,
}

#[auto_wire]
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct BookmarkEntry {
    #[id(1)]
    pub bookmark: String,
    #[id(2)]
    pub hgid: Option<HgId>,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct SetBookmarkRequest {
    #[id(0)]
    pub bookmark: String,

    #[id(1)]
    pub to: Option<HgId>,

    #[id(2)]
    pub from: Option<HgId>,

    #[id(4)]
    pub pushvars: Vec<PushVar>,
}

#[auto_wire]
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct SetBookmarkResponse {
    #[id(0)]
    #[no_default]
    pub data: Result<(), ServerError>,
}

/// Mirrors the server-side `BookmarkUpdateReason`. modern_sync copies a source
/// bookmarks_update_log entry's reason to a `*_shadow` replica so the replica's
/// log matches the source's row for row.
#[auto_wire]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub enum MirrorBookmarkUpdateReason {
    #[id(1)]
    #[default] // Wire requires a default value, shouldn't be used
    Pushrebase,
    #[id(2)]
    Push,
    #[id(3)]
    Blobimport,
    #[id(4)]
    ManualMove,
    #[id(5)]
    TestMove,
    #[id(6)]
    Backsyncer,
    #[id(7)]
    XRepoSync,
    #[id(8)]
    ApiRequest,
    #[id(9)]
    MultiRepoLand,
}

/// One bookmark move in a mirrored chain. Carries the source repo's
/// bookmarks_update_log entry id, its from/to changesets, and its reason, so
/// the replica reuses all three and its log row matches the source 1:1.
///
/// `to` is always required. modern_sync only mirrors the main publishing
/// bookmark (fbsource `master`), which is never deleted, so a move never clears
/// the bookmark. `from` is `None` only for the first move of a brand-new repo,
/// when the bookmark is created; every later move sets `from` to the previous
/// move's `to`.
///
/// The changesets are bonsai ids. The source's bookmarks_update_log holds
/// bonsai, and modern_sync uploads each changeset to the replica under the
/// source's own bonsai id, so both sides name the changeset the same way and
/// neither end has to translate.
#[auto_wire]
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct MirrorBookmarkMove {
    #[id(0)]
    pub log_id: u64,

    #[id(1)]
    pub from: Option<BonsaiChangesetId>,

    #[id(2)]
    pub to: BonsaiChangesetId,

    #[id(3)]
    pub reason: MirrorBookmarkUpdateReason,
}

/// Request to mirror a contiguous chain of bookmark moves for one bookmark to a
/// `*_shadow` replica. Used only by modern_sync. The server applies the whole
/// chain as one compare-and-swap from the first move's `from` to the last
/// move's `to`, then writes one bookmarks_update_log row per move, reusing the
/// source ids. If the first move's `from` is `None`, the server creates the
/// bookmark instead of moving it. `moves` must be non-empty, contiguous
/// (`moves[i].from` equals `moves[i-1].to`), and ordered by strictly increasing
/// `log_id`.
#[auto_wire]
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct ReplayIdenticalMovesRequest {
    #[id(0)]
    pub bookmark: String,

    #[id(1)]
    pub moves: Vec<MirrorBookmarkMove>,

    #[id(2)]
    pub pushvars: Vec<PushVar>,
}

#[auto_wire]
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct ReplayIdenticalMovesResponse {
    #[id(0)]
    #[no_default]
    pub data: Result<(), ServerError>,
}

#[auto_wire]
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct BookmarkResult {
    #[id(0)]
    #[no_default]
    pub data: Result<BookmarkEntry, ServerError>,
}

#[auto_wire]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub enum Freshness {
    #[id(1)]
    MostRecent,
    #[id(2)]
    #[default]
    MaybeStale,
}

/// Request to list bookmarks matching patterns (replacement for wireproto listkeyspatterns).
/// Patterns can be exact bookmark names or prefix patterns ending with '*'.
#[auto_wire]
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct ListBookmarkPatternsRequest {
    /// List of patterns to match. Patterns ending with '*' are treated as
    /// prefix matches; all others are treated as exact matches.
    #[id(0)]
    pub patterns: Vec<String>,

    /// Which bookmark kinds to include in results.
    /// Empty means PullDefaultPublishing only (the most common case).
    #[id(1)]
    pub kinds: Vec<BookmarkKind>,
}

/// Response for listing bookmarks matching patterns.
#[auto_wire]
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct ListBookmarkPatternsResponse {
    #[id(0)]
    #[no_default]
    pub data: Result<BookmarkEntry, ServerError>,
}
