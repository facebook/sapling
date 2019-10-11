/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::fmt::{self, Debug};

use bytes::Bytes;
use failure_ext::{bail_err, chain::*};
use fbthrift::compact_protocol;
use quickcheck::{single_shrinker, Arbitrary, Gen};

use crate::blob::{Blob, BlobstoreValue, RawBundle2Blob};
use crate::errors::*;
use crate::thrift;
use crate::typed_hash::{RawBundle2Id, RawBundle2IdContext};

/// An enum representing contents of a raw bundle2 blob
#[derive(Clone, Eq, PartialEq)]
pub enum RawBundle2 {
    Bytes(Bytes),
}

impl RawBundle2 {
    pub fn new_bytes<B: Into<Bytes>>(b: B) -> Self {
        RawBundle2::Bytes(b.into())
    }

    pub(crate) fn from_thrift(fc: thrift::RawBundle2) -> Result<Self> {
        match fc {
            thrift::RawBundle2::Bytes(bytes) => Ok(RawBundle2::Bytes(bytes.into())),
            thrift::RawBundle2::UnknownField(x) => bail_err!(ErrorKind::InvalidThrift(
                "RawBundle2".into(),
                format!("unknown rawbundle2 field: {}", x)
            )),
        }
    }

    pub fn size(&self) -> usize {
        match *self {
            RawBundle2::Bytes(ref bytes) => bytes.len(),
        }
    }

    /// Whether this starts with a particular string.
    #[inline]
    pub fn starts_with(&self, needle: &[u8]) -> bool {
        match self {
            RawBundle2::Bytes(b) => b.starts_with(needle),
        }
    }

    pub fn into_bytes(self) -> Bytes {
        match self {
            RawBundle2::Bytes(bytes) => bytes,
        }
    }

    pub fn as_bytes(&self) -> &Bytes {
        match self {
            RawBundle2::Bytes(bytes) => &bytes,
        }
    }

    pub(crate) fn into_thrift(self) -> thrift::RawBundle2 {
        match self {
            // TODO (T26959816) -- allow Thrift to represent binary as Bytes
            RawBundle2::Bytes(bytes) => thrift::RawBundle2::Bytes(bytes.to_vec()),
        }
    }

    pub fn from_encoded_bytes(encoded_bytes: Bytes) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(encoded_bytes.as_ref())
            .chain_err(ErrorKind::BlobDeserializeError("RawBundle2".into()))?;
        Self::from_thrift(thrift_tc)
    }
}

impl BlobstoreValue for RawBundle2 {
    type Key = RawBundle2Id;

    fn into_blob(self) -> RawBundle2Blob {
        let mut context = RawBundle2IdContext::new();
        context.update(self.as_bytes());
        let id = context.finish();
        let thrift = self.into_thrift();
        let data = compact_protocol::serialize(&thrift);
        Blob::new(id, data)
    }

    fn from_blob(blob: RawBundle2Blob) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(blob.data().as_ref())
            .chain_err(ErrorKind::BlobDeserializeError("RawBundle2".into()))?;
        Self::from_thrift(thrift_tc)
    }
}

impl Debug for RawBundle2 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            RawBundle2::Bytes(ref bytes) => write!(f, "RawBundle2::Bytes(length {})", bytes.len()),
        }
    }
}

impl Arbitrary for RawBundle2 {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        RawBundle2::new_bytes(Vec::arbitrary(g))
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        single_shrinker(RawBundle2::new_bytes(vec![]))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use quickcheck::quickcheck;

    quickcheck! {
        fn thrift_roundtrip(fc: RawBundle2) -> bool {
            let thrift_fc = fc.clone().into_thrift();
            let fc2 = RawBundle2::from_thrift(thrift_fc)
                .expect("thrift roundtrips should always be valid");
            fc == fc2
        }

        fn blob_roundtrip(cs: RawBundle2) -> bool {
            let blob = cs.clone().into_blob();
            let cs2 = RawBundle2::from_blob(blob)
                .expect("blob roundtrips should always be valid");
            cs == cs2
        }
    }

    #[test]
    fn bad_thrift() {
        let thrift_fc = thrift::RawBundle2::UnknownField(-1);
        RawBundle2::from_thrift(thrift_fc).expect_err("unexpected OK - unknown field");
    }
}
