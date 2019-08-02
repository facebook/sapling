// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::convert::TryInto;
use std::fmt::{self, Debug};

use bytes::Bytes;
use failure_ext::{bail_err, chain::*};
use quickcheck::{single_shrinker, Arbitrary, Gen};
use rust_thrift::compact_protocol;

use crate::{
    blob::{Blob, BlobstoreValue, ContentChunkBlob},
    errors::*,
    file_contents::ContentChunkPointer,
    thrift,
    typed_hash::{ContentChunkId, ContentChunkIdContext},
};

/// Chunk of a file's contents.
#[derive(Clone, Eq, PartialEq)]
pub struct ContentChunk(Bytes);

impl ContentChunk {
    pub fn new_bytes<B: Into<Bytes>>(b: B) -> Self {
        ContentChunk(b.into())
    }

    pub(crate) fn from_thrift(fc: thrift::ContentChunk) -> Result<Self> {
        match fc {
            thrift::ContentChunk::Bytes(bytes) => Ok(ContentChunk(bytes.into())),
            thrift::ContentChunk::UnknownField(x) => bail_err!(ErrorKind::InvalidThrift(
                "ContentChunk".into(),
                format!("unknown ContentChunk variant: {}", x)
            )),
        }
    }

    pub(crate) fn into_thrift(self) -> thrift::ContentChunk {
        thrift::ContentChunk::Bytes(self.0.to_vec())
    }

    pub fn from_encoded_bytes(encoded_bytes: Bytes) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(encoded_bytes.as_ref())
            .chain_err(ErrorKind::BlobDeserializeError("ContentChunk".into()))?;
        Self::from_thrift(thrift_tc)
    }

    pub fn size(&self) -> u64 {
        // NOTE: This panics if the buffer length doesn't fit a u64: that's fine.
        self.0.len().try_into().unwrap()
    }

    pub fn into_bytes(self) -> Bytes {
        self.0
    }
}

impl BlobstoreValue for ContentChunk {
    type Key = ContentChunkId;

    fn into_blob(self) -> ContentChunkBlob {
        let id = {
            let mut context = ContentChunkIdContext::new();
            context.update(&self.0);
            context.finish()
        };

        let data = compact_protocol::serialize(&self.into_thrift());

        Blob::new(id, data)
    }

    fn from_blob(blob: ContentChunkBlob) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(blob.data().as_ref())
            .chain_err(ErrorKind::BlobDeserializeError("ContentChunk".into()))?;
        Self::from_thrift(thrift_tc)
    }
}

impl Debug for ContentChunk {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ContentChunk(length {})", self.0.len())
    }
}

impl Arbitrary for ContentChunk {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        ContentChunk::new_bytes(Vec::arbitrary(g))
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        single_shrinker(ContentChunk::new_bytes(vec![]))
    }
}

pub fn new_blob_and_pointer<B: Into<Bytes>>(bytes: B) -> (ContentChunkBlob, ContentChunkPointer) {
    let chunk = ContentChunk::new_bytes(bytes);
    let size = chunk.size();

    let blob = chunk.into_blob();
    let id = blob.id();

    let pointer = ContentChunkPointer::new(*id, size);

    (blob, pointer)
}

#[cfg(test)]
mod test {
    use super::*;
    use quickcheck::quickcheck;

    quickcheck! {
        fn file_contents_thrift_roundtrip(fc: ContentChunk) -> bool {
            let thrift_fc = fc.clone().into_thrift();
            let fc2 = ContentChunk::from_thrift(thrift_fc)
                .expect("thrift roundtrips should always be valid");
            fc == fc2
        }

        fn file_contents_blob_roundtrip(fc: ContentChunk) -> bool {
            let blob = fc.clone().into_blob();
            let fc2 = ContentChunk::from_blob(blob)
                .expect("blob roundtrips should always be valid");
            fc == fc2
        }

        fn test_blob_and_pointer_consistency(bytes: Vec<u8>) -> bool {
            let (blob, pointer) = new_blob_and_pointer(bytes);
            let blob_id = *blob.id();
            let chunk = ContentChunk::from_blob(blob)
                .expect("blob roundtrips should always be valid");
             blob_id == pointer.chunk_id() && chunk.size() == pointer.size()
        }
    }

    #[test]
    fn bad_thrift() {
        let thrift_fc = thrift::ContentChunk::UnknownField(-1);
        ContentChunk::from_thrift(thrift_fc).expect_err("unexpected OK - unknown field");
    }
}
