/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroU64;

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
#[cfg(any(test, feature = "for-tests"))]
use serde_derive::{Deserialize, Serialize};

use crate::anyid::{AnyId, BonsaiChangesetId, LookupRequest, LookupResponse};
use crate::wire::{
    is_default, ToApi, ToWire, WireAnyFileContentId, WireHgId, WireToApiConversionError,
    WireUploadToken,
};

wire_hash! {
    wire => WireBonsaiChangesetId,
    api  => BonsaiChangesetId,
    size => 32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireAnyId {
    #[serde(rename = "1")]
    WireAnyFileContentId(WireAnyFileContentId),

    #[serde(rename = "2")]
    WireHgFilenodeId(WireHgId),

    #[serde(rename = "3")]
    WireHgTreeId(WireHgId),

    #[serde(rename = "4")]
    WireHgChangesetId(WireHgId),

    #[serde(rename = "5")]
    WireBonsaiChangesetId(WireBonsaiChangesetId),

    #[serde(other, rename = "0")]
    Unknown,
}

impl Default for WireAnyId {
    fn default() -> Self {
        Self::WireAnyFileContentId(WireAnyFileContentId::default())
    }
}

impl ToWire for AnyId {
    type Wire = WireAnyId;

    fn to_wire(self) -> Self::Wire {
        use AnyId::*;
        match self {
            AnyFileContentId(id) => WireAnyId::WireAnyFileContentId(id.to_wire()),
            HgFilenodeId(id) => WireAnyId::WireHgFilenodeId(id.to_wire()),
            HgTreeId(id) => WireAnyId::WireHgTreeId(id.to_wire()),
            HgChangesetId(id) => WireAnyId::WireHgChangesetId(id.to_wire()),
            BonsaiChangesetId(id) => WireAnyId::WireBonsaiChangesetId(id.to_wire()),
        }
    }
}

impl ToApi for WireAnyId {
    type Api = AnyId;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        use WireAnyId::*;
        Ok(match self {
            Unknown => {
                return Err(WireToApiConversionError::UnrecognizedEnumVariant(
                    "WireAnyId",
                ));
            }
            WireAnyFileContentId(id) => AnyId::AnyFileContentId(id.to_api()?),
            WireHgFilenodeId(id) => AnyId::HgFilenodeId(id.to_api()?),
            WireHgTreeId(id) => AnyId::HgTreeId(id.to_api()?),
            WireHgChangesetId(id) => AnyId::HgChangesetId(id.to_api()?),
            WireBonsaiChangesetId(id) => AnyId::BonsaiChangesetId(id.to_api()?),
        })
    }
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireLookupRequest {
    #[serde(rename = "1", default, skip_serializing_if = "is_default")]
    pub id: WireAnyId,

    #[serde(rename = "2", default, skip_serializing_if = "is_default")]
    pub bubble_id: Option<NonZeroU64>,
}

impl ToWire for LookupRequest {
    type Wire = WireLookupRequest;

    fn to_wire(self) -> Self::Wire {
        WireLookupRequest {
            id: self.id.to_wire(),
            bubble_id: self.bubble_id,
        }
    }
}

impl ToApi for WireLookupRequest {
    type Api = LookupRequest;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(LookupRequest {
            id: self.id.to_api()?,
            bubble_id: self.bubble_id,
        })
    }
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireLookupResponse {
    #[serde(rename = "1")]
    pub index: usize,

    #[serde(rename = "2")]
    pub token: Option<WireUploadToken>,
}

impl ToWire for LookupResponse {
    type Wire = WireLookupResponse;

    fn to_wire(self) -> Self::Wire {
        WireLookupResponse {
            index: self.index,
            token: self.token.to_wire(),
        }
    }
}

impl ToApi for WireLookupResponse {
    type Api = LookupResponse;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(LookupResponse {
            index: self.index,
            token: self.token.to_api()?,
        })
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireAnyId {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        use rand::Rng;
        use WireAnyId::*;

        let variant = g.gen_range(0, 5);
        match variant {
            0 => WireAnyFileContentId(Arbitrary::arbitrary(g)),
            1 => WireHgFilenodeId(Arbitrary::arbitrary(g)),
            2 => WireHgTreeId(Arbitrary::arbitrary(g)),
            3 => WireHgChangesetId(Arbitrary::arbitrary(g)),
            4 => WireBonsaiChangesetId(Arbitrary::arbitrary(g)),
            _ => unreachable!(),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireLookupRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            id: Arbitrary::arbitrary(g),
            bubble_id: Arbitrary::arbitrary(g),
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
        fn test_lookup_roundtrip_serialize(v: WireLookupRequest) -> bool {
            check_serialize_roundtrip(v)
        }

        // API-Wire roundtrips
        fn test_lookup_roundtrip_wire(v: LookupRequest) -> bool {
            check_wire_roundtrip(v)
        }
    }
}
