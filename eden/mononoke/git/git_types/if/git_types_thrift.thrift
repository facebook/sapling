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

/// The kind of Git objects that are allowed as entries in GitDeltaManifestV2
enum ObjectKind {
  Blob = 0,
  Tree = 1,
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
