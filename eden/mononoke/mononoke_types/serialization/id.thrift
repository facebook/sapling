/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! ------------
//! IMPORTANT!!!
//! ------------
//! Do not change the order of the fields! Changing the order of the fields
//! results in compatible but *not* identical serializations, so hashes will
//! change.
//! ------------
//! IMPORTANT!!!
//! ------------

/// A blake2 hash.
typedef binary Blake2 (rust.newtype, rust.type = "smallvec::SmallVec<[u8; 32]>")

/// A blake2 hash.
typedef binary Blake3 (rust.newtype, rust.type = "smallvec::SmallVec<[u8; 32]>")

/// A sha1 hash.
typedef binary Sha1 (rust.newtype, rust.type = "smallvec::SmallVec<[u8; 20]>")

/// A sha256 hash.
typedef binary Sha256 (rust.newtype, rust.type = "smallvec::SmallVec<[u8; 32]>")

/// A Git sha-1 hash.
typedef binary GitSha1 (
  rust.newtype,
  rust.type = "smallvec::SmallVec<[u8; 20]>",
)

/// An id.  Mononoke Ids are generally blake2 hashes, however we may change this in the future.
union Id {
  1: Blake2 Blake2;
} (rust.ord)

typedef Id BasenameSuffixSkeletonManifestId (rust.newtype)
typedef Id BssmV3DirectoryId (rust.newtype)
typedef Id ChangesetId (rust.newtype)
typedef Id ContentChunkId (rust.newtype)
typedef Id ContentId (rust.newtype)
typedef Id ContentMetadataV2Id (rust.newtype)
typedef Id DeletedManifestId (rust.newtype)
typedef Id DeletedManifestV2Id (rust.newtype)
typedef Id FastlogBatchId (rust.newtype)
typedef Id FileUnodeId (rust.newtype)
typedef Id FsnodeId (rust.newtype)
typedef Id MPathHash (rust.newtype)
typedef Id ManifestUnodeId (rust.newtype)
typedef Id RawBundle2Id (rust.newtype)
typedef Id RedactionKeyListId (rust.newtype)
typedef Id ShardedMapNodeId (rust.newtype)
typedef Id ShardedMapV2NodeId (rust.newtype)
typedef Id SkeletonManifestId (rust.newtype)
typedef Id TestManifestId (rust.newtype)
typedef Id TestShardedManifestId (rust.newtype)
