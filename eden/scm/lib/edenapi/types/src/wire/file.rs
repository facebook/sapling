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
    file::{FileEntry, FileRequest},
    wire::{
        is_default, ToApi, ToWire, WireKey, WireParents, WireRevisionstoreMetadata,
        WireToApiConversionError,
    },
};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WireFileEntry {
    #[serde(rename = "0", default, skip_serializing_if = "is_default")]
    key: WireKey,

    #[serde(rename = "1", default, skip_serializing_if = "is_default")]
    data: Bytes,

    #[serde(rename = "2", default, skip_serializing_if = "is_default")]
    parents: WireParents,

    #[serde(rename = "3", default, skip_serializing_if = "is_default")]
    metadata: WireRevisionstoreMetadata,
}

impl ToWire for FileEntry {
    type Wire = WireFileEntry;

    fn to_wire(self) -> Self::Wire {
        WireFileEntry {
            key: self.key.to_wire(),
            data: self.data,
            parents: self.parents.to_wire(),
            metadata: self.metadata.to_wire(),
        }
    }
}

impl ToApi for WireFileEntry {
    type Api = FileEntry;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(FileEntry {
            key: self.key.to_api()?,
            data: self.data,
            parents: self.parents.to_api()?,
            metadata: self.metadata.to_api()?,
        })
    }
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireFileRequest {
    #[serde(rename = "0", default, skip_serializing_if = "is_default")]
    pub keys: Vec<WireKey>,
}

impl ToWire for FileRequest {
    type Wire = WireFileRequest;

    fn to_wire(self) -> Self::Wire {
        WireFileRequest {
            keys: self.keys.to_wire(),
        }
    }
}

impl ToApi for WireFileRequest {
    type Api = FileRequest;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(FileRequest {
            keys: self.keys.to_api()?,
        })
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireFileEntry {
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
impl Arbitrary for WireFileRequest {
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
        fn test_request_roundtrip_serialize(v: WireFileRequest) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_request_roundtrip_wire(v: FileRequest) -> bool {
            check_wire_roundtrip(v)
        }

        fn test_entry_roundtrip_serialize(v: WireFileEntry) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_entry_roundtrip_wire(v: FileEntry) -> bool {
            check_wire_roundtrip(v)
        }
    }
}
