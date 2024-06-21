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

/// Manifest that contains an entry for each Git object that was added or modified as part of
/// a commit. The object needs to be different from all objects at the same path in all parents
/// for it to be included.
struct GitDeltaManifestV2 {
  /// The entries corresponding created / modified Git objects
  /// expressed as a map from null-separated MPath bytes -> GDMV2Entry
  1: sharded_map.ShardedMapV2Node entries;
}

/// Identifier for GitDeltaManifestV2 blob
typedef id.Id GitDeltaManifestV2Id (rust.newtype)

/// An entry in the GitDeltaManifestV2 corresponding to a path
struct GDMV2Entry {
  /// The full object that this entry represents
  1: GDMV2ObjectEntry full_object;
  /// A list of entries corresponding to ways to represent this object
  /// as a delta
  2: list<GDMV2DeltaEntry> deltas;
}

/// Struct representing a Git object's metadata in GitDeltaManifestV2.
/// Contains inlined bytes of the object if it's considered small enough.
struct GDMV2ObjectEntry {
  1: id.GitSha1 oid;
  2: i64 size;
  3: ObjectKind kind;
  4: optional data.LargeBinary inlined_bytes;
}

/// Struct indicating a Git blob in GitDeltaManifestV2
struct GDMV2Blob {} (rust.exhaustive)
/// Struct indicating a Git tree in GitDeltaManifestV2
struct GDMV2Tree {} (rust.exhaustive)

/// Struct representing a delta in GitDeltaManifestV2
struct GDMV2DeltaEntry {
  1: id.ChangesetId parent;
  2: GDMV2ObjectEntry base_object;
  3: path.MPath base_object_path;
  4: GDMV2Instructions instructions;
}

/// Struct representing the instructions of a delta in GitDeltaManifestV2
struct GDMV2Instructions {
  1: i64 uncompressed_size;
  2: i64 compressed_size;
  3: GDMV2InstructionBytes instruction_bytes;
}

/// Struct representing the bytes of the instructions of a delta in GitDeltaManifestV2
union GDMV2InstructionBytes {
  /// The instruction bytes are stored inlined
  1: data.LargeBinary inlined;
  /// The instruction bytes are stored in separate chunked blobs, with only
  /// a list of their ids stored inline
  2: list<GDMV2InstructionsChunkId> chunked;
}

/// Identifier for a chunk of delta instructions in GitDeltaManifestV2
typedef id.Id GDMV2InstructionsChunkId (rust.newtype)

/// The byte content of an individual chunk of delta instructions in GitDeltaManifestV2
typedef data.LargeBinary GDMV2InstructionsChunk (rust.newtype)
