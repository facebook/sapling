/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
use serde_derive::{Deserialize, Serialize};

use types::HgId;

use crate::wire::{ToApi, ToWire, WireDagId, WireHgId, WireToApiConversionError};
use crate::{CloneData, FlatSegment, PreparedFlatSegments};

// Only when an id has more than one parent it is sent over the wire.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireCloneData {
    #[serde(rename = "1")]
    pub head_id: WireDagId,
    #[serde(rename = "2")]
    pub flat_segments: Vec<WireFlatSegment>,
    #[serde(rename = "3")]
    pub idmap: Vec<WireIdMapEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireFlatSegment {
    #[serde(rename = "1")]
    pub low: WireDagId,
    #[serde(rename = "2")]
    pub high: WireDagId,
    #[serde(rename = "3")]
    pub parents: Vec<WireDagId>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireIdMapEntry {
    #[serde(rename = "1")]
    pub dag_id: WireDagId,
    #[serde(rename = "2")]
    pub hg_id: WireHgId,
}

impl ToWire for CloneData<HgId> {
    type Wire = WireCloneData;

    fn to_wire(self) -> Self::Wire {
        let idmap = self
            .idmap
            .into_iter()
            .map(|(k, v)| WireIdMapEntry {
                dag_id: k.to_wire(),
                hg_id: v.to_wire(),
            })
            .collect();
        WireCloneData {
            head_id: self.head_id.to_wire(),
            flat_segments: self.flat_segments.segments.to_wire(),
            idmap,
        }
    }
}

impl ToApi for WireCloneData {
    type Api = CloneData<HgId>;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        let mut idmap = HashMap::new();
        for wie in self.idmap {
            idmap.insert(wie.dag_id.to_api()?, wie.hg_id.to_api()?);
        }
        let flat_segments = PreparedFlatSegments {
            segments: self.flat_segments.to_api()?,
        };
        Ok(CloneData {
            head_id: self.head_id.to_api()?,
            flat_segments,
            idmap,
        })
    }
}

impl ToWire for FlatSegment {
    type Wire = WireFlatSegment;

    fn to_wire(self) -> Self::Wire {
        WireFlatSegment {
            low: self.low.to_wire(),
            high: self.high.to_wire(),
            parents: self.parents.to_wire(),
        }
    }
}

impl ToApi for WireFlatSegment {
    type Api = FlatSegment;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(FlatSegment {
            low: self.low.to_api()?,
            high: self.high.to_api()?,
            parents: self.parents.to_api()?,
        })
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireFlatSegment {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        FlatSegment::arbitrary(g).to_wire()
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireCloneData {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        CloneData::arbitrary(g).to_wire()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::wire::tests::{check_serialize_roundtrip, check_wire_roundtrip};

    use quickcheck::quickcheck;

    quickcheck! {
        fn test_request_roundtrip_serialize(v: WireCloneData) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_request_roundtrip_wire(v: CloneData<HgId>) -> bool {
            check_wire_roundtrip(v)
        }
    }
}
