/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(any(test, feature = "for-tests"))]
use quickcheck_arbitrary_derive::Arbitrary;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use type_macros::auto_wire;
use types::hgid::HgId;

use crate::land::PushVar;

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct BookmarkRequest {
    #[id(0)]
    pub bookmarks: Vec<String>,
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
