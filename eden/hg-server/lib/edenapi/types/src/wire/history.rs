/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
use serde_derive::{Deserialize, Serialize};

use crate::{
    wire::{
        ToApi, ToWire, WireHgId, WireKey, WireParents, WireRepoPathBuf, WireToApiConversionError,
    },
    HistoryRequest, HistoryResponseChunk, WireHistoryEntry,
};

#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireHistoryRequest {
    keys: Vec<WireKey>,
    length: Option<u32>,
}

impl ToWire for HistoryRequest {
    type Wire = WireHistoryRequest;

    fn to_wire(self) -> Self::Wire {
        WireHistoryRequest {
            keys: self.keys.to_wire(),
            length: self.length.to_wire(),
        }
    }
}

impl ToApi for WireHistoryRequest {
    type Api = HistoryRequest;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(HistoryRequest {
            keys: self.keys.to_api()?,
            length: self.length.to_api()?,
        })
    }
}

#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
// TODO: Rename, move more functionality to wire types?
pub struct WireWireHistoryEntry {
    node: Option<WireHgId>,
    parents: Option<WireParents>,
    linknode: Option<WireHgId>,
    copyfrom: Option<WireRepoPathBuf>,
}

impl ToWire for WireHistoryEntry {
    type Wire = WireWireHistoryEntry;

    fn to_wire(self) -> Self::Wire {
        WireWireHistoryEntry {
            node: Some(self.node.to_wire()),
            parents: Some(self.parents.to_wire()),
            linknode: Some(self.linknode.to_wire()),
            copyfrom: self.copyfrom.to_wire(),
        }
    }
}

impl ToApi for WireWireHistoryEntry {
    type Api = WireHistoryEntry;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(WireHistoryEntry {
            node: self.node.to_api()?.ok_or(
                WireToApiConversionError::CannotPopulateRequiredField("node"),
            )?,
            parents: self.parents.to_api()?.ok_or(
                WireToApiConversionError::CannotPopulateRequiredField("parents"),
            )?,
            linknode: self.linknode.to_api()?.ok_or(
                WireToApiConversionError::CannotPopulateRequiredField("linknode"),
            )?,
            copyfrom: self.copyfrom.to_api()?,
        })
    }
}

#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireHistoryResponseChunk {
    path: Option<WireRepoPathBuf>,
    entries: Vec<WireWireHistoryEntry>,
}

impl ToWire for HistoryResponseChunk {
    type Wire = WireHistoryResponseChunk;

    fn to_wire(self) -> Self::Wire {
        WireHistoryResponseChunk {
            path: Some(self.path.to_wire()),
            entries: self.entries.to_wire(),
        }
    }
}

impl ToApi for WireHistoryResponseChunk {
    type Api = HistoryResponseChunk;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(HistoryResponseChunk {
            path: self.path.to_api()?.ok_or(
                WireToApiConversionError::CannotPopulateRequiredField("path"),
            )?,
            entries: self.entries.to_api()?,
        })
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireHistoryRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            keys: Arbitrary::arbitrary(g),
            length: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireWireHistoryEntry {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            node: Arbitrary::arbitrary(g),
            parents: Arbitrary::arbitrary(g),
            linknode: Arbitrary::arbitrary(g),
            copyfrom: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireHistoryResponseChunk {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            path: Arbitrary::arbitrary(g),
            entries: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::wire::tests::{check_serialize_roundtrip, check_wire_roundtrip};

    use quickcheck::quickcheck;

    quickcheck! {
        // Wire serialize roundtrips
        fn test_wire_history_request_roundtrip_serialize(v: WireHistoryRequest) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_wire_wire_history_entry_serialize(v: WireWireHistoryEntry) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_wire_history_response_chunk_roundtrip_serialize(v: WireHistoryResponseChunk) -> bool {
            check_serialize_roundtrip(v)
        }

        // API-Wire roundtrips

        fn test_history_request_roundtrip_wire(v: HistoryRequest) -> bool {
            check_wire_roundtrip(v)
        }

        fn test_wire_history_entry_roundtrip_wire(v: WireHistoryEntry) -> bool {
            check_wire_roundtrip(v)
        }

        fn test_history_response_chunk_roundtrip_wire(v: HistoryResponseChunk) -> bool {
            check_wire_roundtrip(v)
        }
    }
}
