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

# Memcache constants. Should be change when we want to invalidate memcache
# entries
const i32 MC_CODEVER = 1;
const i32 MC_SITEVER = 0;

@rust.Exhaustive
struct BonsaiGlobalrevMappingEntry {
  1: i32 repo_id;
  2: optional id.ChangesetId bcs_id;
  3: i64 globalrev;
}
