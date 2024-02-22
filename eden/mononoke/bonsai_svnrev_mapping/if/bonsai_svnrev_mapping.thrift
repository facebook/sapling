/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "eden/mononoke/mononoke_types/serialization/id.thrift"

# Memcache constants. Should be change when we want to invalidate memcache
# entries
const i32 MC_CODEVER = 0;
const i32 MC_SITEVER = 0;

struct BonsaiSvnrevMappingEntry {
  1: i32 repo_id;
  2: id.ChangesetId bcs_id;
  3: i64 svnrev;
} (rust.exhaustive)
