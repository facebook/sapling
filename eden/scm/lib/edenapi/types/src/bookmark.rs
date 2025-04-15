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
use crate::land::PushVar;

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

#[auto_wire]
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct BookmarkResult {
    #[id(0)]
    #[no_default]
    pub data: Result<BookmarkEntry, ServerError>,
}

#[auto_wire]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub enum Freshness {
    #[id(1)]
    MostRecent,
    #[id(2)]
    MaybeStale,
}

impl Default for Freshness {
    fn default() -> Self {
        Self::MaybeStale
    }
}
