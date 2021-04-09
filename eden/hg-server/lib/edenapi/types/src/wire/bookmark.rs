/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
use serde_derive::{Deserialize, Serialize};

use crate::bookmark::{BookmarkEntry, BookmarkRequest};

use crate::wire::{is_default, ToApi, ToWire, WireHgId, WireToApiConversionError};

#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireBookmarkRequest {
    #[serde(rename = "0", default, skip_serializing_if = "is_default")]
    pub bookmarks: Vec<String>,
}

impl ToWire for BookmarkRequest {
    type Wire = WireBookmarkRequest;

    fn to_wire(self) -> Self::Wire {
        WireBookmarkRequest {
            bookmarks: self.bookmarks,
        }
    }
}

impl ToApi for WireBookmarkRequest {
    type Api = BookmarkRequest;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(BookmarkRequest {
            bookmarks: self.bookmarks,
        })
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireBookmarkRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        BookmarkRequest::arbitrary(g).to_wire()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireBookmarkEntry {
    #[serde(rename = "1")]
    pub bookmark: String,
    #[serde(rename = "2")]
    pub hgid: Option<WireHgId>,
}

impl ToWire for BookmarkEntry {
    type Wire = WireBookmarkEntry;

    fn to_wire(self) -> Self::Wire {
        Self::Wire {
            bookmark: self.bookmark,
            hgid: self.hgid.to_wire(),
        }
    }
}

impl ToApi for WireBookmarkEntry {
    type Api = BookmarkEntry;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        let api = Self::Api {
            bookmark: self.bookmark,
            hgid: self.hgid.to_api()?,
        };
        Ok(api)
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireBookmarkEntry {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        BookmarkEntry::arbitrary(g).to_wire()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::wire::tests::{check_serialize_roundtrip, check_wire_roundtrip};

    use quickcheck::quickcheck;

    quickcheck! {

        fn test_roundtrip_serialize_bookmark_request(v: WireBookmarkRequest) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_roundtrip_wire_bookmark_request(v: BookmarkRequest) -> bool {
            check_wire_roundtrip(v)
        }


        fn test_roundtrip_serialize_bookmark_response(v: WireBookmarkEntry) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_roundtrip_wire_bookmark_response(v: BookmarkEntry) -> bool {
            check_wire_roundtrip(v)
        }
    }
}
