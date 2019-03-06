// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

include "scm/mononoke/mononoke_types/if/mononoke_types_thrift.thrift"

struct SkiplistEntry {
  1: RepoId repo_id,
  2: mononoke_types_thrift.ChangesetId cs_id,
  3: GenerationNum gen,
  4: SkiplistNodeType node_type,
}

typedef i32 RepoId (hs.newtype)

 // Thrift does not support unsigned, so using i64 here
typedef i64 GenerationNum (hs.newtype)

struct CommitAndGenerationNumber {
  1: mononoke_types_thrift.ChangesetId cs_id,
  2: GenerationNum gen,
}

struct SkipEdges {
  1: list<CommitAndGenerationNumber> edges,
}

struct ParentEdges {
  1: list<CommitAndGenerationNumber> edges,
}

union SkiplistNodeType {
  1: SkipEdges SkipEdges,
  2: ParentEdges ParentEdges,
}
