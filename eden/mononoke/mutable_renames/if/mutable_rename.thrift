/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "eden/mononoke/mononoke_types/if/mononoke_types_thrift.thrift"

# If you change this, you need to bump CODEVAR in caching.rs

struct PathHash {
  1: required binary path;
  2: required bool is_tree;
} (rust.exhaustive)

struct MutableRenameEntry {
  1: required mononoke_types_thrift.ChangesetId dst_cs_id;
  2: required PathHash dst_path_hash;
  3: required mononoke_types_thrift.ChangesetId src_cs_id;
  4: optional binary src_path;
  5: required PathHash src_path_hash;
  6: required mononoke_types_thrift.Blake2 src_unode;
  7: required byte is_tree;
} (rust.exhaustive)

struct CachedMutableRenameEntry {
  1: optional MutableRenameEntry entry;
} (rust.exhaustive)
