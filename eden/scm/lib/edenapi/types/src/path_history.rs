/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use quickcheck_arbitrary_derive::Arbitrary;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use type_macros::auto_wire;
use types::HgId;
use types::RepoPathBuf;

use crate::ServerError;

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct PathHistoryRequestPaginationCursor {
    #[id(0)]
    pub path: RepoPathBuf,
    // From which commits we start returning history for this path
    // Empty means we start from the beginning/latest
    // The optional path in the tuple is to capture potential renames
    #[id(1)]
    pub starting_commits: Vec<(HgId, Option<RepoPathBuf>)>,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct PathHistoryRequest {
    // The (commit, paths) pair to fetch history for
    #[id(0)]
    pub commit: HgId,
    #[id(1)]
    pub paths: Vec<RepoPathBuf>,

    // The maximum number of history entries to return for each path
    #[id(2)]
    pub limit: Option<u32>,
    // Where to start returning history for each path
    #[id(3)]
    pub cursor: Vec<PathHistoryRequestPaginationCursor>,
}

#[auto_wire]
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct PathHistoryResponse {
    #[id(0)]
    pub path: RepoPathBuf,
    #[id(1)]
    #[no_default]
    pub entries: Result<PathHistoryEntries, ServerError>,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct PathHistoryEntries {
    #[id(0)]
    pub entries: Vec<PathHistoryEntry>,
    // Whether there are more entries to fetch
    #[id(1)]
    pub has_more: bool,
    // If there are more to fetch, where should the next request start
    // Empty when has_more is false
    #[id(2)]
    pub next_commits: Vec<(HgId, Option<RepoPathBuf>)>,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct PathHistoryEntry {
    #[id(0)]
    pub commit: HgId,
}
