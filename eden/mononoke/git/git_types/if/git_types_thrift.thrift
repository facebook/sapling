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
  3: mononoke_types_thrift.binary_bytes encoded_instructions;
} (rust.exhaustive)

/// An entry in the GitDeltaManifest for a given commit
struct GitDeltaManifestEntry {
  1: ObjectEntry full;
  2: list<ObjectDelta> deltas;
} (rust.exhaustive)
