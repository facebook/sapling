/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::str::FromStr;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::LoadableError;
use blobstore::Storable;
use bytes::Bytes;
use context::CoreContext;
use fbthrift::compact_protocol;
use futures::stream::BoxStream;
use futures::StreamExt;
use futures::TryStreamExt;
use gix_hash::oid;
use gix_hash::ObjectId;
use mononoke_types::hash::Blake2;
use mononoke_types::hash::GitSha1;
use mononoke_types::hash::RichGitSha1;
use mononoke_types::impl_typed_hash;
use mononoke_types::impl_typed_hash_loadable;
use mononoke_types::impl_typed_hash_no_context;
use mononoke_types::path::MPath;
use mononoke_types::sharded_map::MapValue;
use mononoke_types::sharded_map::ShardedMapNode;
use mononoke_types::thrift as mononoke_types_thrift;
use mononoke_types::Blob;
use mononoke_types::BlobstoreKey;
use mononoke_types::BlobstoreValue;
use mononoke_types::ChangesetId;
use mononoke_types::MononokeId;
use mononoke_types::ThriftConvert;
use quickcheck::Arbitrary;

use crate::store::StoredInstructionsMetadata;
use crate::thrift;

/// An identifier for a sharded map node used in git delta manifest
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct ShardedMapNodeGitDeltaManifestId(Blake2);

impl_typed_hash! {
    hash_type => ShardedMapNodeGitDeltaManifestId,
    thrift_hash_type => mononoke_types_thrift::ShardedMapNodeId,
    value_type => ShardedMapNode<GitDeltaManifestEntry>,
    context_type => ShardedMapNodeGitDeltaManifestContext,
    context_key => "git_delta_manifest.mapnode",
}

/// An identifier for mapping from ChangesetId to GitDeltaManifest
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct GitDeltaManifestId(Blake2);

impl_typed_hash_no_context! {
    hash_type => GitDeltaManifestId,
    thrift_type => thrift::GitDeltaManifestId,
    blobstore_key => "git_delta_manifest",
}

impl_typed_hash_loadable! {
    hash_type => GitDeltaManifestId,
    value_type => GitDeltaManifest,
}

impl From<ChangesetId> for GitDeltaManifestId {
    fn from(changeset_id: ChangesetId) -> Self {
        Self(changeset_id.blake2().to_owned())
    }
}

impl MononokeId for GitDeltaManifestId {
    #[inline]
    fn sampling_fingerprint(&self) -> u64 {
        self.0.sampling_fingerprint()
    }
}

/// Manifest that contains an entry for each Git object that was added or modified as part of
/// a commit.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct GitDeltaManifest {
    pub commit: ChangesetId,
    pub entries: ShardedMapNode<GitDeltaManifestEntry>,
}

impl GitDeltaManifest {
    pub fn new(commit: ChangesetId) -> Self {
        Self {
            commit,
            entries: ShardedMapNode::default(),
        }
    }

    pub async fn add_entries(
        &mut self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        new_entries: BTreeMap<MPath, GitDeltaManifestEntry>,
    ) -> Result<()> {
        let entries = std::mem::take(&mut self.entries);
        let new_entries = new_entries
            .into_iter()
            .map(|(path, entry)| {
                // Convert the MPath into Vec<u8> by merging MPathElements with null byte as the separator. We use the null-separated
                // path as the key in the ShardedMap to allow for proper ordering of paths.
                (path.to_null_separated_bytes().into(), Some(entry))
            })
            .collect::<BTreeMap<Bytes, _>>();
        self.entries = entries.update(ctx, blobstore, new_entries, |_| ()).await?;
        Ok(())
    }

    pub fn into_subentries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
    ) -> BoxStream<'a, Result<(MPath, GitDeltaManifestEntry)>> {
        self.entries
            .into_entries(ctx, blobstore)
            .and_then(|(k, v)| async move {
                let path = MPath::from_null_separated_bytes(k.to_vec())?;
                anyhow::Ok((path, v))
            })
            .boxed()
    }

    pub fn into_filtered_subentries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        filter: fn(&MPath, &GitDeltaManifestEntry) -> bool,
    ) -> BoxStream<'a, Result<(MPath, GitDeltaManifestEntry)>> {
        self.entries
            .into_entries(ctx, blobstore)
            .try_filter_map(move |(k, v)| async move {
                let path = MPath::from_null_separated_bytes(k.to_vec())?;
                if filter(&path, &v) {
                    anyhow::Ok(Some((path, v)))
                } else {
                    anyhow::Ok(None)
                }
            })
            .boxed()
    }

    pub fn into_prefix_subentries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        prefix: &'a [u8],
    ) -> BoxStream<'a, Result<(MPath, GitDeltaManifestEntry)>> {
        self.entries
            .into_prefix_entries(ctx, blobstore, prefix)
            .map(|res| {
                res.and_then(|(k, v)| {
                    let path = MPath::from_null_separated_bytes(k.to_vec())?;
                    anyhow::Ok((path, v))
                })
            })
            .boxed()
    }

    pub async fn lookup(
        &self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        name: &MPath,
    ) -> Result<Option<GitDeltaManifestEntry>> {
        let path = name.to_null_separated_bytes();
        self.entries.lookup(ctx, blobstore, path.as_ref()).await
    }
}

impl MapValue for GitDeltaManifestEntry {
    type Id = ShardedMapNodeGitDeltaManifestId;
    type Context = ShardedMapNodeGitDeltaManifestContext;
}

impl TryFrom<thrift::GitDeltaManifest> for GitDeltaManifest {
    type Error = Error;

    fn try_from(value: thrift::GitDeltaManifest) -> Result<Self, Self::Error> {
        let commit = ChangesetId::from_thrift(value.commit)?;
        let entries = ShardedMapNode::from_thrift(value.entries)?;
        Ok(GitDeltaManifest { commit, entries })
    }
}

impl From<GitDeltaManifest> for thrift::GitDeltaManifest {
    fn from(value: GitDeltaManifest) -> Self {
        let commit = ChangesetId::into_thrift(value.commit);
        let entries = value.entries.into_thrift();
        thrift::GitDeltaManifest { commit, entries }
    }
}

impl ThriftConvert for GitDeltaManifest {
    const NAME: &'static str = "GitDeltaManifest";
    type Thrift = thrift::GitDeltaManifest;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        t.try_into()
    }

    fn into_thrift(self) -> Self::Thrift {
        self.into()
    }
}

pub type GitDeltaManifestBlob = Blob<GitDeltaManifestId>;

impl BlobstoreValue for GitDeltaManifest {
    type Key = GitDeltaManifestId;

    fn into_blob(self) -> GitDeltaManifestBlob {
        let id: Self::Key = self.commit.clone().into();
        let data = self.into_bytes();
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        Self::from_bytes(blob.data())
    }
}

#[async_trait::async_trait]
impl Storable for GitDeltaManifest {
    type Key = GitDeltaManifestId;

    async fn store<'a, B: Blobstore>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Key> {
        let blob = self.into_blob();
        let id = blob.id().clone();
        blobstore.put(ctx, id.blobstore_key(), blob.into()).await?;
        Ok(id)
    }
}

/// Represents a single entry in the GitDeltaManifest corresponding to a Git object.
/// Contains reference to the full version of the object along with all potential delta entries.
/// The delta variants would be absent if the object is introduced for the first time or if the
/// object is too large (or of unsupported type) to be represented as a delta
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct GitDeltaManifestEntry {
    /// The full version of the Git object
    pub full: ObjectEntry,
    /// The delta variant of the Git object against all possible base objects
    pub deltas: Vec<ObjectDelta>,
}

impl GitDeltaManifestEntry {
    pub fn new(full: ObjectEntry, deltas: Vec<ObjectDelta>) -> Self {
        Self { full, deltas }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let thrift_tc: thrift::GitDeltaManifestEntry = compact_protocol::deserialize(bytes)
            .context("Failure in deserializing bytes to GitDeltaManifestEntry")?;
        thrift_tc
            .try_into()
            .context("Failure in converting Thrift data to GitDeltaManifestEntry")
    }

    pub fn is_tree(&self) -> bool {
        match self.full.kind {
            ObjectKind::Blob => false,
            ObjectKind::Tree => true,
        }
    }

    pub fn is_delta(&self) -> bool {
        !self.deltas.is_empty()
    }
}

impl TryFrom<thrift::GitDeltaManifestEntry> for GitDeltaManifestEntry {
    type Error = Error;

    fn try_from(value: thrift::GitDeltaManifestEntry) -> Result<Self, Self::Error> {
        let full = value.full.try_into()?;
        let deltas = value
            .deltas
            .into_iter()
            .map(|d| d.try_into())
            .collect::<Result<Vec<_>>>()?;
        Ok(GitDeltaManifestEntry { full, deltas })
    }
}

impl From<GitDeltaManifestEntry> for thrift::GitDeltaManifestEntry {
    fn from(value: GitDeltaManifestEntry) -> Self {
        let full = value.full.into();
        let deltas = value.deltas.into_iter().map(|d| d.into()).collect();
        thrift::GitDeltaManifestEntry { full, deltas }
    }
}

impl ThriftConvert for GitDeltaManifestEntry {
    const NAME: &'static str = "GitDeltaManifestEntry";
    type Thrift = thrift::GitDeltaManifestEntry;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        t.try_into()
    }

    fn into_thrift(self) -> Self::Thrift {
        self.into()
    }
}

impl Arbitrary for GitDeltaManifestEntry {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        let full = ObjectEntry::arbitrary(g);
        let deltas = Vec::arbitrary(g);
        GitDeltaManifestEntry { full, deltas }
    }
}

/// Represents the delta for a single Git object
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ObjectDelta {
    /// The commit that originally introduced this Git object
    pub origin: ChangesetId,
    /// The base Git object used for creating the delta
    pub base: ObjectEntry,
    /// The Zlib encoded instructions are stored in the blobstore in chunks. This property
    /// reflects the number of chunks stored for these encoded instructions in the blobstore
    pub instructions_chunk_count: u64,
    /// The total size of the raw delta instructions bytes before Zlib compression/encoding
    pub instructions_uncompressed_size: u64,
    /// The total size of the compressed delta instructions bytes after Zlib compression/encoding
    pub instructions_compressed_size: u64,
}

impl ObjectDelta {
    pub fn new(
        origin: ChangesetId,
        base: ObjectEntry,
        metadata: StoredInstructionsMetadata,
    ) -> Self {
        Self {
            origin,
            base,
            instructions_chunk_count: metadata.chunks,
            instructions_uncompressed_size: metadata.uncompressed_bytes,
            instructions_compressed_size: metadata.compressed_bytes,
        }
    }
}

impl TryFrom<thrift::ObjectDelta> for ObjectDelta {
    type Error = Error;

    fn try_from(value: thrift::ObjectDelta) -> Result<Self, Self::Error> {
        let base = value.base.try_into()?;
        let instructions_chunk_count = value.instructions_chunk_count.try_into()?;
        let instructions_uncompressed_size = value.instructions_uncompressed_size.try_into()?;
        let instructions_compressed_size = value.instructions_compressed_size.try_into()?;
        let origin = ChangesetId::from_thrift(value.origin)?;
        Ok(Self {
            base,
            origin,
            instructions_chunk_count,
            instructions_uncompressed_size,
            instructions_compressed_size,
        })
    }
}

impl From<ObjectDelta> for thrift::ObjectDelta {
    fn from(value: ObjectDelta) -> Self {
        let base = value.base.into();
        let instructions_chunk_count = value.instructions_chunk_count as i64;
        let instructions_uncompressed_size = value.instructions_uncompressed_size as i64;
        let instructions_compressed_size = value.instructions_compressed_size as i64;
        let origin = ChangesetId::into_thrift(value.origin);
        Self {
            base,
            origin,
            instructions_chunk_count,
            instructions_uncompressed_size,
            instructions_compressed_size,
        }
    }
}

impl ThriftConvert for ObjectDelta {
    const NAME: &'static str = "ObjectDelta";
    type Thrift = thrift::ObjectDelta;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        t.try_into()
    }

    fn into_thrift(self) -> Self::Thrift {
        self.into()
    }
}

impl Arbitrary for ObjectDelta {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        let base = ObjectEntry::arbitrary(g);
        let origin = ChangesetId::arbitrary(g);
        let instructions_chunk_count = u64::arbitrary(g) / 2;
        let instructions_uncompressed_size = u64::arbitrary(g) / 2;
        let instructions_compressed_size = u64::arbitrary(g) / 2;
        Self {
            base,
            origin,
            instructions_chunk_count,
            instructions_uncompressed_size,
            instructions_compressed_size,
        }
    }
}

/// Metadata information representing a Git object to be used in
/// the GitDeltaManifest
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ObjectEntry {
    /// The Git object ID which is the SHA1 hash of the object content
    pub oid: ObjectId,
    /// The size of the object in bytes
    pub size: u64,
    /// The type of the Git object, only Blob and Tree are supported in GitDeltaManifest
    pub kind: ObjectKind,
    /// The path of the directory or file corresponding to this Git Tree or Blob
    pub path: MPath,
}

impl ObjectEntry {
    pub fn as_rich_git_sha1(&self) -> Result<RichGitSha1> {
        let sha1 = GitSha1::from_bytes(self.oid.as_bytes())?;
        let ty = match self.kind {
            ObjectKind::Blob => "blob",
            ObjectKind::Tree => "tree",
        };
        Ok(RichGitSha1::from_sha1(sha1, ty, self.size))
    }
}

impl TryFrom<thrift::ObjectEntry> for ObjectEntry {
    type Error = Error;

    fn try_from(t: thrift::ObjectEntry) -> Result<Self, Error> {
        let oid = oid::try_from_bytes(&t.oid.0)?.to_owned();
        let size: u64 = t.size.try_into()?;
        let kind = t.kind.try_into()?;
        let path = MPath::from_thrift(t.path)?;
        Ok(Self {
            oid,
            size,
            kind,
            path,
        })
    }
}

impl From<ObjectEntry> for thrift::ObjectEntry {
    fn from(value: ObjectEntry) -> Self {
        let oid = mononoke_types_thrift::GitSha1(value.oid.as_bytes().into());
        let size = value.size as i64;
        let kind = value.kind.into();
        let path = MPath::into_thrift(value.path);
        thrift::ObjectEntry {
            oid,
            size,
            kind,
            path,
        }
    }
}

impl ThriftConvert for ObjectEntry {
    const NAME: &'static str = "ObjectEntry";
    type Thrift = thrift::ObjectEntry;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        t.try_into()
    }

    fn into_thrift(self) -> Self::Thrift {
        self.into()
    }
}

impl Arbitrary for ObjectEntry {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        let oid = oid::try_from_bytes(mononoke_types::hash::Sha1::arbitrary(g).as_ref())
            .unwrap()
            .into();
        let size = u64::arbitrary(g) / 2;
        let kind = ObjectKind::arbitrary(g);
        let path = MPath::arbitrary(g);
        Self {
            oid,
            size,
            kind,
            path,
        }
    }
}

/// Enum representing the types of Git objects that can be present
/// in a GitDeltaManifest
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ObjectKind {
    Blob,
    Tree,
}

impl TryFrom<thrift::ObjectKind> for ObjectKind {
    type Error = Error;

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

impl Arbitrary for ObjectKind {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        match bool::arbitrary(g) {
            true => ObjectKind::Blob,
            false => ObjectKind::Tree,
        }
    }
}

#[cfg(test)]
mod test {
    use quickcheck::quickcheck;

    use super::*;

    quickcheck! {
        fn git_delta_manifest_entry_thrift_roundtrip(entry: GitDeltaManifestEntry) -> bool {
            let thrift_entry: thrift::GitDeltaManifestEntry = entry.clone().into();
            let from_thrift_entry: GitDeltaManifestEntry = thrift_entry.try_into().expect("thrift roundtrips should always be valid");
            println!("entry: {:?}", entry);
            println!("entry_from_thrift: {:?}", from_thrift_entry);
            entry == from_thrift_entry
        }
    }

    #[test]
    fn test_git_delta_manifest_id() {
        let id = GitDeltaManifestId::from_byte_array([1; 32]);
        assert_eq!(
            id.blobstore_key(),
            format!("git_delta_manifest.blake2.{}", id)
        );

        let id = GitDeltaManifestId::from_byte_array([1; 32]);
        let serialized = serde_json::to_string(&id).unwrap();
        let deserialized = serde_json::from_str(&serialized).unwrap();
        assert_eq!(id, deserialized);
    }
}
