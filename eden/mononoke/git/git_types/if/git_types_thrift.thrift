/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "eden/mononoke/mononoke_types/serialization/bonsai.thrift"
include "eden/mononoke/mononoke_types/serialization/data.thrift"
include "eden/mononoke/mononoke_types/serialization/id.thrift"
include "eden/mononoke/mononoke_types/serialization/path.thrift"
include "eden/mononoke/mononoke_types/serialization/sharded_map.thrift"

struct BlobHandle {
  1: id.GitSha1 oid;
  2: i64 size;
  3: bonsai.FileType file_type;
} (rust.exhaustive)

struct TreeHandle {
  1: id.GitSha1 oid;
  2: i64 size;
} (rust.exhaustive)

struct MappedGitCommitId {
  1: id.GitSha1 oid;
} (rust.exhaustive)

union TreeMember {
  1: BlobHandle Blob;
  2: TreeHandle Tree;
}

struct Tree {
  1: TreeHandle handle;
  2: map<path.MPathElement, TreeMember> members;
} (rust.exhaustive)

/// The kind of Git objects that are allowed as entries in GitDeltaManifest
enum ObjectKind {
  Blob = 0,
  Tree = 1,
} (rust.exhaustive)

/// Struct representing a Git object's metadata along with the path at which it exists
struct ObjectEntry {
  1: id.GitSha1 oid;
  2: i64 size;
  3: ObjectKind kind;
  4: path.MPath path;
} (rust.exhaustive)

/// Struct representing the information required to generate the new object from the delta
/// and the base
struct ObjectDelta {
  1: id.ChangesetId origin;
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
typedef data.LargeBinary DeltaInstructionChunk (rust.newtype)

/// Identifier for accessing a specific delta instruction chunk
typedef id.Id DeltaInstructionChunkId (rust.newtype)

/// Identifier for accessing GitDeltaManifest
typedef id.Id GitDeltaManifestId (rust.newtype)

/// Manifest that contains an entry for each Git object that was added or modified as part of
/// a commit
struct GitDeltaManifest {
  /// The commit for which this GitDeltaManifest exists
  1: id.ChangesetId commit;
  /// The entries corresponding created / modified Git objects
  /// expressed as a map from null-separated MPath bytes -> GitDeltaManifestEntry
  2: sharded_map.ShardedMapNode entries;
} (rust.exhaustive)
