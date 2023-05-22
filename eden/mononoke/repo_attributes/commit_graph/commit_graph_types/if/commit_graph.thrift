/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "eden/mononoke/mononoke_types/if/mononoke_types_thrift.thrift"

# Memcache constants. Should be changed when we want to invalidate memcache
# entries
const i32 MC_CODEVER = 0;
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
