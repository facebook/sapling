/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
use serde_derive::{Deserialize, Serialize};

use dag_types::Location;
use types::HgId;

use crate::commit::{
    CommitHashToLocationRequestBatch, CommitHashToLocationResponse, CommitLocationToHashRequest,
    CommitLocationToHashRequestBatch, CommitLocationToHashResponse,
};
use crate::wire::{ToApi, ToWire, WireHgId, WireToApiConversionError};

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireCommitLocation {
    #[serde(rename = "1")]
    pub descendant: WireHgId,
    #[serde(rename = "2")]
    pub distance: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireCommitLocationToHashRequest {
    #[serde(rename = "1")]
    pub location: WireCommitLocation,
    #[serde(rename = "2")]
    pub count: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireCommitLocationToHashResponse {
    #[serde(rename = "1")]
    pub location: WireCommitLocation,
    #[serde(rename = "2")]
    pub count: u64,
    #[serde(rename = "3")]
    pub hgids: Vec<WireHgId>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireCommitLocationToHashRequestBatch {
    #[serde(rename = "1")]
    pub requests: Vec<WireCommitLocationToHashRequest>,
}

impl ToWire for Location<HgId> {
    type Wire = WireCommitLocation;

    fn to_wire(self) -> Self::Wire {
        Self::Wire {
            descendant: self.descendant.to_wire(),
            distance: self.distance,
        }
    }
}

impl ToApi for WireCommitLocation {
    type Api = Location<HgId>;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        let api = Self::Api {
            descendant: self.descendant.to_api()?,
            distance: self.distance,
        };
        Ok(api)
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireCommitLocation {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Location::arbitrary(g).to_wire()
    }
}

impl ToWire for CommitLocationToHashRequest {
    type Wire = WireCommitLocationToHashRequest;

    fn to_wire(self) -> Self::Wire {
        Self::Wire {
            location: self.location.to_wire(),
            count: self.count,
        }
    }
}

impl ToApi for WireCommitLocationToHashRequest {
    type Api = CommitLocationToHashRequest;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        let api = Self::Api {
            location: self.location.to_api()?,
            count: self.count,
        };
        Ok(api)
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireCommitLocationToHashRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        CommitLocationToHashRequest::arbitrary(g).to_wire()
    }
}

impl ToWire for CommitLocationToHashResponse {
    type Wire = WireCommitLocationToHashResponse;

    fn to_wire(self) -> Self::Wire {
        Self::Wire {
            location: self.location.to_wire(),
            count: self.count,
            hgids: self.hgids.to_wire(),
        }
    }
}

impl ToApi for WireCommitLocationToHashResponse {
    type Api = CommitLocationToHashResponse;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        let api = Self::Api {
            location: self.location.to_api()?,
            count: self.count,
            hgids: self.hgids.to_api()?,
        };
        Ok(api)
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireCommitLocationToHashResponse {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        CommitLocationToHashResponse::arbitrary(g).to_wire()
    }
}

impl ToWire for CommitLocationToHashRequestBatch {
    type Wire = WireCommitLocationToHashRequestBatch;

    fn to_wire(self) -> Self::Wire {
        Self::Wire {
            requests: self.requests.to_wire(),
        }
    }
}

impl ToApi for WireCommitLocationToHashRequestBatch {
    type Api = CommitLocationToHashRequestBatch;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        let api = Self::Api {
            requests: self.requests.to_api()?,
        };
        Ok(api)
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireCommitLocationToHashRequestBatch {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        CommitLocationToHashRequestBatch::arbitrary(g).to_wire()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireCommitHashToLocationRequestBatch {
    #[serde(rename = "1")]
    pub client_head: Option<WireHgId>,
    #[serde(rename = "2")]
    pub hgids: Vec<WireHgId>,
    #[serde(rename = "3", default)]
    pub master_heads: Vec<WireHgId>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireCommitHashToLocationResponse {
    #[serde(rename = "1")]
    pub hgid: WireHgId,
    #[serde(rename = "2")]
    pub location: WireCommitLocation,
}

impl ToWire for CommitHashToLocationRequestBatch {
    type Wire = WireCommitHashToLocationRequestBatch;

    fn to_wire(self) -> Self::Wire {
        let client_head = self.master_heads.get(0).copied().to_wire();
        Self::Wire {
            client_head,
            hgids: self.hgids.to_wire(),
            master_heads: self.master_heads.to_wire(),
        }
    }
}

impl ToApi for WireCommitHashToLocationRequestBatch {
    type Api = CommitHashToLocationRequestBatch;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        let mut master_heads = self.master_heads.to_api()?;
        if master_heads.is_empty() {
            let client_head = self.client_head.to_api()?;
            if let Some(head) = client_head {
                master_heads = vec![head];
            }
        }
        let api = Self::Api {
            master_heads,
            hgids: self.hgids.to_api()?,
        };
        Ok(api)
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireCommitHashToLocationRequestBatch {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        CommitHashToLocationRequestBatch::arbitrary(g).to_wire()
    }
}

impl ToWire for CommitHashToLocationResponse {
    type Wire = WireCommitHashToLocationResponse;

    fn to_wire(self) -> Self::Wire {
        Self::Wire {
            hgid: self.hgid.to_wire(),
            location: self.location.to_wire(),
        }
    }
}

impl ToApi for WireCommitHashToLocationResponse {
    type Api = CommitHashToLocationResponse;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        let api = Self::Api {
            hgid: self.hgid.to_api()?,
            location: self.location.to_api()?,
        };
        Ok(api)
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireCommitHashToLocationResponse {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        CommitHashToLocationResponse::arbitrary(g).to_wire()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::wire::tests::{check_serialize_roundtrip, check_wire_roundtrip};

    use quickcheck::quickcheck;

    quickcheck! {
        fn test_roundtrip_serialize_location(v: WireCommitLocation) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_roundtrip_wire_location(v: Location<HgId>) -> bool {
            check_wire_roundtrip(v)
        }

        fn test_roundtrip_serialize_location_to_hash_request(
            v: WireCommitLocationToHashRequest
        ) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_roundtrip_wire_location_to_hash_request(
            v: CommitLocationToHashRequest
        ) -> bool {
            check_wire_roundtrip(v)
        }

        fn test_roundtrip_serialize_location_to_hash_response(
            v: WireCommitLocationToHashResponse
        ) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_roundtrip_wire_location_to_hash_response(
            v: CommitLocationToHashResponse
        ) -> bool {
            check_wire_roundtrip(v)
        }

        fn test_roundtrip_serialize_location_to_hash_request_batch(
            v: WireCommitLocationToHashRequestBatch
        ) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_roundtrip_wire_location_to_hash_request_batch(
            v: CommitLocationToHashRequestBatch
        ) -> bool {
            check_wire_roundtrip(v)
        }

        fn test_roundtrip_serialize_hash_to_location_request_batch(
            v: WireCommitHashToLocationRequestBatch
        ) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_roundtrip_hash_to_location_request_batch(
            v: CommitHashToLocationRequestBatch
        ) -> bool {
            check_wire_roundtrip(v)
        }

        fn test_roundtrip_serialize_hash_to_location_response(
            v: WireCommitHashToLocationResponse
        ) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_roundtrip_wire_hash_to_location_response(
            v: CommitHashToLocationResponse
        ) -> bool {
            check_wire_roundtrip(v)
        }
    }
}
