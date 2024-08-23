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
include "eden/mononoke/mononoke_types/serialization/sharded_map.thrift"

struct DeletedManifestV2 {
  1: optional id.ChangesetId linknode;
  // Map of MPathElement -> DeletedManifestV2Id
  2: sharded_map.ShardedMapNode subentries;
} (rust.exhaustive)
