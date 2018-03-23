// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt::{self, Debug};

use bytes::Bytes;
use failure::SyncFailure;
use quickcheck::{single_shrinker, Arbitrary, Gen};

use rust_thrift::compact_protocol;

use blob::{Blob, ContentBlob};
use errors::*;
use thrift;
use typed_hash::ContentIdContext;

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

    pub fn from_blob<T: AsRef<[u8]>>(t: T) -> Result<Self> {
        // TODO (T27336549) stop using SyncFailure once thrift is converted to failure
        let thrift_tc = compact_protocol::deserialize(t.as_ref())
            .map_err(SyncFailure::new)
            .context(ErrorKind::BlobDeserializeError("FileContents".into()))?;
        Self::from_thrift(thrift_tc)
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

    pub fn into_bytes(self) -> Bytes {
        match self {
            FileContents::Bytes(bytes) => bytes,
        }
    }

    /// Serialize this structure into a blob.
    pub fn into_blob(self) -> ContentBlob {
        let thrift = self.into_thrift();
        let data = compact_protocol::serialize(&thrift);
        let mut context = ContentIdContext::new();
        context.update(&data);
        let id = context.finish();
        Blob::new(id, data)
    }

    pub(crate) fn into_thrift(self) -> thrift::FileContents {
        match self {
            // TODO (T26959816) -- allow Thrift to represent binary as Bytes
            FileContents::Bytes(bytes) => thrift::FileContents::Bytes(bytes.to_vec()),
        }
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

    fn shrink(&self) -> Box<Iterator<Item = Self>> {
        single_shrinker(FileContents::new_bytes(vec![]))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    quickcheck! {
        fn thrift_roundtrip(fc: FileContents) -> bool {
            let thrift_fc = fc.clone().into_thrift();
            let fc2 = FileContents::from_thrift(thrift_fc)
                .expect("thrift roundtrips should always be valid");
            fc == fc2
        }

        fn blob_roundtrip(cs: FileContents) -> bool {
            let blob = cs.clone().into_blob();
            let cs2 = FileContents::from_blob(blob.data().as_ref())
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
