/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::fmt::Debug;
use std::fmt::Display;
use std::hash::Hash;
use std::result;
use std::str::FromStr;

use abomonation_derive::Abomonation;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::LoadableError;
use context::CoreContext;
use edenapi_types::BonsaiChangesetId as EdenapiBonsaiChangesetId;
use edenapi_types::ContentId as EdenapiContentId;
use edenapi_types::FsnodeId as EdenapiFsnodeId;
use sql::mysql;

use crate::blob::Blob;
use crate::blob::BlobstoreValue;
use crate::bonsai_changeset::BonsaiChangeset;
use crate::content_chunk::ContentChunk;
use crate::content_metadata::ContentMetadata;
use crate::content_metadata_v2::ContentMetadataV2;
use crate::deleted_manifest_v2::DeletedManifestV2;
use crate::fastlog_batch::FastlogBatch;
use crate::file_contents::FileContents;
use crate::fsnode::Fsnode;
use crate::hash::Blake2;
use crate::hash::Blake2Prefix;
use crate::rawbundle2::RawBundle2;
use crate::redaction_key_list::RedactionKeyList;
use crate::sharded_map::ShardedMapNode;
use crate::skeleton_manifest::SkeletonManifest;
use crate::thrift;
use crate::unode::FileUnode;
use crate::unode::ManifestUnode;
use crate::ThriftConvert;

// There is no NULL_HASH for typed hashes. Any places that need a null hash should use an
// Option type, or perhaps a list as desired.

/// A type, which can be parsed from a blobstore key,
/// and from which a blobstore key can be produced
/// (this is implemented by various handle types, where
/// blobstore key consists of two things: a hash
/// and a string, describing what the key refers to)
pub trait BlobstoreKey: FromStr<Err = anyhow::Error> {
    /// Return a key suitable for blobstore use.
    fn blobstore_key(&self) -> String;
    fn parse_blobstore_key(key: &str) -> Result<Self>;
}

pub trait IdContext {
    type Id;
    fn id_from_data(data: impl AsRef<[u8]>) -> Self::Id;
}

/// An identifier used throughout Mononoke.
pub trait MononokeId:
    BlobstoreKey + Loadable + ThriftConvert + Debug + Copy + Eq + Hash + Sync + Send + 'static
where
    <Self as Loadable>::Value: BlobstoreValue<Key = Self>,
{
    /// Return a stable hash fingerprint that can be used for sampling
    fn sampling_fingerprint(&self) -> u64;
}

/// An identifier for a changeset in Mononoke.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash, Abomonation)]
#[derive(mysql::OptTryFromRowField)]
pub struct ChangesetId(Blake2);

/// An identifier for a changeset hash prefix in Mononoke.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash, Abomonation)]
pub struct ChangesetIdPrefix(Blake2Prefix);

/// The type for resolving changesets by prefix of the hash
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum ChangesetIdsResolvedFromPrefix {
    /// Found single changeset
    Single(ChangesetId),
    /// Found several changesets within the limit provided
    Multiple(Vec<ChangesetId>),
    /// Found too many changesets exceeding the limit provided
    TooMany(Vec<ChangesetId>),
    /// Changeset was not found
    NoMatch,
}

/// An identifier for file contents in Mononoke.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct ContentId(Blake2);

/// An identifier for a chunk of a file's contents.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct ContentChunkId(Blake2);

/// An identifier for mapping from a ContentId to various aliases for that content
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct ContentMetadataId(Blake2);

/// An identifier for mapping from ContentId to ContentMetadata
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct ContentMetadataV2Id(Blake2);

/// An identifier for raw bundle2 contents in Mononoke
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct RawBundle2Id(Blake2);

/// An identifier for a file unode
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct FileUnodeId(Blake2);

/// An identifier for a manifest unode
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct ManifestUnodeId(Blake2);

/// An identifier for a deleted manifest v2
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct DeletedManifestV2Id(Blake2);

/// An identifier for a sharded map node used in deleted manifest v2
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct ShardedMapNodeDMv2Id(Blake2);

/// An identifier for an fsnode
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct FsnodeId(Blake2);

/// An identifier for a skeleton manifest
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct SkeletonManifestId(Blake2);

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct FastlogBatchId(Blake2);

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct BlameId(Blake2);

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct RedactionKeyListId(Blake2);

pub struct Blake2HexVisitor;

impl<'de> serde::de::Visitor<'de> for Blake2HexVisitor {
    type Value = String;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("64 hex digits")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(value.to_string())
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(value)
    }
}

/// Implementations of typed hashes.
#[macro_export]
macro_rules! impl_typed_hash_no_context {
    {
        hash_type => $typed: ty,
        thrift_type => $thrift_typed: path,
        blobstore_key => $blobstore_key: expr,
    } => {
        impl $typed {
            pub const fn new(blake2: $crate::private::Blake2) -> Self {
                Self(blake2)
            }


            #[cfg(test)]
            pub(crate) fn from_byte_array(arr: [u8; 32]) -> Self {
                Self::new($crate::private::Blake2::from_byte_array(arr))
            }

            #[inline]
            pub fn from_bytes(bytes: impl AsRef<[u8]>) -> $crate::private::anyhow::Result<Self> {
                $crate::private::Blake2::from_bytes(bytes).map(Self::new)
            }

            #[inline]
            pub fn from_ascii_str(s: &$crate::private::AsciiStr) -> $crate::private::anyhow::Result<Self> {
                $crate::private::Blake2::from_ascii_str(s).map(Self::new)
            }

            pub fn blake2(&self) -> &$crate::private::Blake2 {
                &self.0
            }

            #[inline]
            pub fn to_hex(&self) -> $crate::private::AsciiString {
                self.0.to_hex()
            }

            pub fn to_brief(&self) -> $crate::private::AsciiString {
                self.to_hex().into_iter().take(8).collect()
            }

            pub fn from_thrift(h: $thrift_typed) -> $crate::private::anyhow::Result<Self> {
                $crate::ThriftConvert::from_thrift(h)
            }

            pub fn into_thrift(self) -> $thrift_typed {
                $crate::ThriftConvert::into_thrift(self)
            }
        }

        impl $crate::ThriftConvert for $typed {
            const NAME: &'static str = stringify!($typed);
            type Thrift = $thrift_typed;

            fn from_thrift(h: Self::Thrift) -> $crate::private::anyhow::Result<Self> {
                // This assumes that a null hash is never serialized. This should always be the
                // case.
                match h.0 {
                    $crate::private::thrift::IdType::Blake2(blake2) => Ok(Self::new($crate::private::Blake2::from_thrift(blake2)?)),
                    $crate::private::thrift::IdType::UnknownField(x) => $crate::private::anyhow::bail!($crate::private::ErrorKind::InvalidThrift(
                        stringify!($typed).into(),
                        format!("unknown id type field: {}", x)
                    )),
                }
            }
            fn into_thrift(self) -> Self::Thrift {
                $thrift_typed($crate::private::thrift::IdType::Blake2(self.0.into_thrift()))
            }
            // Ids are special, their bytes serialization is NOT the thrift bytes serialization
            // as that is an union. Instead, it is simply the serialization of their blake2.
            fn from_bytes(b: &$crate::private::Bytes) -> Result<Self> {
                Self::from_bytes(b)
            }
            fn into_bytes(self) -> $crate::private::Bytes {
                self.into()
            }
        }

        impl BlobstoreKey for $typed {
            #[inline]
            fn blobstore_key(&self) -> String {
                format!(concat!($blobstore_key, ".blake2.{}"), self.0)
            }

            fn parse_blobstore_key(key: &str) -> $crate::private::anyhow::Result<Self> {
                let prefix = concat!($blobstore_key, ".blake2.");
                match key.strip_prefix(prefix) {
                    None => $crate::private::anyhow::bail!("{} is not a blobstore key for {}", key, stringify!($typed)),
                    Some(suffix) => Self::from_str(suffix),
                }
            }
        }

        impl TryFrom<$crate::private::Bytes> for $typed {
            type Error = $crate::private::anyhow::Error;
            #[inline]
            fn try_from(b: $crate::private::Bytes) -> $crate::private::anyhow::Result<Self> {
                Self::from_bytes(b)
            }
        }

        impl From<$typed> for $crate::private::Bytes {
            fn from(b: $typed) -> Self {
                Self::copy_from_slice(b.as_ref())
            }
        }

        impl std::str::FromStr for $typed {
            type Err = $crate::private::anyhow::Error;
            #[inline]
            fn from_str(s: &str) -> $crate::private::anyhow::Result<Self> {
                $crate::private::Blake2::from_str(s).map(Self::new)
            }
        }

        impl From<$crate::private::Blake2> for $typed {
            fn from(h: $crate::private::Blake2) -> $typed {
                Self::new(h)
            }
        }

        impl<'a> From<&'a $crate::private::Blake2> for $typed {
            fn from(h: &'a $crate::private::Blake2) -> $typed {
                Self::new(*h)
            }
        }

        impl AsRef<[u8]> for $typed {
            fn as_ref(&self) -> &[u8] {
                self.0.as_ref()
            }
        }

        impl std::fmt::Display for $typed {
            fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
                std::fmt::Display::fmt(&self.0, fmt)
            }
        }

        impl $crate::private::Arbitrary for $typed {
            fn arbitrary(g: &mut $crate::private::Gen) -> Self {
                Self::new($crate::private::Blake2::arbitrary(g))
            }

            fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
                $crate::private::empty_shrinker()
            }
        }

        impl $crate::private::Serialize for $typed {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: $crate::private::Serializer,
            {
                serializer.serialize_str(self.to_hex().as_str())
            }
        }

        impl<'de> $crate::private::Deserialize<'de> for $typed {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: $crate::private::Deserializer<'de>,
            {
                use std::str::FromStr;
                use std::result::Result::*;

                let hex = deserializer.deserialize_string($crate::private::Blake2HexVisitor)?;
                match $crate::private::Blake2::from_str(hex.as_str()) {
                    Ok(blake2) => Ok(Self::new(blake2)),
                    Err(error) => Err($crate::private::DeError::custom(error)),
                }
            }
        }

    }
}

#[macro_export]
macro_rules! impl_typed_hash_loadable {
    {
        hash_type => $typed: ident,
        value_type => $value_type: ty,
    } => {
        #[async_trait]
        impl Loadable for $typed
        {
            type Value = $value_type;

            async fn load<'a, B: Blobstore>(
                &'a self,
                ctx: &'a CoreContext,
                blobstore: &'a B,
            ) -> Result<Self::Value, LoadableError> {
                let id = *self;
                let blobstore_key = id.blobstore_key();
                let get = blobstore.get(ctx, &blobstore_key);

                let bytes = get.await?.ok_or(LoadableError::Missing(blobstore_key))?;
                let blob: Blob<$typed> = Blob::new(id, bytes.into_raw_bytes());
                <Self::Value as BlobstoreValue>::from_blob(blob).map_err(LoadableError::Error)
            }
        }

    }
}

#[macro_export]
macro_rules! impl_typed_context {
    {
        hash_type => $typed: ident,
        context_type => $typed_context: ident,
        context_key => $key: expr,
    } => {
        /// Context for incrementally computing a hash.
        #[derive(Clone)]
        pub struct $typed_context($crate::hash::Context);

        impl $typed_context {
            /// Construct a context.
            #[inline]
            pub fn new() -> Self {
                $typed_context($crate::hash::Context::new($key.as_bytes()))
            }

            #[inline]
            pub fn update<T>(&mut self, data: T)
            where
                T: AsRef<[u8]>,
            {
                self.0.update(data)
            }

            #[inline]
            pub fn finish(self) -> $typed {
                $typed(self.0.finish())
            }
        }

        impl $crate::typed_hash::IdContext for $typed_context {
            type Id = $typed;
            fn id_from_data(data: impl AsRef<[u8]>) -> $typed {
                let mut context = $typed_context::new();
                context.update(data);
                context.finish()
            }
        }
    }
}

#[macro_export]
macro_rules! impl_typed_hash {
    {
        hash_type => $typed: ident,
        thrift_hash_type => $thrift_hash_type: path,
        value_type => $value_type: ty,
        context_type => $typed_context: ident,
        context_key => $key: expr,
    } => {
        $crate::impl_typed_hash_no_context! {
            hash_type => $typed,
            thrift_type => $thrift_hash_type,
            blobstore_key => $key,
        }

        $crate::impl_typed_hash_loadable! {
            hash_type => $typed,
            value_type => $value_type,
        }

        $crate::impl_typed_context! {
            hash_type => $typed,
            context_type => $typed_context,
            context_key => $key,
        }

        impl MononokeId for $typed {
            #[inline]
            fn sampling_fingerprint(&self) -> u64 {
                self.0.sampling_fingerprint()
            }
        }

    }
}

macro_rules! impl_edenapi_hash_convert {
    ($this: ident, $edenapi: ident) => {
        impl From<$this> for $edenapi {
            fn from(v: $this) -> Self {
                $edenapi::from(v.0.into_inner())
            }
        }

        impl From<$edenapi> for $this {
            fn from(v: $edenapi) -> Self {
                $this::new(Blake2::from_byte_array(v.into()))
            }
        }
    };
}

impl_typed_hash! {
    hash_type => ChangesetId,
    thrift_hash_type => thrift::ChangesetId,
    value_type => BonsaiChangeset,
    context_type => ChangesetIdContext,
    context_key => "changeset",
}

impl_edenapi_hash_convert!(ChangesetId, EdenapiBonsaiChangesetId);

impl_typed_hash! {
    hash_type => ContentId,
    thrift_hash_type => thrift::ContentId,
    value_type => FileContents,
    context_type => ContentIdContext,
    context_key => "content",
}

impl_edenapi_hash_convert!(ContentId, EdenapiContentId);

impl_typed_hash! {
    hash_type => ContentChunkId,
    thrift_hash_type => thrift::ContentChunkId,
    value_type => ContentChunk,
    context_type => ContentChunkIdContext,
    context_key => "chunk",
}

impl_typed_hash! {
    hash_type => RawBundle2Id,
    thrift_hash_type => thrift::RawBundle2Id,
    value_type => RawBundle2,
    context_type => RawBundle2IdContext,
    context_key => "rawbundle2",
}

impl_typed_hash! {
    hash_type => FileUnodeId,
    thrift_hash_type => thrift::FileUnodeId,
    value_type => FileUnode,
    context_type => FileUnodeIdContext,
    context_key => "fileunode",
}

impl_typed_hash! {
    hash_type => ManifestUnodeId,
    thrift_hash_type => thrift::ManifestUnodeId,
    value_type => ManifestUnode,
    context_type => ManifestUnodeIdContext,
    context_key => "manifestunode",
}

impl_typed_hash! {
    hash_type => DeletedManifestV2Id,
    thrift_hash_type => thrift::DeletedManifestV2Id,
    value_type => DeletedManifestV2,
    context_type => DeletedManifestV2Context,
    context_key => "deletedmanifest2",
}

impl_typed_hash! {
    hash_type => ShardedMapNodeDMv2Id,
    thrift_hash_type => thrift::ShardedMapNodeId,
    value_type => ShardedMapNode<DeletedManifestV2Id>,
    context_type => ShardedMapNodeDMv2Context,
    context_key => "deletedmanifest2.mapnode",
}

impl_typed_hash! {
    hash_type => FsnodeId,
    thrift_hash_type => thrift::FsnodeId,
    value_type => Fsnode,
    context_type => FsnodeIdContext,
    context_key => "fsnode",
}

impl_typed_hash! {
    hash_type => RedactionKeyListId,
    thrift_hash_type => thrift::RedactionKeyListId,
    value_type => RedactionKeyList,
    context_type => RedactionKeyListIdContext,
    context_key => "redactionkeylist",
}

impl_edenapi_hash_convert!(FsnodeId, EdenapiFsnodeId);

impl_typed_hash! {
    hash_type => SkeletonManifestId,
    thrift_hash_type => thrift::SkeletonManifestId,
    value_type => SkeletonManifest,
    context_type => SkeletonManifestIdContext,
    context_key => "skeletonmanifest",
}

impl_typed_hash_no_context! {
    hash_type => ContentMetadataId,
    thrift_type => thrift::ContentMetadataId,
    blobstore_key => "content_metadata",
}

impl_typed_hash_loadable! {
    hash_type => ContentMetadataId,
    value_type => ContentMetadata,
}

impl_typed_hash_no_context! {
    hash_type => ContentMetadataV2Id,
    thrift_type => thrift::ContentMetadataV2Id,
    blobstore_key => "content_metadata2",
}

impl_typed_hash_loadable! {
    hash_type => ContentMetadataV2Id,
    value_type => ContentMetadataV2,
}

impl_typed_hash! {
    hash_type => FastlogBatchId,
    thrift_hash_type => thrift::FastlogBatchId,
    value_type => FastlogBatch,
    context_type => FastlogBatchIdContext,
    context_key => "fastlogbatch",
}

impl From<ContentId> for ContentMetadataId {
    fn from(content: ContentId) -> Self {
        Self(content.0)
    }
}

impl MononokeId for ContentMetadataId {
    #[inline]
    fn sampling_fingerprint(&self) -> u64 {
        self.0.sampling_fingerprint()
    }
}

impl From<ContentId> for ContentMetadataV2Id {
    fn from(content_id: ContentId) -> Self {
        Self(content_id.0)
    }
}

impl MononokeId for ContentMetadataV2Id {
    #[inline]
    fn sampling_fingerprint(&self) -> u64 {
        self.0.sampling_fingerprint()
    }
}

impl ChangesetIdPrefix {
    pub const fn new(blake2prefix: Blake2Prefix) -> Self {
        ChangesetIdPrefix(blake2prefix)
    }

    pub fn from_bytes<B: AsRef<[u8]> + ?Sized>(bytes: &B) -> Result<Self> {
        Blake2Prefix::from_bytes(bytes).map(Self::new)
    }

    #[inline]
    pub fn min_as_ref(&self) -> &[u8] {
        self.0.min_as_ref()
    }

    #[inline]
    pub fn max_as_ref(&self) -> &[u8] {
        self.0.max_as_ref()
    }

    #[inline]
    pub fn into_changeset_id(self) -> Option<ChangesetId> {
        self.0.into_blake2().map(ChangesetId)
    }
}

impl FromStr for ChangesetIdPrefix {
    type Err = <Blake2Prefix as FromStr>::Err;
    fn from_str(s: &str) -> result::Result<ChangesetIdPrefix, Self::Err> {
        Blake2Prefix::from_str(s).map(ChangesetIdPrefix)
    }
}

impl Display for ChangesetIdPrefix {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.0, fmt)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use bytes::Bytes;
    use quickcheck::quickcheck;

    quickcheck! {
        fn changesetid_thrift_roundtrip(h: ChangesetId) -> bool {
            let v = h.into_thrift();
            let sh = ChangesetId::from_thrift(v)
                .expect("converting a valid Thrift structure should always work");
            h == sh
        }

        fn contentid_thrift_roundtrip(h: ContentId) -> bool {
            let v = h.into_thrift();
            let sh = ContentId::from_thrift(v)
                .expect("converting a valid Thrift structure should always work");
            h == sh
        }
    }

    #[test]
    fn thrift_convert_bytes_consistent_for_ids() {
        let id = ShardedMapNodeDMv2Id::from_byte_array([1; 32]);
        let bytes_1 = Bytes::from(id.clone());
        let bytes_2 = ThriftConvert::into_bytes(id);
        assert_eq!(bytes_1, bytes_2);
        let rev_id_1_2: ShardedMapNodeDMv2Id = ThriftConvert::from_bytes(&bytes_1).unwrap();
        let rev_id_1_1 = ShardedMapNodeDMv2Id::from_bytes(bytes_1).unwrap();
        let rev_id_2_2: ShardedMapNodeDMv2Id = ThriftConvert::from_bytes(&bytes_2).unwrap();
        let rev_id_2_1 = ShardedMapNodeDMv2Id::from_bytes(bytes_2).unwrap();
        assert_eq!(rev_id_1_2, rev_id_1_1);
        assert_eq!(rev_id_1_1, rev_id_2_2);
        assert_eq!(rev_id_2_2, rev_id_2_1);
    }

    #[test]
    fn blobstore_key() {
        // These IDs are persistent, and this test is really to make sure that they don't change
        // accidentally.
        let id = ChangesetId::new(Blake2::from_byte_array([1; 32]));
        assert_eq!(id.blobstore_key(), format!("changeset.blake2.{}", id));

        let id = ContentId::new(Blake2::from_byte_array([1; 32]));
        assert_eq!(id.blobstore_key(), format!("content.blake2.{}", id));

        let id = ShardedMapNodeDMv2Id::from_byte_array([1; 32]);
        assert_eq!(
            id.blobstore_key(),
            format!("deletedmanifest2.mapnode.blake2.{}", id)
        );

        let id = ContentChunkId::from_byte_array([1; 32]);
        assert_eq!(id.blobstore_key(), format!("chunk.blake2.{}", id));

        let id = RawBundle2Id::from_byte_array([1; 32]);
        assert_eq!(id.blobstore_key(), format!("rawbundle2.blake2.{}", id));

        let id = FileUnodeId::from_byte_array([1; 32]);
        assert_eq!(id.blobstore_key(), format!("fileunode.blake2.{}", id));

        let id = ManifestUnodeId::from_byte_array([1; 32]);
        assert_eq!(id.blobstore_key(), format!("manifestunode.blake2.{}", id));

        let id = DeletedManifestV2Id::from_byte_array([1; 32]);
        assert_eq!(
            id.blobstore_key(),
            format!("deletedmanifest2.blake2.{}", id)
        );

        let id = FsnodeId::from_byte_array([1; 32]);
        assert_eq!(id.blobstore_key(), format!("fsnode.blake2.{}", id));

        let id = SkeletonManifestId::from_byte_array([1; 32]);
        assert_eq!(
            id.blobstore_key(),
            format!("skeletonmanifest.blake2.{}", id)
        );

        let id = ContentMetadataId::from_byte_array([1; 32]);
        assert_eq!(
            id.blobstore_key(),
            format!("content_metadata.blake2.{}", id)
        );

        let id = ContentMetadataV2Id::from_byte_array([1; 32]);
        assert_eq!(
            id.blobstore_key(),
            format!("content_metadata2.blake2.{}", id)
        );

        let id = FastlogBatchId::from_byte_array([1; 32]);
        assert_eq!(id.blobstore_key(), format!("fastlogbatch.blake2.{}", id));

        let id = RedactionKeyListId::from_byte_array([1; 32]);
        assert_eq!(
            id.blobstore_key(),
            format!("redactionkeylist.blake2.{}", id)
        );
    }

    #[test]
    fn test_serialize_deserialize() {
        let id = ChangesetId::new(Blake2::from_byte_array([1; 32]));
        let serialized = serde_json::to_string(&id).unwrap();
        let deserialized = serde_json::from_str(&serialized).unwrap();
        assert_eq!(id, deserialized);

        let id = ContentId::new(Blake2::from_byte_array([1; 32]));
        let serialized = serde_json::to_string(&id).unwrap();
        let deserialized = serde_json::from_str(&serialized).unwrap();
        assert_eq!(id, deserialized);

        let id = ShardedMapNodeDMv2Id::from_byte_array([1; 32]);
        let serialized = serde_json::to_string(&id).unwrap();
        let deserialized = serde_json::from_str(&serialized).unwrap();
        assert_eq!(id, deserialized);

        let id = ContentChunkId::from_byte_array([1; 32]);
        let serialized = serde_json::to_string(&id).unwrap();
        let deserialized = serde_json::from_str(&serialized).unwrap();
        assert_eq!(id, deserialized);

        let id = RawBundle2Id::from_byte_array([1; 32]);
        let serialized = serde_json::to_string(&id).unwrap();
        let deserialized = serde_json::from_str(&serialized).unwrap();
        assert_eq!(id, deserialized);

        let id = FileUnodeId::from_byte_array([1; 32]);
        let serialized = serde_json::to_string(&id).unwrap();
        let deserialized = serde_json::from_str(&serialized).unwrap();
        assert_eq!(id, deserialized);

        let id = ManifestUnodeId::from_byte_array([1; 32]);
        let serialized = serde_json::to_string(&id).unwrap();
        let deserialized = serde_json::from_str(&serialized).unwrap();
        assert_eq!(id, deserialized);

        let id = DeletedManifestV2Id::from_byte_array([1; 32]);
        let serialized = serde_json::to_string(&id).unwrap();
        let deserialized = serde_json::from_str(&serialized).unwrap();
        assert_eq!(id, deserialized);

        let id = FsnodeId::from_byte_array([1; 32]);
        let serialized = serde_json::to_string(&id).unwrap();
        let deserialized = serde_json::from_str(&serialized).unwrap();
        assert_eq!(id, deserialized);

        let id = SkeletonManifestId::from_byte_array([1; 32]);
        let serialized = serde_json::to_string(&id).unwrap();
        let deserialized = serde_json::from_str(&serialized).unwrap();
        assert_eq!(id, deserialized);

        let id = ContentMetadataId::from_byte_array([1; 32]);
        let serialized = serde_json::to_string(&id).unwrap();
        let deserialized = serde_json::from_str(&serialized).unwrap();
        assert_eq!(id, deserialized);

        let id = ContentMetadataV2Id::from_byte_array([1; 32]);
        let serialized = serde_json::to_string(&id).unwrap();
        let deserialized = serde_json::from_str(&serialized).unwrap();
        assert_eq!(id, deserialized);

        let id = FastlogBatchId::from_byte_array([1; 32]);
        let serialized = serde_json::to_string(&id).unwrap();
        let deserialized = serde_json::from_str(&serialized).unwrap();
        assert_eq!(id, deserialized);

        let id = RedactionKeyListId::from_byte_array([1; 32]);
        let serialized = serde_json::to_string(&id).unwrap();
        let deserialized = serde_json::from_str(&serialized).unwrap();
        assert_eq!(id, deserialized);
    }
}
