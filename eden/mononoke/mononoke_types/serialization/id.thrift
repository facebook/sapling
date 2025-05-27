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

include "thrift/annotation/rust.thrift"

/// A blake2 hash.
@rust.NewType
@rust.Type{name = "smallvec::SmallVec<[u8; 32]>"}
typedef binary Blake2

/// A blake3 hash.
@rust.NewType
@rust.Type{name = "smallvec::SmallVec<[u8; 32]>"}
typedef binary Blake3

/// A sha1 hash.
@rust.NewType
@rust.Type{name = "smallvec::SmallVec<[u8; 20]>"}
typedef binary Sha1

/// A sha256 hash.
@rust.NewType
@rust.Type{name = "smallvec::SmallVec<[u8; 32]>"}
typedef binary Sha256

/// A Git sha-1 hash.
@rust.NewType
@rust.Type{name = "smallvec::SmallVec<[u8; 20]>"}
typedef binary GitSha1

/// An id.  Mononoke Ids are generally blake2 hashes, however we may change this in the future.
@rust.Ord
union Id {
  1: Blake2 Blake2;
}

@rust.NewType
typedef Id BasenameSuffixSkeletonManifestId
@rust.NewType
typedef Id BssmV3DirectoryId
@rust.NewType
typedef Id SkeletonManifestV2Id
@rust.NewType
typedef Id CaseConflictSkeletonManifestId
@rust.NewType
typedef Id ChangesetId
@rust.NewType
typedef Id ContentChunkId
@rust.NewType
typedef Id ContentId
@rust.NewType
typedef Id ContentManifestId
@rust.NewType
typedef Id ContentMetadataV2Id
@rust.NewType
typedef Id DeletedManifestId
@rust.NewType
typedef Id DeletedManifestV2Id
@rust.NewType
typedef Id FastlogBatchId
@rust.NewType
typedef Id FileUnodeId
@rust.NewType
typedef Id FsnodeId
@rust.NewType
typedef Id InferredCopyFromId
@rust.NewType
typedef Id MPathHash
@rust.NewType
typedef Id ManifestUnodeId
@rust.NewType
typedef Id RawBundle2Id
@rust.NewType
typedef Id RedactionKeyListId
@rust.NewType
typedef Id ShardedMapNodeId
@rust.NewType
typedef Id ShardedMapV2NodeId
@rust.NewType
typedef Id SkeletonManifestId
@rust.NewType
typedef Id TestManifestId
@rust.NewType
typedef Id TestShardedManifestId
