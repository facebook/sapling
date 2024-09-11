/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;
use std::str::FromStr;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use base64::Engine;
use blake3::Hasher as Blake3Hasher;
use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use blobstore::Loadable;
use blobstore::LoadableError;
use blobstore::Storable;
use bytes::Bytes;
use context::CoreContext;
use futures::future;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_ext::FbStreamExt;
use manifest::Entry;
use manifest::Manifest;
use mononoke_types::hash::Blake2;
use mononoke_types::hash::Blake3;
use mononoke_types::hash::Sha1;
use mononoke_types::impl_typed_hash;
use mononoke_types::sharded_map_v2::LoadableShardedMapV2Node;
use mononoke_types::sharded_map_v2::Rollup;
use mononoke_types::sharded_map_v2::ShardedMapV2Node;
use mononoke_types::sharded_map_v2::ShardedMapV2Value;
use mononoke_types::Blob;
use mononoke_types::BlobstoreKey;
use mononoke_types::BlobstoreValue;
use mononoke_types::MononokeId;
use mononoke_types::ThriftConvert;

use crate::blobs::MononokeHgBlobError;
use crate::thrift;
use crate::FileType;
use crate::HgAugmentedManifestId;
use crate::HgNodeHash;
use crate::HgParents;
use crate::MPathElement;
use crate::MononokeHgError;
use crate::Type;
use crate::NULL_HASH;

const MAX_BUFFERED_ENTRIES: usize = 500;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct HgAugmentedFileLeafNode {
    pub file_type: FileType,
    pub filenode: HgNodeHash,
    pub content_blake3: Blake3,
    pub content_sha1: Sha1,
    pub total_size: u64,
    pub file_header_metadata: Option<Bytes>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct HgAugmentedDirectoryNode {
    pub treenode: HgNodeHash,
    pub augmented_manifest_id: Blake3,
    pub augmented_manifest_size: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum HgAugmentedManifestEntry {
    FileNode(HgAugmentedFileLeafNode),
    DirectoryNode(HgAugmentedDirectoryNode),
}

/// An identifier for a sharded map node used in (sharded) Augmented Manifest
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct ShardedMapV2NodeHgAugmentedManifestId(Blake2);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShardedHgAugmentedManifestRollupCount(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShardedHgAugmentedManifest {
    pub hg_node_id: HgNodeHash,
    pub p1: Option<HgNodeHash>,
    pub p2: Option<HgNodeHash>,
    pub computed_node_id: HgNodeHash,
    pub subentries: ShardedMapV2Node<HgAugmentedManifestEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HgAugmentedManifestEnvelope {
    // Expected to match the hash of the encoded augmented mf.
    pub augmented_manifest_id: Blake3,
    // Expected to match the size of the encoded augmented mf.
    pub augmented_manifest_size: u64,
    pub augmented_manifest: ShardedHgAugmentedManifest,
}

impl ShardedHgAugmentedManifest {
    pub async fn lookup(
        &self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        name: &MPathElement,
    ) -> Result<Option<HgAugmentedManifestEntry>> {
        self.subentries.lookup(ctx, blobstore, name.as_ref()).await
    }

    pub fn into_subentries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
    ) -> BoxStream<'a, Result<(MPathElement, HgAugmentedManifestEntry)>> {
        self.subentries
            .into_entries(ctx, blobstore)
            .and_then(|(k, v)| async move { anyhow::Ok((MPathElement::from_smallvec(k)?, v)) })
            .boxed()
    }

    pub fn into_subentries_skip<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        skip: usize,
    ) -> BoxStream<'a, Result<(MPathElement, HgAugmentedManifestEntry)>> {
        self.subentries
            .into_entries_skip(ctx, blobstore, skip)
            .and_then(|(k, v)| async move { anyhow::Ok((MPathElement::from_smallvec(k)?, v)) })
            .boxed()
    }

    pub fn into_prefix_subentries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        prefix: &'a [u8],
    ) -> BoxStream<'a, Result<(MPathElement, HgAugmentedManifestEntry)>> {
        self.subentries
            .into_prefix_entries(ctx, blobstore, prefix.as_ref())
            .and_then(|(k, v)| async move { anyhow::Ok((MPathElement::from_smallvec(k)?, v)) })
            .boxed()
    }

    pub fn into_prefix_subentries_after<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        prefix: &'a [u8],
        after: &'a [u8],
    ) -> BoxStream<'a, Result<(MPathElement, HgAugmentedManifestEntry)>> {
        self.subentries
            .into_prefix_entries_after(ctx, blobstore, prefix, after)
            .map(|res| res.and_then(|(k, v)| anyhow::Ok((MPathElement::from_smallvec(k)?, v))))
            .boxed()
    }

    #[inline]
    pub fn hg_node_id(&self) -> HgNodeHash {
        self.hg_node_id
    }

    #[inline]
    pub fn p1(&self) -> Option<HgNodeHash> {
        self.p1
    }

    #[inline]
    pub fn p2(&self) -> Option<HgNodeHash> {
        self.p2
    }

    #[inline]
    pub fn hg_parents(&self) -> HgParents {
        HgParents::new(self.p1, self.p2)
    }

    #[inline]
    pub fn computed_node_id(&self) -> HgNodeHash {
        self.computed_node_id
    }

    // The format of the content addressed manifest blob is as follows:
    //
    // entry ::= <path> '\0' <hg-node-hex> <type> ' ' <entry-value> '\n'
    //
    // entry-value ::= <cas-blake3-hex> ' ' <size-dec> ' ' <sha1-hex> ' ' <base64(file_header_metadata) (if present) or '-'>
    //               | <cas-blake3-hex> ' ' <size-dec>
    //
    // tree ::= <version> ' ' <sha1-hex> ' ' <computed_sha1-hex (if different) or '-'> ' ' <p1-hex or '-'> ' ' <p2-hex or '-'> '\n' <entry>*

    fn serialize_content_addressed_prefix(&self) -> Result<Bytes> {
        let mut buf = Vec::with_capacity(41 * 4); // 40 for a hex hash and a separator
        self.write_content_addressed_prefix(&mut buf)?;
        Ok(buf.into())
    }

    fn serialize_content_addressed_entries(
        entries: Vec<(MPathElement, HgAugmentedManifestEntry)>,
    ) -> Result<Bytes> {
        let mut buf: Vec<u8> = Vec::with_capacity(entries.len() * 100);
        Self::write_content_addressed_entries(
            entries
                .iter()
                .map(|(path, augmented_manifest_entry)| (path, augmented_manifest_entry)),
            &mut buf,
        )?;
        Ok(buf.into())
    }

    fn write_content_addressed_entries<'a>(
        entries: impl Iterator<Item = (&'a MPathElement, &'a HgAugmentedManifestEntry)>,
        mut w: impl Write,
    ) -> Result<()> {
        for (path, augmented_manifest_entry) in entries {
            Self::write_content_addressed_entry(path, augmented_manifest_entry, &mut w)?;
        }
        Ok(())
    }

    fn write_content_addressed_prefix(&self, mut w: impl Write) -> Result<()> {
        w.write_all(b"v1")?; // version number for this format
        w.write_all(b" ")?;
        w.write_all(self.hg_node_id.to_hex().as_bytes())?;
        w.write_all(b" ")?;
        if self.hg_node_id != self.computed_node_id {
            w.write_all(self.computed_node_id.to_hex().as_bytes())?;
        } else {
            w.write_all(b"-")?;
        }
        w.write_all(b" ")?;
        if let Some(p1) = self.p1 {
            w.write_all(p1.to_hex().as_bytes())?;
        } else {
            w.write_all(b"-")?;
        }
        w.write_all(b" ")?;
        if let Some(p2) = self.p2 {
            w.write_all(p2.to_hex().as_bytes())?;
        } else {
            w.write_all(b"-")?;
        }
        w.write_all(b"\n")?;
        Ok(())
    }

    fn write_content_addressed_entry(
        path: &MPathElement,
        augmented_manifest_entry: &HgAugmentedManifestEntry,
        mut w: impl Write,
    ) -> Result<()> {
        w.write_all(path.as_ref())?;
        let (tag, sapling_hash) = match augmented_manifest_entry {
            HgAugmentedManifestEntry::DirectoryNode(ref directory) => {
                (Type::Tree.augmented_manifest_suffix()?, directory.treenode)
            }
            HgAugmentedManifestEntry::FileNode(ref file) => {
                let tag = Type::File(file.file_type).augmented_manifest_suffix()?;
                (tag, file.filenode)
            }
        };
        w.write_all(b"\0")?;
        w.write_all(sapling_hash.to_hex().as_bytes())?;
        w.write_all(tag)?;
        w.write_all(b" ")?;
        Self::write_content_addressed_entry_value(augmented_manifest_entry, &mut w)?;
        w.write_all(b"\n")?;
        Ok(())
    }

    fn write_content_addressed_entry_value(
        augmented_manifest_entry: &HgAugmentedManifestEntry,
        mut w: impl Write,
    ) -> Result<()> {
        // Representation of content addressed Digest.
        let (id, size) = match augmented_manifest_entry {
            HgAugmentedManifestEntry::DirectoryNode(ref directory) => (
                directory.augmented_manifest_id,
                directory.augmented_manifest_size,
            ),
            HgAugmentedManifestEntry::FileNode(ref file) => (file.content_blake3, file.total_size),
        };
        w.write_all(id.to_hex().as_bytes())?;
        w.write_all(b" ")?;
        w.write_all(size.to_string().as_bytes())?;
        if let HgAugmentedManifestEntry::FileNode(ref file) = augmented_manifest_entry {
            w.write_all(b" ")?;
            w.write_all(file.content_sha1.to_hex().as_bytes())?;
            w.write_all(b" ")?;
            if let Some(file_header_metadata) = &file.file_header_metadata {
                w.write_all(
                    base64::engine::general_purpose::STANDARD
                        .encode(file_header_metadata)
                        .as_ref(),
                )?;
            } else {
                w.write_all(b"-")?;
            }
        }
        Ok(())
    }

    pub fn into_content_addressed_manifest_blob<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
    ) -> BoxStream<'a, Result<Bytes>> {
        let prefix_bytes = self.serialize_content_addressed_prefix();
        stream::once(std::future::ready(prefix_bytes))
            .chain(
                self.subentries
                    .into_entries(ctx, blobstore)
                    .map(|res| {
                        res.and_then(|(k, v)| anyhow::Ok((MPathElement::from_smallvec(k)?, v)))
                    })
                    .yield_periodically()
                    .chunks(MAX_BUFFERED_ENTRIES)
                    .map(|results| {
                        results
                            .into_iter()
                            .collect::<Result<Vec<_>, _>>()
                            .and_then(Self::serialize_content_addressed_entries)
                    }),
            )
            .boxed()
    }

    pub async fn compute_content_addressed_digest(
        self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
    ) -> Result<(Blake3, u64)> {
        let mut calculator = AugmentedManifestDigestCalculator::new();
        self.write_content_addressed_prefix(&mut calculator)?;
        self.subentries
            .into_entries(ctx, blobstore)
            .and_then(
                |(path, entry)| async move { Ok((MPathElement::from_smallvec(path)?, entry)) },
            )
            .yield_periodically()
            .try_for_each(|(path, entry)| {
                future::ready(Self::write_content_addressed_entry(
                    &path,
                    &entry,
                    &mut calculator,
                ))
            })
            .await?;
        calculator.finalize()
    }
}

struct AugmentedManifestDigestCalculator {
    hasher: Blake3Hasher,
    size: u64,
}

impl AugmentedManifestDigestCalculator {
    fn new() -> Self {
        #[cfg(fbcode_build)]
        let key = blake3_constants::BLAKE3_HASH_KEY;
        #[cfg(not(fbcode_build))]
        let key = b"20220728-2357111317192329313741#";
        Self {
            hasher: Blake3Hasher::new_keyed(key),
            size: 0,
        }
    }

    fn finalize(self) -> Result<(Blake3, u64)> {
        let hash = Blake3::from_bytes(self.hasher.finalize().as_bytes())?;
        Ok((hash, self.size))
    }
}

impl Write for AugmentedManifestDigestCalculator {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.hasher.update(buf);
        self.size += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl HgAugmentedManifestEntry {
    pub fn rollup_count(&self) -> ShardedHgAugmentedManifestRollupCount {
        ShardedHgAugmentedManifestRollupCount(1)
    }
}

impl ThriftConvert for HgAugmentedFileLeafNode {
    const NAME: &'static str = "HgAugmentedFileLeafNode";
    type Thrift = thrift::HgAugmentedFileLeaf;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        Ok(Self {
            file_type: FileType::from_thrift(t.file_type)?,
            filenode: HgNodeHash::from_thrift(t.filenode)?,
            content_blake3: Blake3::from_thrift(t.content_blake3)?,
            content_sha1: Sha1::from_bytes(t.content_sha1.0)?,
            total_size: t.total_size as u64,
            file_header_metadata: t.file_header_metadata.map(Bytes::from),
        })
    }

    fn into_thrift(self) -> Self::Thrift {
        Self::Thrift {
            file_type: self.file_type.into_thrift(),
            filenode: self.filenode.into_thrift(),
            content_blake3: self.content_blake3.into_thrift(),
            content_sha1: self.content_sha1.into_thrift(),
            total_size: self.total_size as i64,
            file_header_metadata: self.file_header_metadata.map(Bytes::into),
        }
    }
}

impl ThriftConvert for HgAugmentedDirectoryNode {
    const NAME: &'static str = "HgAugmentedDirectoryNode";
    type Thrift = thrift::HgAugmentedDirectoryNode;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        Ok(Self {
            treenode: HgNodeHash::from_thrift(t.treenode)?,
            augmented_manifest_id: Blake3::from_thrift(t.augmented_manifest_id)?,
            augmented_manifest_size: t.augmented_manifest_size as u64,
        })
    }

    fn into_thrift(self) -> Self::Thrift {
        Self::Thrift {
            treenode: self.treenode.into_thrift(),
            augmented_manifest_id: self.augmented_manifest_id.into_thrift(),
            augmented_manifest_size: self.augmented_manifest_size as i64,
        }
    }
}

impl ThriftConvert for HgAugmentedManifestEntry {
    const NAME: &'static str = "HgAugmentedManifestEntry";
    type Thrift = thrift::HgAugmentedManifestEntry;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        match t {
            Self::Thrift::file(file) => Ok(Self::FileNode(ThriftConvert::from_thrift(file)?)),
            Self::Thrift::directory(directory) => {
                Ok(Self::DirectoryNode(ThriftConvert::from_thrift(directory)?))
            }
            _ => Err(anyhow::anyhow!("Unknown HgAugmentedManifestEntry variant")),
        }
    }

    fn into_thrift(self) -> Self::Thrift {
        match self {
            Self::FileNode(file) => Self::Thrift::file(file.into_thrift()),
            Self::DirectoryNode(directory) => Self::Thrift::directory(directory.into_thrift()),
        }
    }
}

impl ThriftConvert for ShardedHgAugmentedManifest {
    const NAME: &'static str = "ShardedHgAugmentedManifest";
    type Thrift = thrift::HgAugmentedManifest;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        Ok(Self {
            hg_node_id: HgNodeHash::from_thrift(t.hg_node_id)?,
            p1: HgNodeHash::from_thrift_opt(t.p1)?,
            p2: HgNodeHash::from_thrift_opt(t.p2)?,
            computed_node_id: HgNodeHash::from_thrift(t.computed_node_id)?,
            subentries: ShardedMapV2Node::from_thrift(t.subentries)?,
        })
    }

    fn into_thrift(self) -> Self::Thrift {
        Self::Thrift {
            hg_node_id: self.hg_node_id.into_thrift(),
            p1: self.p1.map(HgNodeHash::into_thrift),
            p2: self.p2.map(HgNodeHash::into_thrift),
            computed_node_id: self.computed_node_id.into_thrift(),
            subentries: self.subentries.into_thrift(),
        }
    }
}

impl ThriftConvert for HgAugmentedManifestEnvelope {
    const NAME: &'static str = "HgAugmentedManifestEnvelope";
    type Thrift = thrift::HgAugmentedManifestEnvelope;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        Ok(Self {
            augmented_manifest_id: Blake3::from_thrift(t.augmented_manifest_id)?,
            augmented_manifest_size: t.augmented_manifest_size as u64,
            augmented_manifest: ShardedHgAugmentedManifest::from_thrift(t.augmented_manifest)?,
        })
    }

    fn into_thrift(self) -> Self::Thrift {
        Self::Thrift {
            augmented_manifest_id: self.augmented_manifest_id.into_thrift(),
            augmented_manifest_size: self.augmented_manifest_size as i64,
            augmented_manifest: self.augmented_manifest.into_thrift(),
        }
    }
}

impl ThriftConvert for ShardedHgAugmentedManifestRollupCount {
    const NAME: &'static str = "ShardedHgAugmentedManifestRollupCount";
    type Thrift = i64;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        Ok(Self(t as u64))
    }

    fn into_thrift(self) -> Self::Thrift {
        self.0 as i64
    }
}

impl_typed_hash! {
    hash_type => ShardedMapV2NodeHgAugmentedManifestId,
    thrift_hash_type => mononoke_types::thrift::id::ShardedMapV2NodeId,
    value_type => ShardedMapV2Node<HgAugmentedManifestEntry>,
    context_type => ShardedMapV2NodeHgAugmentedManifestContext,
    context_key => "hgaugmentedmanifest.map2node",
}

impl ShardedMapV2Value for HgAugmentedManifestEntry {
    type NodeId = ShardedMapV2NodeHgAugmentedManifestId;
    type Context = ShardedMapV2NodeHgAugmentedManifestContext;
    type RollupData = ShardedHgAugmentedManifestRollupCount;

    const WEIGHT_LIMIT: usize = 2000;
}

impl Rollup<HgAugmentedManifestEntry> for ShardedHgAugmentedManifestRollupCount {
    fn rollup(value: Option<&HgAugmentedManifestEntry>, child_rollup_data: Vec<Self>) -> Self {
        child_rollup_data.into_iter().fold(
            value.map_or(ShardedHgAugmentedManifestRollupCount(0), |value| {
                value.rollup_count()
            }),
            |acc, child| ShardedHgAugmentedManifestRollupCount(acc.0 + child.0),
        )
    }
}

impl HgAugmentedManifestEnvelope {
    pub fn from_blob(blob: Bytes) -> Result<Self> {
        let thrift_tc = fbthrift::compact_protocol::deserialize(blob).with_context(|| {
            MononokeHgError::BlobDeserializeError("HgAugmentedManifestEnvelope".into())
        })?;
        Self::from_thrift(thrift_tc)
    }

    pub async fn load<'a, B: Blobstore>(
        ctx: &CoreContext,
        blobstore: &B,
        manifestid: HgAugmentedManifestId,
    ) -> Result<Option<Self>> {
        async {
            let blobstore_key = manifestid.blobstore_key();
            let bytes = blobstore
                .get(ctx, &blobstore_key)
                .await
                .context("While fetching aurmented manifest envelope blob")?;
            (|| {
                let envelope = match bytes {
                    Some(bytes) => Self::from_blob(bytes.into_raw_bytes())?,
                    None => return Ok(None),
                };
                if manifestid.into_nodehash() != envelope.augmented_manifest.hg_node_id() {
                    bail!(
                        "Augmented Manifest ID mismatch (requested: {}, got: {})",
                        manifestid,
                        envelope.augmented_manifest.hg_node_id()
                    );
                }
                Ok(Some(envelope))
            })()
            .context(MononokeHgBlobError::ManifestDeserializeFailed(
                blobstore_key,
            ))
        }
        .await
        .context(format!(
            "Failed to load manifest {} from blobstore",
            manifestid
        ))
    }

    /// Serialize this structure into bytes
    #[inline]
    pub fn into_blob(self) -> Bytes {
        let thrift = self.into_thrift();
        fbthrift::compact_protocol::serialize(&thrift)
    }

    pub fn augmented_manifest(&self) -> &ShardedHgAugmentedManifest {
        &self.augmented_manifest
    }

    /// The next 3 functions are used to generate the content addressed manifest blob to store in content addressed store.

    pub fn augmented_manifest_id(&self) -> Blake3 {
        self.augmented_manifest_id
    }

    pub fn augmented_manifest_size(&self) -> u64 {
        self.augmented_manifest_size
    }

    pub fn into_content_addressed_manifest_blob<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
    ) -> BoxStream<'a, Result<Bytes>> {
        self.augmented_manifest
            .into_content_addressed_manifest_blob(ctx, blobstore)
    }
}

pub async fn fetch_augmented_manifest_envelope_opt<B: Blobstore>(
    ctx: &CoreContext,
    blobstore: &B,
    augmented_node_id: HgAugmentedManifestId,
) -> Result<Option<HgAugmentedManifestEnvelope>> {
    if augmented_node_id == HgAugmentedManifestId::new(NULL_HASH) {
        return Ok(None);
    }
    let blobstore_key = augmented_node_id.blobstore_key();
    let bytes = blobstore
        .get(ctx, &blobstore_key)
        .await
        .context("While fetching augmented manifest envelope blob")?;
    let blobstore_bytes = match bytes {
        Some(bytes) => bytes,
        None => return Ok(None),
    };
    let envelope = HgAugmentedManifestEnvelope::from_blob(blobstore_bytes.into_raw_bytes())?;
    if augmented_node_id.into_nodehash() != envelope.augmented_manifest.hg_node_id() {
        bail!(
            "Manifest ID mismatch (requested: {}, got: {})",
            augmented_node_id,
            envelope.augmented_manifest.hg_node_id()
        );
    }
    Ok(Some(envelope))
}

#[async_trait]
impl Loadable for HgAugmentedManifestId {
    type Value = HgAugmentedManifestEnvelope;

    async fn load<'a, B: Blobstore>(
        &'a self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Value, LoadableError> {
        let id = *self;
        HgAugmentedManifestEnvelope::load(ctx, blobstore, id)
            .await?
            .ok_or_else(|| LoadableError::Missing(id.blobstore_key()))
    }
}

#[async_trait]
impl Storable for HgAugmentedManifestEnvelope {
    type Key = HgAugmentedManifestId;

    async fn store<'a, B: Blobstore>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Key> {
        let key = HgAugmentedManifestId::new(self.augmented_manifest.hg_node_id);
        let blob = BlobstoreBytes::from_bytes(self.into_blob());
        blobstore
            .put(ctx, key.blobstore_key(), blob)
            .await
            .context("Failed to store augmented manifest")?;
        Ok(key)
    }
}

fn convert_hg_augmented_manifest_entry(
    entry: HgAugmentedManifestEntry,
) -> Entry<HgAugmentedManifestId, HgAugmentedFileLeafNode> {
    match entry {
        HgAugmentedManifestEntry::FileNode(file) => Entry::Leaf(file),
        HgAugmentedManifestEntry::DirectoryNode(directory) => {
            Entry::Tree(HgAugmentedManifestId::new(directory.treenode))
        }
    }
}

#[async_trait]
impl<Store: Blobstore> Manifest<Store> for HgAugmentedManifestEnvelope {
    type TreeId = HgAugmentedManifestId;

    type Leaf = HgAugmentedFileLeafNode;

    type TrieMapType = LoadableShardedMapV2Node<HgAugmentedManifestEntry>;

    async fn list(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        anyhow::Ok(
            self.augmented_manifest
                .clone()
                .into_subentries(ctx, blobstore)
                .map_ok(|(path, entry)| (path, convert_hg_augmented_manifest_entry(entry)))
                .boxed(),
        )
    }

    async fn list_prefix(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        prefix: &[u8],
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        anyhow::Ok(
            self.augmented_manifest
                .clone()
                .into_prefix_subentries(ctx, blobstore, prefix)
                .map_ok(|(path, entry)| (path, convert_hg_augmented_manifest_entry(entry)))
                .boxed(),
        )
    }

    async fn list_prefix_after(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        prefix: &[u8],
        after: &[u8],
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        anyhow::Ok(
            self.augmented_manifest
                .clone()
                .into_prefix_subentries_after(ctx, blobstore, prefix, after)
                .map_ok(|(path, entry)| (path, convert_hg_augmented_manifest_entry(entry)))
                .boxed(),
        )
    }

    async fn list_skip(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        skip: usize,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        anyhow::Ok(
            self.augmented_manifest
                .clone()
                .into_subentries_skip(ctx, blobstore, skip)
                .map_ok(|(path, entry)| (path, convert_hg_augmented_manifest_entry(entry)))
                .boxed(),
        )
    }

    async fn lookup(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        name: &MPathElement,
    ) -> Result<Option<Entry<Self::TreeId, Self::Leaf>>> {
        Ok(self
            .augmented_manifest
            .lookup(ctx, blobstore, name)
            .await?
            .map(convert_hg_augmented_manifest_entry))
    }

    async fn into_trie_map(
        self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<Self::TrieMapType> {
        Ok(LoadableShardedMapV2Node::Inlined(
            self.augmented_manifest.subentries,
        ))
    }
}

#[cfg(test)]
mod sharded_augmented_manifest_tests {
    use std::io::Cursor;

    use bonsai_hg_mapping::BonsaiHgMapping;
    use bookmarks::Bookmarks;
    use bytes::BytesMut;
    use commit_graph::CommitGraph;
    use commit_graph::CommitGraphWriter;
    use fbinit::FacebookInit;
    use filestore::FilestoreConfig;
    use fixtures::Linear;
    use fixtures::TestRepoFixture;
    use mononoke_macros::mononoke;
    use repo_blobstore::RepoBlobstore;
    use repo_blobstore::RepoBlobstoreArc;
    use repo_derived_data::RepoDerivedData;
    use repo_identity::RepoIdentity;
    use types::AugmentedTree;

    use super::*;

    #[facet::container]
    #[derive(Clone)]
    struct TestRepo(
        dyn BonsaiHgMapping,
        dyn Bookmarks,
        RepoBlobstore,
        RepoDerivedData,
        RepoIdentity,
        CommitGraph,
        dyn CommitGraphWriter,
        FilestoreConfig,
    );

    fn hash_ones() -> HgNodeHash {
        HgNodeHash::new("1111111111111111111111111111111111111111".parse().unwrap())
    }

    fn hash_twos() -> HgNodeHash {
        HgNodeHash::new("2222222222222222222222222222222222222222".parse().unwrap())
    }

    fn hash_threes() -> HgNodeHash {
        HgNodeHash::new("3333333333333333333333333333333333333333".parse().unwrap())
    }

    fn hash_fours() -> HgNodeHash {
        HgNodeHash::new("4444444444444444444444444444444444444444".parse().unwrap())
    }

    fn blake3_ones() -> Blake3 {
        Blake3::from_byte_array([0x11; 32])
    }

    fn blake3_twos() -> Blake3 {
        Blake3::from_byte_array([0x22; 32])
    }

    fn blake3_threes() -> Blake3 {
        Blake3::from_byte_array([0x33; 32])
    }

    fn blake3_fours() -> Blake3 {
        Blake3::from_byte_array([0x44; 32])
    }

    #[allow(dead_code)]
    fn sha1_ones() -> Sha1 {
        Sha1::from_byte_array([0x11; 20])
    }

    fn sha1_twos() -> Sha1 {
        Sha1::from_byte_array([0x21; 20])
    }

    #[allow(dead_code)]
    fn sha1_three() -> Sha1 {
        Sha1::from_byte_array([0x33; 20])
    }

    fn sha1_fours() -> Sha1 {
        Sha1::from_byte_array([0x44; 20])
    }

    #[mononoke::fbinit_test]
    async fn test_serialize_augmented_manifest(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let blobrepo: TestRepo = Linear::get_repo(fb).await;
        let blobstore = blobrepo.repo_blobstore_arc();

        let subentries = vec![
            (
                MPathElement::new_from_slice(b"a.rs")?,
                HgAugmentedManifestEntry::FileNode(HgAugmentedFileLeafNode {
                    file_type: FileType::Regular,
                    filenode: hash_fours(),
                    content_blake3: blake3_fours(),
                    content_sha1: sha1_fours(),
                    total_size: 10,
                    file_header_metadata: Some(Bytes::from(
                        "\x01\ncopy: fbcode/eden/scm/lib/revisionstore/TARGETS\ncopyrev: a459504f676a5fec5ab3d1a14f4616430391c03e\n\x01\n",
                    )),
                }),
            ),
            (
                MPathElement::new_from_slice(b"b.rs")?,
                HgAugmentedManifestEntry::FileNode(HgAugmentedFileLeafNode {
                    file_type: FileType::Regular,
                    filenode: hash_twos(),
                    content_blake3: blake3_twos(),
                    content_sha1: sha1_twos(),
                    total_size: 1000,
                    file_header_metadata: None,
                }),
            ),
            (
                MPathElement::new_from_slice(b"dir_1")?,
                HgAugmentedManifestEntry::DirectoryNode(HgAugmentedDirectoryNode {
                    treenode: hash_threes(),
                    augmented_manifest_id: blake3_threes(),
                    augmented_manifest_size: 10,
                }),
            ),
            (
                MPathElement::new_from_slice(b"dir_2")?,
                HgAugmentedManifestEntry::DirectoryNode(HgAugmentedDirectoryNode {
                    treenode: hash_ones(),
                    augmented_manifest_id: blake3_ones(),
                    augmented_manifest_size: 10000,
                }),
            ),
        ];

        let augmented_manifest = ShardedHgAugmentedManifest {
            hg_node_id: hash_ones(),
            p1: Some(hash_twos()),
            p2: Some(hash_threes()),
            computed_node_id: hash_ones(),
            subentries: ShardedMapV2Node::from_entries(&ctx, &blobstore, subentries).await?,
        };

        let bytes = augmented_manifest
            .into_content_addressed_manifest_blob(&ctx, &blobstore)
            .map(|b| b.unwrap())
            .collect::<BytesMut>()
            .await;

        assert_eq!(
            bytes,
            Bytes::from(concat!(
                "v1 1111111111111111111111111111111111111111 - 2222222222222222222222222222222222222222 3333333333333333333333333333333333333333\n",
                "a.rs\x004444444444444444444444444444444444444444r 4444444444444444444444444444444444444444444444444444444444444444 10 4444444444444444444444444444444444444444 AQpjb3B5OiBmYmNvZGUvZWRlbi9zY20vbGliL3JldmlzaW9uc3RvcmUvVEFSR0VUUwpjb3B5cmV2OiBhNDU5NTA0ZjY3NmE1ZmVjNWFiM2QxYTE0ZjQ2MTY0MzAzOTFjMDNlCgEK\n",
                "b.rs\x002222222222222222222222222222222222222222r 2222222222222222222222222222222222222222222222222222222222222222 1000 2121212121212121212121212121212121212121 -\n",
                "dir_1\x003333333333333333333333333333333333333333t 3333333333333333333333333333333333333333333333333333333333333333 10\n",
                "dir_2\x001111111111111111111111111111111111111111t 1111111111111111111111111111111111111111111111111111111111111111 10000\n"
            ))
        );

        // Check compatibility with the Sapling Type, to make sure Sapling can deserialize
        assert!(AugmentedTree::try_deserialize(Cursor::new(bytes)).is_ok());

        Ok(())
    }
}
