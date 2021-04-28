/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::wire::{ToApi, ToWire, WireToApiConversionError};
use crate::Batch;

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireBatch<T> {
    #[serde(rename = "1")]
    pub batch: Vec<T>,
}

impl<T> ToWire for Batch<T>
where
    T: ToWire,
{
    type Wire = WireBatch<T::Wire>;

    fn to_wire(self) -> Self::Wire {
        Self::Wire {
            batch: self.batch.to_wire(),
        }
    }
}

impl<T> ToApi for WireBatch<T>
where
    T: ToApi,
{
    type Api = Batch<T::Api>;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        let batch = self.batch.to_api().map_err(|e| e.into())?;
        Ok(Batch { batch })
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl<T> quickcheck::Arbitrary for WireBatch<T>
where
    T: quickcheck::Arbitrary,
{
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        WireBatch {
            batch: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use quickcheck::quickcheck;

    use types::HgId;

    use crate::wire::tests::{check_serialize_roundtrip, check_wire_roundtrip};
    use crate::wire::WireHgId;

    quickcheck! {
        fn test_serialize_roundtrip_batch_request(v: WireBatch<WireHgId>) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_wire_roundtrip_batch_request(v: Batch<HgId>) -> bool {
            check_wire_roundtrip(v)
        }
    }
}
