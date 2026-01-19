/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "eden/mononoke/mononoke_types/serialization/id.thrift"
include "thrift/annotation/rust.thrift"
include "thrift/annotation/thrift.thrift"

@thrift.AllowLegacyMissingUris
package;

# Memcache constants. Should be changed when we want to invalidate memcache
# entries
const i32 MC_CODEVER = 2;
const i32 MC_SITEVER = 0;

@rust.NewType
typedef i64 Generation

@rust.Exhaustive
struct ChangesetNode {
  1: id.ChangesetId cs_id;
  2: Generation generation;
  3: i64 skip_tree_depth;
  4: i64 p1_linear_depth;
  5: optional Generation subtree_source_generation; // omitted if the same as generation
  6: optional i64 subtree_source_depth; // omitted if the same as skip_tree_depth
}

@rust.Exhaustive
struct ChangesetEdges {
  1: ChangesetNode node;
  2: list<ChangesetNode> parents;
  3: optional ChangesetNode merge_ancestor;
  4: optional ChangesetNode skip_tree_parent;
  5: optional ChangesetNode skip_tree_skew_ancestor;
  6: optional ChangesetNode p1_linear_skew_ancestor;
  7: optional list<ChangesetNode> subtree_sources;
  8: optional ChangesetNode subtree_or_merge_ancestor; // omitted if the same as merge_ancestor
  9: optional ChangesetNode subtree_source_parent; // omitted if the same as skip_tree_parent
  10: optional ChangesetNode subtree_source_skew_ancestor; // omitted if the same as skip_tree_skew_ancestor
}

@rust.Exhaustive
struct CachedChangesetEdges {
  1: ChangesetEdges edges;
  2: optional list<ChangesetEdges> prefetched_edges;
}

@rust.Exhaustive
struct PreloadedEdges {
  1: list<CompactChangesetEdges> edges;
  2: optional i64 max_sql_id;
}

@rust.Exhaustive
struct CompactChangesetEdges {
  1: id.ChangesetId cs_id;
  2: i32 unique_id;
  3: i32 generation;
  4: i32 skip_tree_depth;
  5: i32 p1_linear_depth;
  6: list<i32> parents;
  7: optional i32 merge_ancestor;
  8: optional i32 skip_tree_parent;
  9: optional i32 skip_tree_skew_ancestor;
  10: optional i32 p1_linear_skew_ancestor;
  11: optional i32 subtree_source_generation; // omitted if the same as generation
  12: optional i32 subtree_source_depth; // omitted if the same as skip_tree_depth
  13: optional list<i32> subtree_sources; // omitted if empty
  14: optional i32 subtree_or_merge_ancestor; // omitted if the same as merge_ancestor
  15: optional i32 subtree_source_parent; // omitted if the same as skip_tree_parent
  16: optional i32 subtree_source_skew_ancestor; // omitted if the same as skip_tree_skew_ancestor
}
