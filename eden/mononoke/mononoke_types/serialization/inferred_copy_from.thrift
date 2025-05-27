/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! ------------
//! IMPORTANT!!!
//! ------------
//! Do not change the order of the fields! Changing the order of the fields
//! results in compatible but *not* identical serializations, so hashes will
//! change.
//! ------------
//! IMPORTANT!!!
//! ------------

include "eden/mononoke/mononoke_types/serialization/id.thrift"
include "eden/mononoke/mononoke_types/serialization/path.thrift"
include "eden/mononoke/mononoke_types/serialization/sharded_map.thrift"
include "thrift/annotation/rust.thrift"

@rust.Exhaustive
struct InferredCopyFromEntry {
  1: id.ChangesetId from_csid;
  2: path.MPath from_path;
}

// InferredCopyFrom is a per-commit mapping of inferred copies in that commit,
// sharded by the destination path.
@rust.Exhaustive
struct InferredCopyFrom {
  // Map of MPath -> InferredCopyFromEntry
  1: sharded_map.ShardedMapV2Node subentries;
}
