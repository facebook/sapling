/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
use serde_derive::{Deserialize, Serialize};

use types::hgid::HgId;

#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct BookmarkRequest {
    pub bookmarks: Vec<String>,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for BookmarkRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        BookmarkRequest {
            bookmarks: Arbitrary::arbitrary(g),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
#[derive(Serialize, Deserialize)]
pub struct BookmarkEntry {
    pub bookmark: String,
    pub hgid: Option<HgId>,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for BookmarkEntry {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        BookmarkEntry {
            bookmark: Arbitrary::arbitrary(g),
            hgid: Arbitrary::arbitrary(g),
        }
    }
}
