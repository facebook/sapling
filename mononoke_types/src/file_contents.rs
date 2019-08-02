// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::convert::TryInto;
use std::fmt::{self, Debug};

use bytes::Bytes;
use failure_ext::{bail_err, chain::*};
use quickcheck::{empty_shrinker, single_shrinker, Arbitrary, Gen};
use rust_thrift::compact_protocol;

use crate::{
    blob::{Blob, BlobstoreBytes, BlobstoreValue, ContentBlob, ContentMetadataBlob},
    errors::*,
    hash, thrift, thrift_field,
    typed_hash::{ContentChunkId, ContentId, ContentIdContext, ContentMetadataId},
};

/// An enum representing contents for a file.
#[derive(Clone, Eq, PartialEq)]
pub enum FileContents {
    /// Raw contents of the file
    Bytes(Bytes),
    /// Reference to separate ContentChunks that need to be joined together to produce this file
    Chunked(ChunkedFileContents),
}

impl FileContents {
    pub fn new_bytes<B: Into<Bytes>>(b: B) -> Self {
        FileContents::Bytes(b.into())
    }

    pub(crate) fn from_thrift(fc: thrift::FileContents) -> Result<Self> {
        match fc {
            thrift::FileContents::Bytes(bytes) => Ok(FileContents::Bytes(bytes.into())),
            thrift::FileContents::Chunked(chunked) => {
                let contents = ChunkedFileContents::from_thrift(chunked)?;
                Ok(FileContents::Chunked(contents))
            }
            thrift::FileContents::UnknownField(x) => bail_err!(ErrorKind::InvalidThrift(
                "FileContents".into(),
                format!("unknown file contents field: {}", x)
            )),
        }
    }

    pub(crate) fn into_thrift(self) -> thrift::FileContents {
        match self {
            // TODO (T26959816) -- allow Thrift to represent binary as Bytes
            FileContents::Bytes(bytes) => thrift::FileContents::Bytes(bytes.to_vec()),
            FileContents::Chunked(chunked) => thrift::FileContents::Chunked(chunked.into_thrift()),
        }
    }

    pub fn from_encoded_bytes(encoded_bytes: Bytes) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(encoded_bytes.as_ref())
            .chain_err(ErrorKind::BlobDeserializeError("FileContents".into()))?;
        Self::from_thrift(thrift_tc)
    }

    pub fn content_id(&self) -> ContentId {
        match self {
            this @ FileContents::Bytes(..) => *this.clone().into_blob().id(),
            FileContents::Chunked(chunked) => chunked.content_id(),
        }
    }

    pub fn size(&self) -> u64 {
        match self {
            // NOTE: This unwrap() will panic iif we have a Bytes in memory that's larger than a
            // u64. That's not going to happen.
            FileContents::Bytes(bytes) => bytes.len().try_into().unwrap(),
            FileContents::Chunked(chunked) => chunked.size(),
        }
    }
}

impl BlobstoreValue for FileContents {
    type Key = ContentId;

    fn into_blob(self) -> ContentBlob {
        let id = match &self {
            FileContents::Bytes(bytes) => {
                let mut context = ContentIdContext::new();
                context.update(&bytes);
                context.finish()
            }
            FileContents::Chunked(chunked) => chunked.content_id(),
        };

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
            FileContents::Chunked(ref chunked) => write!(
                f,
                "FileContents::Chunked({}, length {})",
                chunked.content_id(),
                chunked.size()
            ),
        }
    }
}

impl Arbitrary for FileContents {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        if bool::arbitrary(g) {
            FileContents::new_bytes(Vec::arbitrary(g))
        } else {
            FileContents::Chunked(ChunkedFileContents::arbitrary(g))
        }
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        match self {
            FileContents::Bytes(..) => single_shrinker(FileContents::new_bytes(vec![])),
            FileContents::Chunked(..) => empty_shrinker(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ChunkedFileContents {
    content_id: ContentId,
    chunks: Vec<ContentChunkPointer>,
    // NOTE: We compute the size upon construction to make size() a O(1) call, not O(len(chunks)).
    size: u64,
}

impl ChunkedFileContents {
    pub fn new(content_id: ContentId, chunks: Vec<ContentChunkPointer>) -> Self {
        let size = chunks.iter().map(|c| c.size).sum();

        Self {
            content_id,
            chunks,
            size,
        }
    }

    pub fn from_bytes(blob: Bytes) -> Result<Self> {
        let thrift_chunked = compact_protocol::deserialize(blob.as_ref()).chain_err(
            ErrorKind::BlobDeserializeError("ChunkedFileContents".into()),
        )?;
        Self::from_thrift(thrift_chunked)
    }

    pub fn from_thrift(thrift_chunked: thrift::ChunkedFileContents) -> Result<Self> {
        let content_id = ContentId::from_thrift(thrift_chunked.content_id)?;
        let chunks = thrift_chunked
            .chunks
            .into_iter()
            .map(ContentChunkPointer::from_thrift)
            .collect::<Result<Vec<_>>>()?;

        Ok(Self::new(content_id, chunks))
    }

    pub fn into_thrift(self) -> thrift::ChunkedFileContents {
        let content_id = self.content_id.into_thrift();
        let chunks = self
            .chunks
            .into_iter()
            .map(ContentChunkPointer::into_thrift)
            .collect();
        thrift::ChunkedFileContents { content_id, chunks }
    }

    pub fn into_chunks(self) -> Vec<ContentChunkPointer> {
        self.chunks
    }

    pub fn content_id(&self) -> ContentId {
        self.content_id
    }

    pub fn size(&self) -> u64 {
        self.size
    }
}

impl Arbitrary for ChunkedFileContents {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        Self::new(ContentId::arbitrary(g), Vec::arbitrary(g))
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Copy)]
pub struct ContentChunkPointer {
    chunk_id: ContentChunkId,
    size: u64,
}

impl ContentChunkPointer {
    pub fn new(chunk_id: ContentChunkId, size: u64) -> Self {
        Self { chunk_id, size }
    }

    pub fn from_bytes(blob: Bytes) -> Result<Self> {
        let thrift_chunk = compact_protocol::deserialize(blob.as_ref()).chain_err(
            ErrorKind::BlobDeserializeError("ContentChunkPointer".into()),
        )?;
        Self::from_thrift(thrift_chunk)
    }

    pub fn from_thrift(thrift_chunk: thrift::ContentChunkPointer) -> Result<Self> {
        let chunk_id = ContentChunkId::from_thrift(thrift_chunk.chunk_id)?;
        let size: u64 = thrift_chunk.size.try_into()?;
        Ok(Self::new(chunk_id, size))
    }

    pub fn into_thrift(self) -> thrift::ContentChunkPointer {
        // NOTE: unwrap() will fail here if we are dealing with a chunk whose size doesn't fit an
        // i64 but does fit a u64. This isn't something we meaningfully seek to support at the
        // moment.
        let chunk_id = self.chunk_id.into_thrift();
        let size: i64 = self.size.try_into().unwrap();
        thrift::ContentChunkPointer { chunk_id, size }
    }

    pub fn chunk_id(&self) -> ContentChunkId {
        self.chunk_id
    }

    pub fn size(&self) -> u64 {
        self.size
    }
}

impl Arbitrary for ContentChunkPointer {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        Self::new(ContentChunkId::arbitrary(g), u64::arbitrary(g))
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ContentAlias(ContentId);

impl ContentAlias {
    pub fn from_content_id(id: ContentId) -> Self {
        ContentAlias(id)
    }

    pub fn from_bytes(blob: Bytes) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(blob.as_ref())
            .chain_err(ErrorKind::BlobDeserializeError("ContentAlias".into()))?;
        Self::from_thrift(thrift_tc)
    }

    pub fn from_thrift(ca: thrift::ContentAlias) -> Result<Self> {
        match ca {
            thrift::ContentAlias::ContentId(id) => {
                Ok(Self::from_content_id(ContentId::from_thrift(id)?))
            }
            thrift::ContentAlias::UnknownField(x) => bail_err!(ErrorKind::InvalidThrift(
                "ContentAlias".into(),
                format!("unknown content alias field: {}", x)
            )),
        }
    }

    pub fn into_blob(self) -> BlobstoreBytes {
        let alias = thrift::ContentAlias::ContentId(self.0.into_thrift());
        BlobstoreBytes::from_bytes(compact_protocol::serialize(&alias))
    }

    pub fn content_id(&self) -> ContentId {
        self.0
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ContentMetadata {
    pub total_size: u64,
    pub content_id: ContentId,
    pub sha1: hash::Sha1,
    pub sha256: hash::Sha256,
    pub git_sha1: hash::GitSha1,
}

impl ContentMetadata {
    pub fn from_thrift(cab: thrift::ContentMetadata) -> Result<Self> {
        let total_size = thrift_field!(ContentMetadata, cab, total_size)?;
        let total_size: u64 = total_size.try_into()?;

        let res = ContentMetadata {
            total_size,
            content_id: ContentId::from_thrift(thrift_field!(ContentMetadata, cab, content_id)?)?,
            sha1: hash::Sha1::from_bytes(&thrift_field!(ContentMetadata, cab, sha1)?.0)?,
            sha256: hash::Sha256::from_bytes(&thrift_field!(ContentMetadata, cab, sha256)?.0)?,
            git_sha1: hash::GitSha1::from_bytes(
                &thrift_field!(ContentMetadata, cab, git_sha1)?.0,
                "blob",
                total_size,
            )?,
        };

        Ok(res)
    }

    fn into_thrift(self) -> thrift::ContentMetadata {
        thrift::ContentMetadata {
            total_size: Some(self.total_size as i64),
            content_id: Some(self.content_id.into_thrift()),
            sha1: Some(self.sha1.into_thrift()),
            git_sha1: Some(self.git_sha1.into_thrift()),
            sha256: Some(self.sha256.into_thrift()),
        }
    }
}

impl Arbitrary for ContentMetadata {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        let total_size = u64::arbitrary(g);

        Self {
            total_size,
            content_id: ContentId::arbitrary(g),
            sha1: hash::Sha1::arbitrary(g),
            sha256: hash::Sha256::arbitrary(g),
            git_sha1: hash::GitSha1::from_sha1(hash::Sha1::arbitrary(g), "blob", total_size),
        }
    }
}

impl BlobstoreValue for ContentMetadata {
    type Key = ContentMetadataId;

    fn into_blob(self) -> ContentMetadataBlob {
        let id = From::from(self.content_id.clone());
        let thrift = self.into_thrift();
        let data = compact_protocol::serialize(&thrift);
        Blob::new(id, data)
    }

    fn from_blob(blob: ContentMetadataBlob) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(blob.data().as_ref())
            .chain_err(ErrorKind::BlobDeserializeError("ContentMetadata".into()))?;
        Self::from_thrift(thrift_tc)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use quickcheck::quickcheck;

    quickcheck! {
        fn file_contents_thrift_roundtrip(fc: FileContents) -> bool {
            let thrift_fc = fc.clone().into_thrift();
            let fc2 = FileContents::from_thrift(thrift_fc)
                .expect("thrift roundtrips should always be valid");
            fc == fc2
        }

        fn file_contents_blob_roundtrip(fc: FileContents) -> bool {
            let blob = fc.clone().into_blob();
            let fc2 = FileContents::from_blob(blob)
                .expect("blob roundtrips should always be valid");
            fc == fc2
        }

        fn content_alias_metadata_thrift_roundtrip(cab: ContentMetadata) -> bool {
            let thrift_cab = cab.clone().into_thrift();
            let cab2 = ContentMetadata::from_thrift(thrift_cab)
                .expect("thrift roundtrips should always be valid");
            println!("cab: {:?}", cab);
            println!("cab2: {:?}", cab2);
            cab == cab2
        }

        fn content_alias_metadata_blob_roundtrip(cab: ContentMetadata) -> bool {
            let blob = cab.clone().into_blob();
            let cab2 = ContentMetadata::from_blob(blob)
                .expect("blob roundtrips should always be valid");
            cab == cab2
        }
    }

    #[test]
    fn bad_thrift() {
        let thrift_fc = thrift::FileContents::UnknownField(-1);
        FileContents::from_thrift(thrift_fc).expect_err("unexpected OK - unknown field");
    }
}
