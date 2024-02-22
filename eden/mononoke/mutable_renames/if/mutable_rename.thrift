/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "eden/mononoke/mononoke_types/serialization/id.thrift"

# If you change this, you need to bump CODEVAR in caching.rs

struct PathHash {
  1: binary path;
  2: bool is_tree;
} (rust.exhaustive)

struct MutableRenameEntry {
  1: id.ChangesetId dst_cs_id;
  2: PathHash dst_path_hash;
  3: id.ChangesetId src_cs_id;
  4: binary src_path;
  5: PathHash src_path_hash;
  6: id.Blake2 src_unode;
  7: byte is_tree;
} (rust.exhaustive)

struct CachedMutableRenameEntry {
  1: optional MutableRenameEntry entry;
} (rust.exhaustive)

struct ChangesetIdSet {
  1: list<id.ChangesetId> cs_ids;
} (rust.exhaustive)
