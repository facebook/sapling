// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt::{self, Debug};

use bytes::Bytes;
use failure_ext::{bail_err, chain::*};
use quickcheck::{single_shrinker, Arbitrary, Gen};
use rust_thrift::compact_protocol;

use crate::blob::{Blob, BlobstoreValue, ContentBlob};
use crate::errors::*;
use crate::thrift;
use crate::typed_hash::{ContentId, ContentIdContext};

/// An enum representing contents for a file. In the future this may have
/// special support for very large files.
#[derive(Clone, Eq, PartialEq)]
pub enum FileContents {
    Bytes(Bytes),
}

impl FileContents {
    pub fn new_bytes<B: Into<Bytes>>(b: B) -> Self {
        FileContents::Bytes(b.into())
    }

    pub(crate) fn from_thrift(fc: thrift::FileContents) -> Result<Self> {
        match fc {
            thrift::FileContents::Bytes(bytes) => Ok(FileContents::Bytes(bytes.into())),
            thrift::FileContents::UnknownField(x) => bail_err!(ErrorKind::InvalidThrift(
                "FileContents".into(),
                format!("unknown file contents field: {}", x)
            )),
        }
    }

    pub fn size(&self) -> usize {
        match *self {
            FileContents::Bytes(ref bytes) => bytes.len(),
        }
    }

    /// Whether this starts with a particular string.
    #[inline]
    pub fn starts_with(&self, needle: &[u8]) -> bool {
        match self {
            FileContents::Bytes(b) => b.starts_with(needle),
        }
    }

    pub fn into_bytes(self) -> Bytes {
        match self {
            FileContents::Bytes(bytes) => bytes,
        }
    }

    pub fn as_bytes(&self) -> &Bytes {
        match self {
            FileContents::Bytes(bytes) => &bytes,
        }
    }

    pub(crate) fn into_thrift(self) -> thrift::FileContents {
        match self {
            // TODO (T26959816) -- allow Thrift to represent binary as Bytes
            FileContents::Bytes(bytes) => thrift::FileContents::Bytes(bytes.to_vec()),
        }
    }

    pub fn from_encoded_bytes(encoded_bytes: Bytes) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(encoded_bytes.as_ref())
            .chain_err(ErrorKind::BlobDeserializeError("FileContents".into()))?;
        Self::from_thrift(thrift_tc)
    }
}

impl BlobstoreValue for FileContents {
    type Key = ContentId;

    fn into_blob(self) -> ContentBlob {
        let mut context = ContentIdContext::new();
        context.update(self.as_bytes());
        let id = context.finish();
        let thrift = self.into_thrift();
        let data = compact_protocol::serialize(&thrift);
        Blob::new(id, data)
    }

    fn from_blob(blob: ContentBlob) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(blob.data().as_ref())
            .chain_err(ErrorKind::BlobDeserializeError("FileContents".into()))?;
        Self::from_thrift(thrift_tc)
    }
}

impl Debug for FileContents {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            FileContents::Bytes(ref bytes) => {
                write!(f, "FileContents::Bytes(length {})", bytes.len())
            }
        }
    }
}

impl Arbitrary for FileContents {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        FileContents::new_bytes(Vec::arbitrary(g))
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        single_shrinker(FileContents::new_bytes(vec![]))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use quickcheck::quickcheck;

    quickcheck! {
        fn thrift_roundtrip(fc: FileContents) -> bool {
            let thrift_fc = fc.clone().into_thrift();
            let fc2 = FileContents::from_thrift(thrift_fc)
                .expect("thrift roundtrips should always be valid");
            fc == fc2
        }

        fn blob_roundtrip(cs: FileContents) -> bool {
            let blob = cs.clone().into_blob();
            let cs2 = FileContents::from_blob(blob)
                .expect("blob roundtrips should always be valid");
            cs == cs2
        }
    }

    #[test]
    fn bad_thrift() {
        let thrift_fc = thrift::FileContents::UnknownField(-1);
        FileContents::from_thrift(thrift_fc).expect_err("unexpected OK - unknown field");
    }
}
