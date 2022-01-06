/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "eden/mononoke/mononoke_types/if/mononoke_types_thrift.thrift"

struct SkiplistEntry {
  1: RepoId repo_id;
  2: mononoke_types_thrift.ChangesetId cs_id;
  3: GenerationNum gen;
  4: SkiplistNodeType node_type;
} (rust.exhaustive)

typedef i32 RepoId (rust.newtype)

// Thrift does not support unsigned, so using i64 here
typedef i64 GenerationNum (rust.newtype)

struct CommitAndGenerationNumber {
  1: mononoke_types_thrift.ChangesetId cs_id;
  2: GenerationNum gen;
} (rust.exhaustive)

struct SkipEdges {
  1: list<CommitAndGenerationNumber> edges;
} (rust.exhaustive)

struct ParentEdges {
  1: list<CommitAndGenerationNumber> edges;
} (rust.exhaustive)

union SkiplistNodeType {
  1: SkipEdges SkipEdges;
  2: ParentEdges ParentEdges;
}
