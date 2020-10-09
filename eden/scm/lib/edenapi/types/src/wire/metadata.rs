/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::Infallible;

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
use serde_derive::{Deserialize, Serialize};

use crate::{
    wire::is_default, DirectoryMetadata, DirectoryMetadataRequest, FileMetadata,
    FileMetadataRequest, ToApi, ToWire,
};

/// Directory entry metadata
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireDirectoryMetadata {}

impl ToWire for DirectoryMetadata {
    type Wire = WireDirectoryMetadata;

    fn to_wire(self) -> Self::Wire {
        WireDirectoryMetadata {}
    }
}

impl ToApi for WireDirectoryMetadata {
    type Api = DirectoryMetadata;
    type Error = Infallible;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(DirectoryMetadata {})
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireDirectoryMetadataRequest {}

impl ToWire for DirectoryMetadataRequest {
    type Wire = WireDirectoryMetadataRequest;

    fn to_wire(self) -> Self::Wire {
        WireDirectoryMetadataRequest {}
    }
}

impl ToApi for WireDirectoryMetadataRequest {
    type Api = DirectoryMetadataRequest;
    type Error = Infallible;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(DirectoryMetadataRequest {})
    }
}

/// File entry metadata
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireFileMetadata {
    #[serde(rename = "0", default, skip_serializing_if = "is_default")]
    revisionstore_flags: Option<u64>,
}

impl ToWire for FileMetadata {
    type Wire = WireFileMetadata;

    fn to_wire(self) -> Self::Wire {
        WireFileMetadata {
            revisionstore_flags: self.revisionstore_flags,
        }
    }
}

impl ToApi for WireFileMetadata {
    type Api = FileMetadata;
    type Error = Infallible;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(FileMetadata {
            revisionstore_flags: self.revisionstore_flags,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireFileMetadataRequest {
    #[serde(rename = "0", default, skip_serializing_if = "is_default")]
    with_revisionstore_flags: bool,
}

impl ToWire for FileMetadataRequest {
    type Wire = WireFileMetadataRequest;

    fn to_wire(self) -> Self::Wire {
        WireFileMetadataRequest {
            with_revisionstore_flags: self.with_revisionstore_flags,
        }
    }
}

impl ToApi for WireFileMetadataRequest {
    type Api = FileMetadataRequest;
    type Error = Infallible;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(FileMetadataRequest {
            with_revisionstore_flags: self.with_revisionstore_flags,
        })
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireDirectoryMetadata {
    fn arbitrary<G: quickcheck::Gen>(_g: &mut G) -> Self {
        Self {}
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireDirectoryMetadataRequest {
    fn arbitrary<G: quickcheck::Gen>(_g: &mut G) -> Self {
        Self {}
    }
}
#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireFileMetadata {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            revisionstore_flags: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireFileMetadataRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            with_revisionstore_flags: Arbitrary::arbitrary(g),
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
        fn test_file_meta_roundtrip_serialize(v: WireFileMetadata) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_file_meta_req_roundtrip_serialize(v: WireFileMetadataRequest) -> bool {
            check_serialize_roundtrip(v)
        }

        // API-Wire roundtrips
        fn test_file_meta_roundtrip_wire(v: FileMetadata) -> bool {
            check_wire_roundtrip(v)
        }

        fn test_file_meta_req_roundtrip_wire(v: FileMetadataRequest) -> bool {
            check_wire_roundtrip(v)
        }
    }
}
