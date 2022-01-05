/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
use serde_derive::Deserialize;
use serde_derive::Serialize;

use crate::wire::ToApi;
use crate::wire::ToWire;
use crate::wire::WireHgId;
use crate::wire::WireKey;
use crate::wire::WireParents;
use crate::wire::WireRepoPathBuf;
use crate::wire::WireToApiConversionError;
use crate::HistoryRequest;
use crate::HistoryResponseChunk;
use crate::WireHistoryEntry;

// TODO: attributes in this file aren't renamed to 0, 1, ...
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
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        Self {
            keys: Arbitrary::arbitrary(g),
            length: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireWireHistoryEntry {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
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
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        Self {
            path: Arbitrary::arbitrary(g),
            entries: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::tests::auto_wire_tests;

    auto_wire_tests!(
        WireHistoryRequest,
        WireWireHistoryEntry,
        WireHistoryResponseChunk,
    );
}
