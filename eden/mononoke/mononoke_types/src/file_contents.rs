/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::fmt::Debug;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use bytes::Bytes;
use fbthrift::compact_protocol;
use quickcheck::empty_shrinker;
use quickcheck::single_shrinker;
use quickcheck::Arbitrary;
use quickcheck::Gen;

use crate::blob::Blob;
use crate::blob::BlobstoreValue;
use crate::blob::ContentBlob;
use crate::errors::ErrorKind;
use crate::thrift;
use crate::typed_hash::ContentChunkId;
use crate::typed_hash::ContentId;
use crate::typed_hash::ContentIdContext;

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
            thrift::FileContents::Bytes(bytes) => Ok(FileContents::Bytes(bytes)),
            thrift::FileContents::Chunked(chunked) => {
                let contents = ChunkedFileContents::from_thrift(chunked)?;
                Ok(FileContents::Chunked(contents))
            }
            thrift::FileContents::UnknownField(x) => bail!(ErrorKind::InvalidThrift(
                "FileContents".into(),
                format!("unknown file contents field: {}", x)
            )),
        }
    }

    pub(crate) fn into_thrift(self) -> thrift::FileContents {
        match self {
            FileContents::Bytes(bytes) => thrift::FileContents::Bytes(bytes),
            FileContents::Chunked(chunked) => thrift::FileContents::Chunked(chunked.into_thrift()),
        }
    }

    pub fn from_encoded_bytes(encoded_bytes: Bytes) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(encoded_bytes)
            .with_context(|| ErrorKind::BlobDeserializeError("FileContents".into()))?;
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

    pub fn content_id_for_bytes(bytes: &Bytes) -> ContentId {
        let mut context = ContentIdContext::new();
        context.update(&bytes);
        context.finish()
    }
}

impl BlobstoreValue for FileContents {
    type Key = ContentId;

    fn into_blob(self) -> ContentBlob {
        let id = match &self {
            Self::Bytes(bytes) => Self::content_id_for_bytes(bytes),
            Self::Chunked(chunked) => chunked.content_id(),
        };

        let thrift = self.into_thrift();
        let data = compact_protocol::serialize(&thrift);

        Blob::new(id, data)
    }

    fn from_blob(blob: ContentBlob) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(blob.data())
            .with_context(|| ErrorKind::BlobDeserializeError("FileContents".into()))?;
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
    fn arbitrary(g: &mut Gen) -> Self {
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
        let thrift_chunked = compact_protocol::deserialize(blob)
            .with_context(|| ErrorKind::BlobDeserializeError("ChunkedFileContents".into()))?;
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

    pub fn num_chunks(&self) -> usize {
        self.chunks.len()
    }

    pub fn iter_chunks(&self) -> impl Iterator<Item = &ContentChunkPointer> {
        self.chunks.iter()
    }

    pub fn content_id(&self) -> ContentId {
        self.content_id
    }

    pub fn size(&self) -> u64 {
        self.size
    }
}

impl Arbitrary for ChunkedFileContents {
    fn arbitrary(g: &mut Gen) -> Self {
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
        let thrift_chunk = compact_protocol::deserialize(blob)
            .with_context(|| ErrorKind::BlobDeserializeError("ContentChunkPointer".into()))?;
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
    fn arbitrary(g: &mut Gen) -> Self {
        // We don't want a big size because the sum of multiple ContentChunkPointer
        // should also fit into a i64
        let size = u32::arbitrary(g) as u64;
        Self::new(ContentChunkId::arbitrary(g), size)
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
    }

    #[test]
    fn bad_thrift() {
        let thrift_fc = thrift::FileContents::UnknownField(-1);
        FileContents::from_thrift(thrift_fc).expect_err("unexpected OK - unknown field");
    }
}
