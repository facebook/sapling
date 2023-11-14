/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "eden/mononoke/mononoke_types/if/mononoke_types_thrift.thrift"

# Memcache constants. Should be changed when we want to invalidate memcache
# entries
const i32 MC_CODEVER = 1;
const i32 MC_SITEVER = 0;

typedef i64 Generation (rust.newtype)

struct ChangesetNode {
  1: mononoke_types_thrift.ChangesetId cs_id;
  2: Generation generation;
  3: i64 skip_tree_depth;
  4: i64 p1_linear_depth;
} (rust.exhaustive)

struct ChangesetEdges {
  1: ChangesetNode node;
  2: list<ChangesetNode> parents;
  3: optional ChangesetNode merge_ancestor;
  4: optional ChangesetNode skip_tree_parent;
  5: optional ChangesetNode skip_tree_skew_ancestor;
  6: optional ChangesetNode p1_linear_skew_ancestor;
} (rust.exhaustive)

struct CachedChangesetEdges {
  1: ChangesetEdges edges;
  2: optional list<ChangesetEdges> prefetched_edges;
} (rust.exhaustive)

struct PreloadedEdges {
  1: list<CompactChangesetEdges> edges;
  2: optional i64 max_sql_id;
} (rust.exhaustive)

struct CompactChangesetEdges {
  1: mononoke_types_thrift.ChangesetId cs_id;
  2: i32 unique_id;
  3: i32 generation;
  4: i32 skip_tree_depth;
  5: i32 p1_linear_depth;
  6: list<i32> parents;
  7: optional i32 merge_ancestor;
  8: optional i32 skip_tree_parent;
  9: optional i32 skip_tree_skew_ancestor;
  10: optional i32 p1_linear_skew_ancestor;
} (rust.exhaustive)
