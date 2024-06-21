/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::LoadableError;
use bytes::Bytes;
use context::CoreContext;
use futures::stream::BoxStream;
use futures::StreamExt;
use futures::TryStreamExt;
use gix_hash::oid;
use gix_hash::ObjectId;
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

use crate::delta_manifest::ObjectKind;
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
            .into_entries(ctx, blobstore)
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
