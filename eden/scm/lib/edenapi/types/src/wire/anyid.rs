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

use crate::anyid::AnyId;
use crate::anyid::BonsaiChangesetId;
pub use crate::anyid::WireLookupRequest;
pub use crate::anyid::WireLookupResponse;
pub use crate::anyid::WireLookupResult;
use crate::wire::ToApi;
use crate::wire::ToWire;
use crate::wire::WireAnyFileContentId;
use crate::wire::WireHgId;
use crate::wire::WireToApiConversionError;

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

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireAnyId {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        use WireAnyId::*;

        let variant = g.choose(&[0, 1, 2, 3, 4]).unwrap();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::tests::auto_wire_tests;

    auto_wire_tests!(
        WireAnyId,
        WireLookupRequest,
        WireLookupResponse,
        WireLookupResult,
    );
}
