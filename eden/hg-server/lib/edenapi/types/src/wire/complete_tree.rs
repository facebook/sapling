/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use serde_derive::{Deserialize, Serialize};

use crate::{
    wire::{is_default, ToApi, ToWire, WireHgId, WireRepoPathBuf, WireToApiConversionError},
    CompleteTreeRequest,
};

/// Struct reprenting the arguments to a "gettreepack" operation, which
/// is used by Mercurial to prefetch treemanifests. This struct is intended
/// to provide a way to support requests compatible with Mercurial's existing
/// gettreepack wire protocol command.
///
/// In the future, we'd like to migrate away from requesting trees in this way.
/// In general, trees can be requested from the API server using a `TreeRequest`
/// containing the keys of the desired tree nodes.
///
/// In all cases, trees will be returned in a `TreeResponse`.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireCompleteTreeRequest {
    #[serde(rename = "0", default, skip_serializing_if = "is_default")]
    pub rootdir: WireRepoPathBuf,

    #[serde(rename = "1", default, skip_serializing_if = "is_default")]
    pub mfnodes: Vec<WireHgId>,

    #[serde(rename = "2", default, skip_serializing_if = "is_default")]
    pub basemfnodes: Vec<WireHgId>,

    #[serde(rename = "3", default, skip_serializing_if = "is_default")]
    pub depth: Option<usize>,
}

impl ToWire for CompleteTreeRequest {
    type Wire = WireCompleteTreeRequest;

    fn to_wire(self) -> Self::Wire {
        WireCompleteTreeRequest {
            rootdir: self.rootdir.to_wire(),
            mfnodes: self.mfnodes.to_wire(),
            basemfnodes: self.basemfnodes.to_wire(),
            depth: self.depth,
        }
    }
}

impl ToApi for WireCompleteTreeRequest {
    type Api = CompleteTreeRequest;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(CompleteTreeRequest {
            rootdir: self.rootdir.to_api()?,
            mfnodes: self.mfnodes.to_api()?,
            basemfnodes: self.basemfnodes.to_api()?,
            depth: self.depth,
        })
    }
}

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireCompleteTreeRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            rootdir: Arbitrary::arbitrary(g),
            mfnodes: Arbitrary::arbitrary(g),
            basemfnodes: Arbitrary::arbitrary(g),
            depth: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::wire::tests::{check_serialize_roundtrip, check_wire_roundtrip};

    use quickcheck::quickcheck;

    quickcheck! {
        fn test_request_roundtrip_serialize(v: WireCompleteTreeRequest) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_request_roundtrip_wire(v: CompleteTreeRequest) -> bool {
            check_wire_roundtrip(v)
        }
    }
}
