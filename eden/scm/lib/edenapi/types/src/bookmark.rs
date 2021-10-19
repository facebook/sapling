/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use type_macros::auto_wire;

use types::hgid::HgId;

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct BookmarkRequest {
    #[id(0)]
    pub bookmarks: Vec<String>,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for BookmarkRequest {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        BookmarkRequest {
            bookmarks: Arbitrary::arbitrary(g),
        }
    }
}

#[auto_wire]
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
#[derive(Serialize, Deserialize)]
pub struct BookmarkEntry {
    #[id(1)]
    pub bookmark: String,
    #[id(2)]
    pub hgid: Option<HgId>,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for BookmarkEntry {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        BookmarkEntry {
            bookmark: Arbitrary::arbitrary(g),
            hgid: Arbitrary::arbitrary(g),
        }
    }
}
