/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

include "scm/mononoke/mononoke_types/if/mononoke_types_thrift.thrift"

typedef mononoke_types_thrift.IdType FastlogBatchId (rust.newtype)

// Structure that holds a commit graph, usually a history of a file
// or a directory hence the name. Semantically it stores list of
// (commit hash, [parent commit hashes]), however it's stored in compressed form
// described below. Compressed form is used to save space.
//
// FastlogBatch has two parts: `latest` and `previous_batches`.
// `previous_batches` field points to another FastlogBatch structures so
// FastlogBatch is a recursive structure. However normally `previous_batches`
// point to degenerate version of FastlogBatch with empty `previous_batches`
// i.e. we have only one level of nesting.
//
// In order to get the full list we need to get latest commits and concatenate
// it with lists from `previous_batches`.
//
// `latest` stores commit hashes and offsets to commit parents
// i.e. if offset is 1, then next commit is a parent of a current commit.
// For example, a list like
//
//  (HASH_A, [HASH_B])
//  (HASH_B, [])
//
//  will be encoded as
//  (HASH_A, [1])  # offset is 1, means next hash
//  (HASH_B, [])
//
//  A list with a merge
//  (HASH_A, [HASH_B, HASH_C])
//  (HASH_B, [])
//  (HASH_C, [])
//
//  will be encoded differently
//  (HASH_A, [1, 2])
//  (HASH_B, [])
//  (HASH_C, [])
//
// Note that offset might point to a commit in a next FastlogBatch or even
// point to batch outside of all previous_batches.
struct FastlogBatch {
  1: list<CompressedHashAndParents> latest,
  2: list<FastlogBatchId> previous_batches,
}

typedef i32 ParentOffset (rust.newtype)

struct CompressedHashAndParents {
  1: mononoke_types_thrift.ChangesetId cs_id,
  2: list<ParentOffset> parent_offsets,
}
