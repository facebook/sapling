/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::LoadableError;
use blobstore::Storable;
use bytes::Bytes;
use bytes::BytesMut;
use context::CoreContext;
use flate2::Compression;
use flate2::write::ZlibEncoder;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::future;
use futures::stream;
use futures::stream::BoxStream;
use gix_hash::ObjectId;
use gix_hash::oid;
use manifest::Entry;
use mononoke_types::Blob;
use mononoke_types::BlobstoreKey;
use mononoke_types::BlobstoreValue;
use mononoke_types::MononokeId;
use mononoke_types::ThriftConvert;
use mononoke_types::hash::Blake2;
use mononoke_types::impl_typed_hash;
use mononoke_types::path::MPath;
use mononoke_types::typed_hash::IdContext;

use crate::GitDeltaManifestEntryOps;
use crate::GitDeltaManifestOps;
use crate::GitLeaf;
use crate::GitTreeId;
use crate::ObjectDeltaOps;
use crate::delta_manifest_ops::ObjectKind;
use crate::thrift;

/// A manifest that contains an entry for each Git object that was added or modified as part of
/// a commit. The object needs to be different from all objects at the same path in all parents
/// for it to be included.
#[derive(ThriftConvert, Clone, Debug, Eq, PartialEq, Hash)]
#[thrift(thrift::GitDeltaManifestV3)]
pub enum GitDeltaManifestV3 {
    Inlined(Vec<GDMV3Entry>),
    Chunked(Vec<GDMV3ChunkId>),
}

impl GitDeltaManifestV3 {
    pub async fn from_entries(
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        entries: Vec<GDMV3Entry>,
        max_inlinable_size: usize,
    ) -> Result<Self> {
        let total_inlined_bytes_size: usize =
            entries.iter().map(|entry| entry.inlined_bytes_size()).sum();

        if total_inlined_bytes_size <= max_inlinable_size {
            return Ok(Self::Inlined(entries));
        }

        let mut chunks = vec![];
        let mut last_chunk = vec![];
        let mut last_chunk_size = 0;
        for entry in entries {
            let entry_size = entry.inlined_bytes_size();
            if !last_chunk.is_empty() && last_chunk_size + entry_size > max_inlinable_size {
                chunks.push(last_chunk);
                last_chunk = vec![entry];
                last_chunk_size = entry_size;
            } else {
                last_chunk.push(entry);
                last_chunk_size += entry_size;
            }
        }
        if !last_chunk.is_empty() {
            chunks.push(last_chunk);
        }

        Ok(Self::Chunked(
            stream::iter(chunks)
                .map(async |chunk| {
                    let chunk = GDMV3Chunk { entries: chunk };
                    chunk.into_blob().store(ctx, blobstore).await
                })
                .buffered(24)
                .try_collect::<Vec<_>>()
                .await?,
        ))
    }

    pub fn into_entries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
    ) -> BoxStream<'a, Result<GDMV3Entry>> {
        match self {
            Self::Inlined(entries) => stream::iter(entries).map(Ok).boxed(),
            Self::Chunked(chunk_ids) => stream::iter(chunk_ids)
                .map(async |chunk_id| {
                    let chunk = chunk_id.load(ctx, blobstore).await?;
                    anyhow::Ok(stream::iter(chunk.entries).map(Ok))
                })
                .buffered(24)
                .try_flatten()
                .boxed(),
        }
    }
}

#[derive(ThriftConvert, Clone, Debug, Eq, PartialEq, Hash)]
#[thrift(thrift::GDMV3Chunk)]
pub struct GDMV3Chunk {
    pub entries: Vec<GDMV3Entry>,
}

/// An entry in the GitDeltaManifestV3 corresponding to a path
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct GDMV3Entry {
    /// The path of the object
    pub path: MPath,
    /// The full object that this entry represents
    pub full_object: GDMV3ObjectEntry,
    /// A delta entry corresponding to a way to represent this object
    /// as a delta
    pub delta: Option<GDMV3DeltaEntry>,
}

impl GDMV3Entry {
    fn inlined_bytes_size(&self) -> usize {
        self.full_object
            .inlined_bytes
            .as_ref()
            .map_or(0, |bytes| bytes.len())
            + self
                .delta
                .as_ref()
                .map_or(0, |delta| match &delta.instructions.instruction_bytes {
                    GDMV3InstructionBytes::Inlined(bytes) => bytes.len(),
                    GDMV3InstructionBytes::Chunked(_chunks) => 0,
                })
    }
}

impl ThriftConvert for GDMV3Entry {
    const NAME: &'static str = "GDMV3Entry";
    type Thrift = thrift::GDMV3Entry;

    fn from_thrift(thrift: Self::Thrift) -> Result<Self> {
        Ok(Self {
            path: MPath::from_thrift(thrift.path)?,
            full_object: GDMV3ObjectEntry::from_thrift(thrift.full_object)?,
            delta: thrift.delta.map(GDMV3DeltaEntry::from_thrift).transpose()?,
        })
    }

    fn into_thrift(self) -> Self::Thrift {
        thrift::GDMV3Entry {
            path: self.path.into_thrift(),
            full_object: self.full_object.into_thrift(),
            delta: self.delta.map(GDMV3DeltaEntry::into_thrift),
            ..Default::default()
        }
    }
}

/// Struct representing a Git object's metadata in GitDeltaManifestV2.
/// Contains inlined bytes of the object if it's considered small enough.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct GDMV3ObjectEntry {
    pub oid: ObjectId,
    pub size: u64,
    pub kind: ObjectKind,
    pub inlined_bytes: Option<Bytes>,
}

impl GDMV3ObjectEntry {
    pub async fn from_tree_entry(
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        entry: &Entry<GitTreeId, GitLeaf>,
        inlined_bytes: Option<Bytes>,
    ) -> Result<Self> {
        let (oid, size, kind) = match entry {
            Entry::Leaf(leaf) => (
                leaf.oid(),
                leaf.size(ctx, blobstore).await?,
                ObjectKind::Blob,
            ),
            Entry::Tree(tree) => (tree.0, tree.size(ctx, blobstore).await?, ObjectKind::Tree),
        };

        Ok(GDMV3ObjectEntry {
            oid,
            size,
            kind,
            inlined_bytes,
        })
    }
}

#[derive(ThriftConvert, Clone, Debug, Eq, PartialEq, Hash)]
#[thrift(thrift::GDMV3DeltaEntry)]
pub struct GDMV3DeltaEntry {
    pub base_object: GDMV3ObjectEntry,
    pub instructions: GDMV3Instructions,
}

#[derive(ThriftConvert, Clone, Debug, Eq, PartialEq, Hash)]
#[thrift(thrift::GDMV3Instructions)]
pub struct GDMV3Instructions {
    pub uncompressed_size: u64,
    pub compressed_size: u64,
    pub instruction_bytes: GDMV3InstructionBytes,
}

impl GDMV3Instructions {
    pub async fn from_raw_delta(
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        delta: Vec<u8>,
        chunk_size: u64,
        max_inlinable_size: u64,
    ) -> Result<Self> {
        let raw_instruction_bytes = delta;
        let uncompressed_size = raw_instruction_bytes.len() as u64;

        // Zlib encode the instructions before writing to the store
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&raw_instruction_bytes)
            .context("Failure in writing raw delta instruction bytes to ZLib buffer")?;
        let compressed_instruction_bytes = encoder
            .finish()
            .context("Failure in ZLib encoding delta instruction bytes")?;
        let compressed_size = compressed_instruction_bytes.len() as u64;

        let size = filestore::ExpectedSize::new(compressed_size);
        let raw_instructions_stream =
            stream::once(future::ok(Bytes::from(compressed_instruction_bytes)));
        let chunk_stream = filestore::make_chunks(raw_instructions_stream, size, Some(chunk_size));

        let instruction_bytes = match chunk_stream {
            filestore::Chunks::Inline(fallible_bytes) => {
                if compressed_size <= max_inlinable_size {
                    GDMV3InstructionBytes::Inlined(
                        fallible_bytes
                            .await
                            .context("Error in getting inlined bytes from chunk stream")?,
                    )
                } else {
                    GDMV3InstructionBytes::Chunked(vec![
                        GDMV3InstructionsChunk(
                            fallible_bytes
                                .await
                                .context("Error in getting bytes from chunk stream")?,
                        )
                        .into_blob()
                        .store(ctx, blobstore)
                        .await?,
                    ])
                }
            }
            filestore::Chunks::Chunked(_, bytes_stream) => {
                GDMV3InstructionBytes::Chunked(
                    bytes_stream
                        .enumerate()
                        .map(|(idx, fallible_bytes)| async move {
                            let instructions_chunk =
                                GDMV3InstructionsChunk(fallible_bytes.with_context(|| {
                                    format!(
                                        "Error in getting bytes from chunk {} in chunked stream",
                                        idx
                                    )
                                })?);
                            instructions_chunk.into_blob().store(ctx, blobstore).await
                        })
                        .buffered(24) // Same as the concurrency used for filestore
                        .try_collect::<Vec<_>>()
                        .await?,
                )
            }
        };

        Ok(Self {
            uncompressed_size,
            compressed_size,
            instruction_bytes,
        })
    }
}

impl GDMV3InstructionBytes {
    pub async fn into_raw_bytes(
        self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
    ) -> Result<Vec<u8>> {
        match self {
            GDMV3InstructionBytes::Inlined(bytes) => Ok(bytes.into()),
            GDMV3InstructionBytes::Chunked(chunks) => {
                Ok(stream::iter(chunks)
                    .map(|chunk: GDMV3InstructionsChunkId| async move {
                        chunk.load(ctx, blobstore).await
                    })
                    .buffered(24)
                    .try_fold(BytesMut::new(), |mut acc, chunk| async move {
                        acc.extend_from_slice(&chunk.0);
                        Ok(acc)
                    })
                    .await?
                    .to_vec())
            }
        }
    }
}

#[derive(ThriftConvert, Clone, Debug, Eq, PartialEq, Hash)]
#[thrift(thrift::GDMV3InstructionBytes)]
pub enum GDMV3InstructionBytes {
    /// The instruction bytes are stored inlined
    Inlined(Bytes),
    /// The instruction bytes are stored in separate chunked blobs, with only
    /// a list of their ids stored inline
    Chunked(Vec<GDMV3InstructionsChunkId>),
}

pub struct GDMV3InstructionsChunk(Bytes);

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct GDMV3InstructionsChunkId(Blake2);

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct GDMV3ChunkId(Blake2);

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct GitDeltaManifestV3Id(Blake2);

impl ThriftConvert for GDMV3ObjectEntry {
    const NAME: &'static str = "GDMV3ObjectEntry";
    type Thrift = thrift::GDMV3ObjectEntry;

    fn from_thrift(thrift: Self::Thrift) -> Result<Self> {
        Ok(Self {
            oid: oid::try_from_bytes(&thrift.oid.0)?.to_owned(),
            size: thrift.size as u64,
            kind: ObjectKind::from_thrift(thrift.kind)?,
            inlined_bytes: thrift.inlined_bytes,
        })
    }
    fn into_thrift(self) -> Self::Thrift {
        thrift::GDMV3ObjectEntry {
            oid: mononoke_types_serialization::id::GitSha1(self.oid.as_bytes().into()),
            size: self.size as i64,
            kind: self.kind.into_thrift(),
            inlined_bytes: self.inlined_bytes,
            ..Default::default()
        }
    }
}

impl ThriftConvert for GDMV3InstructionsChunk {
    const NAME: &'static str = "GDMV3InstructionsChunk";
    type Thrift = thrift::GDMV3InstructionsChunk;

    fn from_thrift(thrift: Self::Thrift) -> Result<Self> {
        Ok(Self(thrift.0))
    }
    fn into_thrift(self) -> Self::Thrift {
        thrift::GDMV3InstructionsChunk(self.0)
    }
}

impl_typed_hash! {
    hash_type => GitDeltaManifestV3Id,
    thrift_hash_type => thrift::GitDeltaManifestV3Id,
    value_type => GitDeltaManifestV3,
    context_type => GitDeltaManifestV3IdContext,
    context_key => "gdm3",
}

impl_typed_hash! {
    hash_type => GDMV3ChunkId,
    thrift_hash_type => thrift::GDMV3ChunkId,
    value_type => GDMV3Chunk,
    context_type => GDMV3ChunkIdContext,
    context_key => "gdm3_chunk",
}

impl_typed_hash! {
    hash_type => GDMV3InstructionsChunkId,
    thrift_hash_type => thrift::GDMV3InstructionsChunkId,
    value_type => GDMV3InstructionsChunk,
    context_type => GDMV3InstructionsChunkIdContext,
    context_key => "gdm3_instructions_chunk",
}

impl BlobstoreValue for GDMV3InstructionsChunk {
    type Key = GDMV3InstructionsChunkId;

    fn into_blob(self) -> Blob<Self::Key> {
        let data = self.into_bytes();
        let id = GDMV3InstructionsChunkIdContext::id_from_data(&data);
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        Self::from_bytes(blob.data())
    }
}

impl BlobstoreValue for GDMV3Chunk {
    type Key = GDMV3ChunkId;

    fn into_blob(self) -> Blob<Self::Key> {
        let data = self.into_bytes();
        let id = GDMV3ChunkIdContext::id_from_data(&data);
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        Self::from_bytes(blob.data())
    }
}

impl BlobstoreValue for GitDeltaManifestV3 {
    type Key = GitDeltaManifestV3Id;

    fn into_blob(self) -> Blob<Self::Key> {
        let data = self.into_bytes();
        let id = GitDeltaManifestV3IdContext::id_from_data(&data);
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        Self::from_bytes(blob.data())
    }
}

impl GitDeltaManifestOps for GitDeltaManifestV3 {
    fn into_entries<'a>(
        self: Box<Self>,
        ctx: &'a CoreContext,
        blobstore: &'a Arc<dyn Blobstore>,
    ) -> BoxStream<'a, Result<Box<dyn GitDeltaManifestEntryOps + Send>>> {
        GitDeltaManifestV3::into_entries(*self, ctx, blobstore)
            .map_ok(|entry| -> Box<dyn GitDeltaManifestEntryOps + Send> { Box::new(entry) })
            .boxed()
    }
}

impl GitDeltaManifestEntryOps for GDMV3Entry {
    fn path(&self) -> &MPath {
        &self.path
    }

    fn full_object_size(&self) -> u64 {
        self.full_object.size
    }

    fn full_object_oid(&self) -> ObjectId {
        self.full_object.oid
    }

    fn full_object_kind(&self) -> ObjectKind {
        self.full_object.kind
    }

    fn into_full_object_inlined_bytes(&mut self) -> Option<Vec<u8>> {
        self.full_object
            .inlined_bytes
            .take()
            .map(|bytes| bytes.into())
    }

    fn deltas(&self) -> Box<dyn Iterator<Item = &(dyn ObjectDeltaOps + Sync)> + '_> {
        Box::new(
            self.delta
                .iter()
                .map(|delta| delta as &(dyn ObjectDeltaOps + Sync)),
        )
    }
}

#[async_trait]
impl ObjectDeltaOps for GDMV3DeltaEntry {
    fn instructions_uncompressed_size(&self) -> u64 {
        self.instructions.uncompressed_size
    }

    fn instructions_compressed_size(&self) -> u64 {
        self.instructions.compressed_size
    }

    fn base_object_oid(&self) -> ObjectId {
        self.base_object.oid
    }

    fn base_object_kind(&self) -> ObjectKind {
        self.base_object.kind
    }

    fn base_object_size(&self) -> u64 {
        self.base_object.size
    }

    async fn instruction_bytes(
        &self,
        ctx: &CoreContext,
        blobstore: &Arc<dyn Blobstore>,
    ) -> Result<Vec<u8>> {
        self.instructions
            .instruction_bytes
            .clone()
            .into_raw_bytes(ctx, blobstore)
            .await
    }
}

#[cfg(test)]
mod tests {
    use delayblob::DelayedBlobstore;
    use fbinit::FacebookInit;
    use flate2::write::ZlibDecoder;
    use memblob::Memblob;
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::fbinit_test]
    async fn test_gdm_v3_delta_instructions_round_trip(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let blobstore = DelayedBlobstore::new(
            Memblob::default(),
            rand_distr::Normal::new(0.005, 0.005).unwrap(),
            rand_distr::Normal::new(0.05, 0.05).unwrap(),
        );

        let delta = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let chunk_size = 1;
        let max_inlinable_size = 0;

        let gdm_v3_instructions = GDMV3Instructions::from_raw_delta(
            &ctx,
            &blobstore,
            delta.clone(),
            chunk_size,
            max_inlinable_size,
        )
        .await
        .unwrap();

        let delta_bytes = gdm_v3_instructions
            .instruction_bytes
            .into_raw_bytes(&ctx, &blobstore)
            .await
            .unwrap();

        let round_trip_delta = vec![];
        let mut decoder = ZlibDecoder::new(round_trip_delta);
        decoder.write_all(delta_bytes.as_ref()).unwrap();
        let round_trip_delta = decoder.finish().unwrap();

        assert_eq!(delta, round_trip_delta);
    }
}
