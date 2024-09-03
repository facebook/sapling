/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "eden/mononoke/mononoke_types/serialization/id.thrift"

# Memcache constants. Should be changed when we want to invalidate memcache
# entries
const i32 MC_CODEVER = 0;
const i32 MC_SITEVER = 0;

struct CacheEntry {
  1: list<FetchedMappingEntry> mapping_entries;
} (rust.exhaustive)

struct FetchedMappingEntry {
  1: id.ChangesetId target_bcs_id;
  2: optional string maybe_version_name;
  3: optional SyncedCommitSourceRepo maybe_source_repo;
} (rust.exhaustive)

enum SyncedCommitSourceRepo {
  LARGE = 0,
  SMALL = 1,
}
