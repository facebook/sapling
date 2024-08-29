/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;
use std::str::FromStr;

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
use flate2::write::ZlibEncoder;
use flate2::Compression;
use futures::future;
use futures::stream;
use futures::stream::BoxStream;
use futures::StreamExt;
use futures::TryStreamExt;
use gix_hash::oid;
use gix_hash::ObjectId;
use gix_object::Kind;
use mononoke_types::hash::Blake2;
use mononoke_types::hash::BLAKE2_HASH_LENGTH_BYTES;
use mononoke_types::impl_typed_hash;
use mononoke_types::path::MPath;
use mononoke_types::sharded_map_v2::ShardedMapV2Node;
use mononoke_types::sharded_map_v2::ShardedMapV2Value;
use mononoke_types::typed_hash::IdContext;
use mononoke_types::Blob;
use mononoke_types::BlobstoreKey;
use mononoke_types::BlobstoreValue;
use mononoke_types::MononokeId;
use mononoke_types::ThriftConvert;

use crate::thrift;
use crate::TreeMember;

/// A manifest that contains an entry for each Git object that was added or modified as part of
/// a commit. The object needs to be different from all objects at the same path in all parents
/// for it to be included.
#[derive(ThriftConvert, Clone, Debug, Eq, PartialEq, Hash)]
#[thrift(thrift::GitDeltaManifestV2)]
pub struct GitDeltaManifestV2 {
    entries: ShardedMapV2Node<GDMV2Entry>,
}

/// An entry in the GitDeltaManifestV2 corresponding to a path
impl GitDeltaManifestV2 {
    pub async fn from_entries(
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        entries: impl IntoIterator<Item = (MPath, GDMV2Entry)>,
    ) -> Result<Self> {
        Ok(Self {
            entries: ShardedMapV2Node::from_entries(
                ctx,
                blobstore,
                entries.into_iter().map(|(path, entry)| {
                    // Convert the MPath into Vec<u8> by merging MPathElements with null byte as the separator. We use the null-separated
                    // path as the key in the ShardedMap to allow for proper ordering of paths.
                    (path.to_null_separated_bytes(), entry)
                }),
            )
            .await?,
        })
    }

    pub fn into_entries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
    ) -> BoxStream<'a, Result<(MPath, GDMV2Entry)>> {
        self.entries
            .into_entries_unordered(ctx, blobstore, 200)
            .and_then(|(bytes, entry)| async move {
                let path = MPath::from_null_separated_bytes(bytes.to_vec())?;
                anyhow::Ok((path, entry))
            })
            .boxed()
    }

    pub async fn lookup(
        &self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        name: &MPath,
    ) -> Result<Option<GDMV2Entry>> {
        let path = name.to_null_separated_bytes();
        self.entries.lookup(ctx, blobstore, path.as_ref()).await
    }
}

#[derive(ThriftConvert, Clone, Debug, Eq, PartialEq, Hash)]
#[thrift(thrift::GDMV2Entry)]
pub struct GDMV2Entry {
    /// The full object that this entry represents
    pub full_object: GDMV2ObjectEntry,
    /// A list of entries corresponding to ways to represent this object
    /// as a delta
    pub deltas: Vec<GDMV2DeltaEntry>,
}

impl ShardedMapV2Value for GDMV2Entry {
    type NodeId = ShardedMapV2GDMV2EntryId;
    type Context = ShardedMapV2GDMV2EntryIdContext;
    type RollupData = ();

    const WEIGHT_LIMIT: usize = 1_000_000;

    fn weight(&self) -> usize {
        // This is an approximation of the number of bytes in the entry.
        let inlined_bytes_size = self
            .full_object
            .inlined_bytes
            .as_ref()
            .map_or(0, |bytes| bytes.len());
        let deltas_size = self
            .deltas
            .iter()
            .map(|delta| delta.instructions.instruction_bytes.approximate_size())
            .sum::<usize>();
        1 + inlined_bytes_size + deltas_size
    }
}

impl GDMV2Entry {
    pub fn has_deltas(&self) -> bool {
        !self.deltas.is_empty()
    }
}

/// Struct representing a Git object's metadata in GitDeltaManifestV2.
/// Contains inlined bytes of the object if it's considered small enough.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct GDMV2ObjectEntry {
    pub oid: ObjectId,
    pub size: u64,
    pub kind: ObjectKind,
    pub inlined_bytes: Option<Bytes>,
}

impl GDMV2ObjectEntry {
    pub fn from_tree_member(member: &TreeMember, inlined_bytes: Option<Bytes>) -> Result<Self> {
        let oid = ObjectId::from_hex(member.oid().to_hex().as_bytes()).with_context(|| {
            format!(
                "Error while converting hash {:?} to ObjectId",
                member.oid().to_hex()
            )
        })?;
        let size = member.oid().size();
        let kind = match member.kind() {
            crate::ObjectKind::Blob => crate::DeltaObjectKind::Blob,
            crate::ObjectKind::Tree => crate::DeltaObjectKind::Tree,
            kind => anyhow::bail!("Unexpected object kind {:?} for DeltaObjectEntry", kind),
        };

        Ok(GDMV2ObjectEntry {
            oid,
            size,
            kind,
            inlined_bytes,
        })
    }
}

/// Enum representing the types of Git objects that can be present
/// in a GitDeltaManifest
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum ObjectKind {
    Blob,
    Tree,
}

impl ObjectKind {
    pub fn to_gix_kind(&self) -> Kind {
        match self {
            ObjectKind::Blob => Kind::Blob,
            ObjectKind::Tree => Kind::Tree,
        }
    }

    pub fn is_tree(&self) -> bool {
        *self == ObjectKind::Tree
    }

    pub fn is_blob(&self) -> bool {
        *self == ObjectKind::Blob
    }
}

impl TryFrom<thrift::ObjectKind> for ObjectKind {
    type Error = anyhow::Error;

    fn try_from(value: thrift::ObjectKind) -> Result<Self, Self::Error> {
        match value {
            thrift::ObjectKind::Blob => Ok(Self::Blob),
            thrift::ObjectKind::Tree => Ok(Self::Tree),
            thrift::ObjectKind(x) => anyhow::bail!("Unsupported object kind: {}", x),
        }
    }
}

impl From<ObjectKind> for thrift::ObjectKind {
    fn from(value: ObjectKind) -> Self {
        match value {
            ObjectKind::Blob => thrift::ObjectKind::Blob,
            ObjectKind::Tree => thrift::ObjectKind::Tree,
        }
    }
}

impl ThriftConvert for ObjectKind {
    const NAME: &'static str = "ObjectKind";
    type Thrift = thrift::ObjectKind;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        t.try_into()
    }

    fn into_thrift(self) -> Self::Thrift {
        self.into()
    }
}

#[derive(ThriftConvert, Clone, Debug, Eq, PartialEq, Hash)]
#[thrift(thrift::GDMV2DeltaEntry)]
pub struct GDMV2DeltaEntry {
    pub base_object: GDMV2ObjectEntry,
    pub base_object_path: MPath,
    pub instructions: GDMV2Instructions,
}

#[derive(ThriftConvert, Clone, Debug, Eq, PartialEq, Hash)]
#[thrift(thrift::GDMV2Instructions)]
pub struct GDMV2Instructions {
    pub uncompressed_size: u64,
    pub compressed_size: u64,
    pub instruction_bytes: GDMV2InstructionBytes,
}

impl GDMV2Instructions {
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
                    GDMV2InstructionBytes::Inlined(
                        fallible_bytes
                            .await
                            .context("Error in getting inlined bytes from chunk stream")?,
                    )
                } else {
                    GDMV2InstructionBytes::Chunked(vec![
                        GDMV2InstructionsChunk(
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
                GDMV2InstructionBytes::Chunked(
                    bytes_stream
                        .enumerate()
                        .map(|(idx, fallible_bytes)| async move {
                            let instructions_chunk =
                                GDMV2InstructionsChunk(fallible_bytes.with_context(|| {
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

impl GDMV2InstructionBytes {
    pub async fn into_raw_bytes(
        self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
    ) -> Result<Bytes> {
        match self {
            GDMV2InstructionBytes::Inlined(bytes) => Ok(bytes),
            GDMV2InstructionBytes::Chunked(chunks) => {
                Ok(stream::iter(chunks)
                    .map(|chunk: GDMV2InstructionsChunkId| async move {
                        chunk.load(ctx, blobstore).await
                    })
                    .buffered(24)
                    .try_fold(BytesMut::new(), |mut acc, chunk| async move {
                        acc.extend_from_slice(&chunk.0);
                        Ok(acc)
                    })
                    .await?
                    .freeze())
            }
        }
    }
}

#[derive(ThriftConvert, Clone, Debug, Eq, PartialEq, Hash)]
#[thrift(thrift::GDMV2InstructionBytes)]
pub enum GDMV2InstructionBytes {
    /// The instruction bytes are stored inlined
    Inlined(Bytes),
    /// The instruction bytes are stored in separate chunked blobs, with only
    /// a list of their ids stored inline
    Chunked(Vec<GDMV2InstructionsChunkId>),
}

impl GDMV2InstructionBytes {
    fn approximate_size(&self) -> usize {
        match self {
            GDMV2InstructionBytes::Inlined(bytes) => bytes.len(),
            GDMV2InstructionBytes::Chunked(chunks) => chunks.len() * BLAKE2_HASH_LENGTH_BYTES,
        }
    }
}

pub struct GDMV2InstructionsChunk(Bytes);

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct GDMV2InstructionsChunkId(Blake2);

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct ShardedMapV2GDMV2EntryId(Blake2);

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct GitDeltaManifestV2Id(Blake2);

impl ThriftConvert for GDMV2ObjectEntry {
    const NAME: &'static str = "GDMV2ObjectEntry";
    type Thrift = thrift::GDMV2ObjectEntry;

    fn from_thrift(thrift: Self::Thrift) -> Result<Self> {
        Ok(Self {
            oid: oid::try_from_bytes(&thrift.oid.0)?.to_owned(),
            size: thrift.size as u64,
            kind: ObjectKind::from_thrift(thrift.kind)?,
            inlined_bytes: thrift.inlined_bytes,
        })
    }
    fn into_thrift(self) -> Self::Thrift {
        thrift::GDMV2ObjectEntry {
            oid: mononoke_types_serialization::id::GitSha1(self.oid.as_bytes().into()),
            size: self.size as i64,
            kind: self.kind.into_thrift(),
            inlined_bytes: self.inlined_bytes,
            ..Default::default()
        }
    }
}

impl ThriftConvert for GDMV2InstructionsChunk {
    const NAME: &'static str = "GDMV2InstructionsChunk";
    type Thrift = thrift::GDMV2InstructionsChunk;

    fn from_thrift(thrift: Self::Thrift) -> Result<Self> {
        Ok(Self(thrift.0))
    }
    fn into_thrift(self) -> Self::Thrift {
        thrift::GDMV2InstructionsChunk(self.0)
    }
}

impl_typed_hash! {
    hash_type => GitDeltaManifestV2Id,
    thrift_hash_type => thrift::GitDeltaManifestV2Id,
    value_type => GitDeltaManifestV2,
    context_type => GitDeltaManifestV2IdContext,
    context_key => "gdm2",
}

impl_typed_hash! {
    hash_type => GDMV2InstructionsChunkId,
    thrift_hash_type => thrift::GDMV2InstructionsChunkId,
    value_type => GDMV2InstructionsChunk,
    context_type => GDMV2InstructionsChunkIdContext,
    context_key => "gdm2_instructions_chunk",
}

impl_typed_hash! {
    hash_type => ShardedMapV2GDMV2EntryId,
    thrift_hash_type => mononoke_types_serialization::id::ShardedMapV2NodeId,
    value_type => ShardedMapV2Node<GDMV2Entry>,
    context_type => ShardedMapV2GDMV2EntryIdContext,
    context_key => "gdm2.map2node",
}

impl BlobstoreValue for GDMV2InstructionsChunk {
    type Key = GDMV2InstructionsChunkId;

    fn into_blob(self) -> Blob<Self::Key> {
        let data = self.into_bytes();
        let id = GDMV2InstructionsChunkIdContext::id_from_data(&data);
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        Self::from_bytes(blob.data())
    }
}

impl BlobstoreValue for GitDeltaManifestV2 {
    type Key = GitDeltaManifestV2Id;

    fn into_blob(self) -> Blob<Self::Key> {
        let data = self.into_bytes();
        let id = GitDeltaManifestV2IdContext::id_from_data(&data);
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        Self::from_bytes(blob.data())
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
    async fn test_gdm_v2_delta_instructions_round_trip(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let blobstore = DelayedBlobstore::new(
            Memblob::default(),
            rand_distr::Normal::new(0.005, 0.005).unwrap(),
            rand_distr::Normal::new(0.05, 0.05).unwrap(),
        );

        let delta = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let chunk_size = 1;
        let max_inlinable_size = 0;

        let gdm_v2_instructions = GDMV2Instructions::from_raw_delta(
            &ctx,
            &blobstore,
            delta.clone(),
            chunk_size,
            max_inlinable_size,
        )
        .await
        .unwrap();

        let delta_bytes = gdm_v2_instructions
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
