/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
use serde_derive::{Deserialize, Serialize};

use crate::{
    tree::{TreeEntry, TreeRequest},
    wire::{
        is_default, ToApi, ToWire, WireKey, WireParents, WireRevisionstoreMetadata,
        WireToApiConversionError,
    },
};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WireTreeEntry {
    #[serde(rename = "0", default, skip_serializing_if = "is_default")]
    key: WireKey,

    #[serde(rename = "1", default, skip_serializing_if = "is_default")]
    data: Bytes,

    #[serde(rename = "2", default, skip_serializing_if = "is_default")]
    parents: WireParents,

    #[serde(rename = "3", default, skip_serializing_if = "is_default")]
    metadata: WireRevisionstoreMetadata,
}

impl ToWire for TreeEntry {
    type Wire = WireTreeEntry;

    fn to_wire(self) -> Self::Wire {
        WireTreeEntry {
            key: self.key.to_wire(),
            data: self.data,
            parents: self.parents.to_wire(),
            metadata: self.metadata.to_wire(),
        }
    }
}

impl ToApi for WireTreeEntry {
    type Api = TreeEntry;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(TreeEntry {
            key: self.key.to_api()?,
            data: self.data,
            parents: self.parents.to_api()?,
            metadata: self.metadata.to_api()?,
        })
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireTreeRequest {
    #[serde(rename = "0", default, skip_serializing_if = "is_default")]
    pub keys: Vec<WireKey>,
}

impl ToWire for TreeRequest {
    type Wire = WireTreeRequest;

    fn to_wire(self) -> Self::Wire {
        WireTreeRequest {
            keys: self.keys.to_wire(),
        }
    }
}

impl ToApi for WireTreeRequest {
    type Api = TreeRequest;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(TreeRequest {
            keys: self.keys.to_api()?,
        })
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireTreeEntry {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        let bytes: Vec<u8> = Arbitrary::arbitrary(g);
        Self {
            key: Arbitrary::arbitrary(g),
            data: Bytes::from(bytes),
            parents: Arbitrary::arbitrary(g),
            metadata: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireTreeRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            keys: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::wire::tests::{check_serialize_roundtrip, check_wire_roundtrip};

    use quickcheck::quickcheck;

    quickcheck! {
        fn test_request_roundtrip_serialize(v: WireTreeRequest) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_request_roundtrip_wire(v: TreeRequest) -> bool {
            check_wire_roundtrip(v)
        }

        fn test_entry_roundtrip_serialize(v: WireTreeEntry) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_entry_roundtrip_wire(v: TreeEntry) -> bool {
            check_wire_roundtrip(v)
        }
    }
}
