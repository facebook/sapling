/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "eden/mononoke/mononoke_types/if/mononoke_types_thrift.thrift"

struct BlobHandle {
  1: mononoke_types_thrift.GitSha1 oid;
  2: i64 size;
  3: mononoke_types_thrift.FileType file_type;
} (rust.exhaustive)

struct TreeHandle {
  1: mononoke_types_thrift.GitSha1 oid;
  2: i64 size;
} (rust.exhaustive)

struct MappedGitCommitId {
  1: mononoke_types_thrift.GitSha1 oid;
} (rust.exhaustive)

union TreeMember {
  1: BlobHandle Blob;
  2: TreeHandle Tree;
}

struct Tree {
  1: TreeHandle handle;
  2: map<mononoke_types_thrift.MPathElement, TreeMember> members;
} (rust.exhaustive)

/// The kind of Git objects that are allowed as entries in GitDeltaManifest
enum ObjectKind {
  Blob = 0,
  Tree = 1,
} (rust.exhaustive)

/// Struct representing a Git object's metadata along with the path at which it exists
struct ObjectEntry {
  1: mononoke_types_thrift.GitSha1 oid;
  2: i64 size;
  3: ObjectKind kind;
  4: mononoke_types_thrift.MPath path;
} (rust.exhaustive)

/// Struct representing the information required to generate the new object from the delta
/// and the base
struct ObjectDelta {
  1: mononoke_types_thrift.ChangesetId origin;
  2: ObjectEntry base;
  3: i64 instructions_chunk_count;
  4: i64 instructions_uncompressed_size;
  5: i64 instructions_compressed_size;
} (rust.exhaustive)

/// An entry in the GitDeltaManifest for a given commit
struct GitDeltaManifestEntry {
  1: ObjectEntry full;
  2: list<ObjectDelta> deltas;
} (rust.exhaustive)

/// The byte content of an individual chunk of DeltaInstructions
typedef mononoke_types_thrift.binary_bytes DeltaInstructionChunk (rust.newtype)

/// Identifier for accessing a specific delta instruction chunk
typedef mononoke_types_thrift.IdType DeltaInstructionChunkId (rust.newtype)

/// Identifier for accessing GitDeltaManifest
typedef mononoke_types_thrift.IdType GitDeltaManifestId (rust.newtype)

/// Manifest that contains an entry for each Git object that was added or modified as part of
/// a commit
struct GitDeltaManifest {
  /// The commit for which this GitDeltaManifest exists
  1: mononoke_types_thrift.ChangesetId commit;
  /// The entries corresponding created / modified Git objects
  /// expressed as a map from null-separated MPath bytes -> GitDeltaManifestEntry
  2: mononoke_types_thrift.ShardedMapNode entries;
} (rust.exhaustive)
